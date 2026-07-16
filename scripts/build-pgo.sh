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
bundled_profiles=(
    fenrin japanese ancient-roman slavic klingon oceanic
    uralic caucasian aurelian obsidian
)

echo "Training core and distinct-name paths across bundled profiles"
for profile in "${bundled_profiles[@]}"; do
    LLVM_PROFILE_FILE="$profiles_root/$profile-benchmark-%m-%p.profraw" \
        "$benchmark" --config "$profile" 100000 >/dev/null
    LLVM_PROFILE_FILE="$profiles_root/$profile-cli-%m-%p.profraw" \
        "$fenrin" --seed 42 --config "$profile" 10000 >/dev/null
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
echo "Optimized CLI: $optimized_target/release/fenrin"
echo "Optimized benchmark: $optimized_target/release/examples/benchmark"
