# Benchmark-Guided Optimization Loop

Use this process to let a coding agent optimize Fenrin without changing its
output or hiding regressions. Optimize either name generation or SAS encoding
in one loop, not both at once.

## Loop limits

- Try at most **8 candidate changes**.
- Stop early after **3 consecutive rejected candidates**.
- Make one small, reversible change per iteration.
- Measure each baseline and candidate **3 times** and compare medians.
- Treat a 2–5% improvement as uncertain; measure it 5 times before deciding.

These limits are long enough to explore several real hypotheses while avoiding
endless micro-optimization.

## Before the loop

1. Work in a clean, isolated branch or worktree. Never discard unrelated local
   changes.
2. Choose one primary target: `names` or `sas`.
3. Freeze `examples/benchmark.rs` and the benchmark commands for the entire
   loop. If the benchmark itself must change, stop, fix it separately, and
   establish a new baseline.
4. Keep the machine conditions stable: use release mode, close heavy background
   work, and do not run benchmarks in parallel.
5. Confirm the starting point:

   ```sh
   cargo fmt -- --check
   cargo test --all-targets
   cargo clippy --all-targets -- -D warnings
   ```

## Establish the baseline

Run the relevant command three times and record the median throughput.

For name generation, use two structurally different profiles:

```sh
cargo run --release --example benchmark -- 100000
cargo run --release --example benchmark -- --config japanese 100000
```

Also capture deterministic output for later comparison:

```sh
cargo run --quiet -- --seed 42 --config fenrin 1000 > /tmp/fenrin-before.txt
cargo run --quiet -- --seed 42 --config japanese 1000 > /tmp/japanese-before.txt
```

For SAS encoding:

```sh
cargo run --release --example benchmark -- --sas 1000000
```

Record results in a small working table:

| Iteration | Hypothesis | Median before | Median after | Change | Quality | Decision |
| --- | --- | ---: | ---: | ---: | --- | --- |
| 0 | Baseline | — | value | — | baseline | keep |

For names, also record `duplicate %`, `pair matches`, `collision bits`,
`effective diversity`, and `max freq`.

## Each iteration

1. **Choose one hypothesis.** Inspect or profile the current hotspot and state
   what work will be removed, such as an allocation, repeated scan, lookup, or
   sort. Do not change generation constants merely to make the benchmark faster.
2. **Implement only that change.** Keep the diff small enough to revert without
   affecting previously accepted optimizations.
3. **Run correctness gates:**

   ```sh
   cargo fmt -- --check
   cargo test --all-targets
   cargo clippy --all-targets -- -D warnings
   ```

4. **Check behavior.** Name-generation optimizations must reproduce the saved
   seeded outputs:

   ```sh
   cargo run --quiet -- --seed 42 --config fenrin 1000 > /tmp/fenrin-after.txt
   cargo run --quiet -- --seed 42 --config japanese 1000 > /tmp/japanese-after.txt
   cmp /tmp/fenrin-before.txt /tmp/fenrin-after.txt
   cmp /tmp/japanese-before.txt /tmp/japanese-after.txt
   ```

   Both `cmp` commands must exit successfully. SAS compatibility is guarded by
   the existing exhaustive SAS tests.
5. **Benchmark three times** with the exact baseline commands. Record the median
   `names/second` or `phrases/second`; do not select only the best run.
6. **Decide:**

   - Keep a change with at least 5% median improvement.
   - For a 2–5% improvement, run five measurements and keep it only if the new
     median is at least 3% faster.
   - Reject a change below 2%, any correctness failure, or a regression greater
     than 2% in the secondary name profile.
   - Name quality statistics must remain identical for a behavior-preserving
     optimization. Treat an intentional distribution change as a separate task.

7. **Keep or revert.** Commit an accepted change with a focused message such as
   `perf(grammar): avoid repeated surface allocation`. Revert only the current
   candidate when it is rejected; never use `git reset --hard`.
8. Update the working table. An accepted change becomes the next iteration's
   baseline and resets the consecutive-rejection count.

## Stop conditions

Stop when any of these is true:

- 8 candidates have been tried.
- 3 candidates in a row were rejected.
- No measured hotspot or concrete hypothesis remains.
- Results vary by more than 5% between repeated runs; stabilize the environment
  before continuing.
- A proposed speedup requires changing output, public APIs, profile files, or
  SAS compatibility. Report it as a separate proposal instead.

## Final verification

Run the full checks and large benchmarks once after the last accepted change:

```sh
cargo fmt -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo run --release --example benchmark
cargo run --release --example benchmark -- --config japanese 1000000
cargo run --release --example benchmark -- --sas
```

For a grammar-engine optimization, also smoke-test every bundled profile:

```sh
for profile in fenrin japanese ancient-roman slavic klingon oceanic uralic caucasian aurelian obsidian; do
  cargo run --release --example benchmark -- --config "$profile" 100000
done
```

The final report should list accepted and rejected hypotheses, starting and
ending medians, cumulative improvement, verification results, and any remaining
hotspot worth investigating in a future loop.
