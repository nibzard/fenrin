#!/usr/bin/env bash
# ABOUTME: Builds and trains a reproducible profile-guided Fenrin release.
# ABOUTME: Keeps instrumented data and optimized artifacts in an isolated run directory.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
run_name=${1:-$(date -u +%Y%m%dT%H%M%SZ)}
run_root="$repo_root/target/pgo/$run_name"
profiles_root="$run_root/profiles"
instrumented_target="$run_root/instrumented"
optimized_target="$run_root/optimized"
merged_profile="$run_root/fenrin.profdata"
manifest="$run_root/manifest.txt"

raw_training_seed=42
raw_training_count=100000
raw_training_sessions=10
cli_training_seed=42
cli_training_count=10000
bundled_profiles=(
    fenrin japanese ancient-roman slavic klingon oceanic
    uralic caucasian aurelian obsidian
)

if [[ -e "$run_root" ]]; then
    echo "PGO run directory already exists: $run_root" >&2
    echo "Choose a different run name." >&2
    exit 2
fi

host=$(rustc -vV | sed -n 's/^host: //p')
llvm_profdata="$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-profdata"
if [[ ! -x "$llvm_profdata" ]]; then
    echo "rustc's llvm-profdata was not found at $llvm_profdata" >&2
    exit 2
fi

mkdir -p "$profiles_root"

source_revision=$(git -C "$repo_root" rev-parse --verify HEAD 2>/dev/null || true)
source_revision=${source_revision:-unknown}
if [[ "$source_revision" == unknown ]]; then
    source_state=unknown
elif [[ -n "$(git -C "$repo_root" status --porcelain --untracked-files=normal)" ]]; then
    source_state=dirty
else
    source_state=clean
fi
rustc_version=$(rustc --version)
cargo_version=$(cargo --version)
llvm_profdata_version=$(
    "$llvm_profdata" --version | sed -n 's/^[[:space:]]*//; /LLVM version/p'
)

{
    printf 'format=fenrin-pgo-v1\n'
    printf 'source_revision=%s\n' "$source_revision"
    printf 'source_state=%s\n' "$source_state"
    printf 'rustc=%s\n' "$rustc_version"
    printf 'host=%s\n' "$host"
    printf 'cargo=%s\n' "$cargo_version"
    printf 'llvm_profdata=%s\n' "$llvm_profdata_version"
    printf 'profiles=%s\n' "${bundled_profiles[*]}"
    printf 'raw_seed=%s\n' "$raw_training_seed"
    printf 'raw_count=%s\n' "$raw_training_count"
    printf 'raw_sessions=%s\n' "$raw_training_sessions"
    printf 'raw_command=benchmark --measure raw --config <profile> --seed %s --sessions %s %s\n' \
        "$raw_training_seed" "$raw_training_sessions" "$raw_training_count"
    printf 'cli_seed=%s\n' "$cli_training_seed"
    printf 'cli_count=%s\n' "$cli_training_count"
    printf 'cli_command=fenrin --seed %s --config <profile> %s\n' \
        "$cli_training_seed" "$cli_training_count"
} >"$manifest"

echo "Building instrumented release artifacts"
(
    cd "$repo_root"
    CARGO_TARGET_DIR="$instrumented_target" \
        RUSTFLAGS="-Cprofile-generate=$profiles_root" \
        cargo build --release --bins --example benchmark
)

# Cargo may execute instrumented build scripts while compiling. Their default
# profiles describe the build rather than Fenrin's runtime workload, so exclude
# them before collecting the deliberately named training profiles below.
find "$profiles_root" -maxdepth 1 -type f -name '*.profraw' -delete

benchmark="$instrumented_target/release/examples/benchmark"
fenrin="$instrumented_target/release/fenrin"

echo "Training core and distinct-name paths across bundled profiles"
for profile in "${bundled_profiles[@]}"; do
    LLVM_PROFILE_FILE="$profiles_root/$profile-benchmark-%m-%p.profraw" \
        "$benchmark" --measure raw --config "$profile" \
        --seed "$raw_training_seed" --sessions "$raw_training_sessions" \
        "$raw_training_count" >/dev/null
    LLVM_PROFILE_FILE="$profiles_root/$profile-cli-%m-%p.profraw" \
        "$fenrin" --seed "$cli_training_seed" --config "$profile" \
        "$cli_training_count" >/dev/null
done

mapfile -t training_profiles < <(
    find "$profiles_root" -maxdepth 1 -type f -name '*.profraw' -print | sort
)
expected_profiles=$((2 * ${#bundled_profiles[@]}))
if (( ${#training_profiles[@]} != expected_profiles )); then
    echo "Expected $expected_profiles training profiles, found ${#training_profiles[@]}" >&2
    exit 2
fi

"$llvm_profdata" merge --failure-mode=all \
    -o "$merged_profile" "${training_profiles[@]}"

echo "Building profile-optimized release artifacts"
(
    cd "$repo_root"
    CARGO_TARGET_DIR="$optimized_target" \
        RUSTFLAGS="-Cprofile-use=$merged_profile -Cllvm-args=-pgo-warn-missing-function" \
        cargo build --release --bins --example benchmark
)

echo "PGO profile: $merged_profile"
echo "Training manifest: $manifest"
echo "Optimized CLI: $optimized_target/release/fenrin"
echo "Optimized benchmark: $optimized_target/release/examples/benchmark"
