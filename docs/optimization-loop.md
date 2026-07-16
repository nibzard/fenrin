# Benchmark-Guided Optimization Loop

Use fixed-work paired measurements for optimization decisions. The primary
metric is distinct names completed per second through the production session:
generation, exact deduplication, first-seen ordering, line formatting, and
buffering into a counting sink. Grammar parsing and process startup are outside
the timed region.

Raw owned-`String` draws per second remain available as a diagnostic. They are
not a substitute for the distinct-session result.

The old three/five-run median tables remain historical evidence only. Existing
human-readable benchmark commands still work, but new optimization decisions
must use `--measure` records and the paired runner. A prebuilt binary from
before the `fenrin-fixed-v1` protocol cannot participate in this design; first
apply the benchmark-only protocol commit to both comparison revisions.

## Freeze the experiment

1. Work in an isolated branch or worktree and keep unrelated changes intact.
2. Commit benchmark changes separately, then build baseline and candidate from
   revisions that both contain the same benchmark protocol.
3. Choose profiles, count, timed sessions, work seeds, confidence rule, and the
   maximum number of candidates before measuring a candidate.
4. Keep machine conditions stable. Do not run benchmark series concurrently.
5. Capture deterministic output and run the correctness gates:

   ```sh
   cargo fmt -- --check
   cargo test --all-targets
   cargo clippy --all-targets -- -D warnings

   cargo run --quiet -- --seed 42 --config fenrin 1000 > /tmp/fenrin-before.txt
   cargo run --quiet -- --seed 42 --config japanese 1000 > /tmp/japanese-before.txt
   ```

## Fixed-work measurements

Build the dependency-free benchmark once in each worktree. Separate target
directories keep both executables available. Run the first build in the
baseline worktree and the second in the candidate worktree:

```sh
CARGO_TARGET_DIR=/tmp/fenrin-a cargo build --release --example benchmark
CARGO_TARGET_DIR=/tmp/fenrin-b cargo build --release --example benchmark
cargo build --release --example paired
```

The existing exploratory benchmark CLI is unchanged:

```sh
cargo run --release --example benchmark -- 100000
cargo run --release --example benchmark -- --config japanese 100000
cargo run --release --example benchmark -- --sas 1000000
```

Fixed modes emit one tab-delimited, versioned record:

```sh
/tmp/fenrin-a/release/examples/benchmark \
  --measure distinct --config fenrin --seed 42 --sessions 50 10000

/tmp/fenrin-a/release/examples/benchmark \
  --measure raw --config japanese --seed 42 --sessions 10 100000
```

`distinct` completes exactly `count` unique names in each session and includes
the real CLI path. `raw` performs exactly `count`
`Grammar::generate_name` calls per session. Both run one full untimed warmup
session, then a fixed, non-adaptive number of timed sessions. Each session has
a fresh RNG derived deterministically from the base seed. Grammar parsing stays
outside timing; buffer allocation and destruction are inside distinct timing.

The default 50 sessions make a 10,000-name observation roughly half a second or
longer on the optimized profiles. Increase the fixed session count before an
experiment if A/A records are materially shorter. The record includes base
seed, count, sessions, requested/completed work, attempts, elapsed nanoseconds,
throughput, and formatted byte count.

## Reproducible PGO training

Build an instrumented binary, train it, and build the profile-optimized binary
with one isolated command:

```sh
scripts/build-pgo.sh <unique-run-name>
```

The script trains every bundled grammar with explicit fixed-work raw sessions
and an explicit seeded CLI distinct-name run. Training counts, session counts,
seeds, source revision/state, and tool versions are recorded in
`target/pgo/<run-name>/manifest.txt`. Build-time profiles are discarded, and
the merge fails unless every planned runtime profile was produced. Treat the
resulting optimized benchmark as another candidate and evaluate it with the
same paired protocol; do not compare build durations or adaptive training runs.

## Calibrate noise with A/A

Run the same prebuilt binary under both labels before testing candidates. The
runner generates its entire ABBA/BAAB schedule from `--order-seed` and prints
the plan before the first process starts. Repeating `--seed` cycles a fixed,
preregistered base-seed set across blocks.

```sh
target/release/examples/paired \
  --aa /tmp/fenrin-a/release/examples/benchmark \
  --mode distinct --config fenrin --count 10000 --sessions 50 \
  --blocks 16 --order-seed 731 \
  --seed 42 --seed 314159 --seed 271828 --seed 161803 \
  --target-speedup 3 | tee /tmp/fenrin-aa.log
```

`CALIBRATION` reports the observed block log-ratio standard deviation and an
approximate block count for 80% power at the requested speedup. Choose and
record the A/B block count before seeing candidate results; normally use at
least 16 blocks. A/A is a noise estimate, not evidence of a performance change.

## Compare prebuilt A/B binaries

```sh
target/release/examples/paired \
  --baseline /tmp/fenrin-a/release/examples/benchmark \
  --candidate /tmp/fenrin-b/release/examples/benchmark \
  --mode distinct --config fenrin --count 10000 --sessions 50 \
  --blocks 24 --order-seed 9127 \
  --seed 42 --seed 314159 --seed 271828 --seed 161803 \
  | tee /tmp/fenrin-ab.log
```

Run the same frozen design for Japanese. Use `--mode raw` only to locate whether
a result comes from generation itself or session overhead. An explicit schedule
can be archived and replayed with, for example,
`--schedule ABBA,BAAB,ABBA,BAAB`; it cannot be combined with `--blocks` or
`--order-seed`.

Each block contains two A and two B observations. The runner computes one
paired log-throughput ratio per block, then reports:

- the geometric candidate/baseline speedup;
- the standard deviation of block log ratios;
- a Student-t 95% one-sided lower confidence bound.

Every complete, valid preregistered block is retained, including unusually fast
or slow blocks. A block is invalid only when a process cannot start, exits
unsuccessfully, emits a malformed record, or reports different requested work.
Invalid blocks are logged, never replaced, and cause a failing runner exit.

The per-candidate 95% result is explicitly labeled
`scope=exploratory_per_candidate`. It is a screening result, not a
familywise-error-controlled claim across an optimization campaign. Use its
lower bound with the preregistered behavior and secondary-profile rules to
choose candidates, then reserve the confirmatory claim for fresh held-out data.

## Each candidate

1. State one concrete hypothesis and the work it should remove.
2. Make one reversible change.
3. Run formatting, tests, Clippy, and compare saved seeded outputs:

   ```sh
   cargo run --quiet -- --seed 42 --config fenrin 1000 > /tmp/fenrin-after.txt
   cargo run --quiet -- --seed 42 --config japanese 1000 > /tmp/japanese-after.txt
   cmp /tmp/fenrin-before.txt /tmp/fenrin-after.txt
   cmp /tmp/japanese-before.txt /tmp/japanese-after.txt
   ```

4. Build a prebuilt candidate binary and run the frozen paired designs.
5. Keep or reject using only the preregistered screening and behavior rules.
   Do not add runs, remove observations, or change the analysis after seeing the
   result.
6. Log the hypothesis, commit, full `PLAN`/`RESULT` records, quality checks, and
   decision. An accepted candidate becomes the next baseline.

For an intentional distribution change, define separate multi-seed quality
criteria before timing: hard-constraint failures, distinct yield, collision
probability/effective diversity, shape frequencies, and soft-score
distribution. Do not use throughput to waive those criteria.

## Held-out confirmation

After the last accepted candidate, use fresh work seeds and a new order seed for
one optimized-versus-start comparison. Add `--held-out` so the record is labeled
`scope=held_out_confirmation`:

```sh
target/release/examples/paired \
  --baseline /tmp/fenrin-start/release/examples/benchmark \
  --candidate /tmp/fenrin-final/release/examples/benchmark \
  --mode distinct --config fenrin --count 10000 --sessions 50 \
  --blocks 24 --order-seed 880301 --held-out \
  --seed 8675309 --seed 11235813 --seed 299792458 --seed 4294967291
```

Do not tune from this result. The 95% one-sided lower bound is the campaign's
confirmatory performance result. Also run all correctness gates, seeded
comparisons, the Japanese held-out design, and smoke tests for every bundled
profile. Record every held-out observation and final bound in `LOG.md`.
