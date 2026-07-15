# Name-generation optimization log

Date: 2026-07-15  
Branch: `perf/name-generation-10-rounds`  
Primary target: `names`

This run follows `docs/optimization-loop.md`, with one explicit limit override:
the requested ten candidate rounds replace the document's default cap of eight.
All benchmark commands, seeded snapshots, correctness gates, quality requirements,
and accept/reject thresholds remain frozen for the run.

## Frozen procedure

- Primary benchmark: `cargo run --release --example benchmark -- 100000`
- Secondary benchmark: `cargo run --release --example benchmark -- --config japanese 100000`
- Each profile is measured three times and medians are compared.
- A 2–5% primary improvement is remeasured five times and must retain at least 3%.
- A candidate is rejected below 2%, on any correctness or output-equivalence failure,
  or when the Japanese secondary profile regresses by more than 2%.
- Correctness gates: formatting, all-target tests, clippy with warnings denied, and
  byte-for-byte comparison of 1,000 seeded outputs for both profiles.
- The benchmark source and commands are unchanged throughout the loop.

## Environment and starting verification

- CPU: 12th Gen Intel Core i9-12900HK, 4 online virtual CPUs, KVM guest.
- Rust dependencies were fetched before baseline measurement.
- `perf` hardware counters were unavailable because `perf_event_paranoid=4`.
- `cargo fmt -- --check`: pass.
- `cargo test --all-targets`: pass (53 tests).
- `cargo clippy --all-targets -- -D warnings`: pass.
- Seeded output snapshot hashes:
  - fenrin: `6da69b54e4638bd55021a2f78405afc0ae3b55b09ddd61135b5358710566a17a`
  - japanese: `133e62c7e1b9d2903fd9dcb9def6d0d9dabe8504400b82c0e96200971ce4d3b9`

## Iterations

| Iteration | Hypothesis | Fenrin before | Fenrin after | Change | Japanese before | Japanese after | Secondary change | Quality | Decision |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| 0 | Baseline | — | 94,987 | — | — | 47,757 | — | baseline | keep |
| 1 | Precompute feature-selector membership instead of hashing strings during every constraint scan | 94,987 | 272,491 | +186.87% | 47,757 | 79,972 | +67.46% | identical | keep |

Baseline raw measurements (names/second):

- Fenrin: 96,479; 93,699; 94,987 (median 94,987; spread 2.97%).
- Japanese: 47,757; 49,409; 47,614 (median 47,757; spread 3.77%).

Baseline quality statistics were identical across all repetitions:

| Profile | duplicate % | pair matches | collision bits | effective diversity | max freq |
| --- | ---: | ---: | ---: | ---: | ---: |
| fenrin | 20.989% | 31,211 | 17.29 | 1.602e5 | 10 |
| japanese | 16.447% | 22,833 | 17.74 | 2.190e5 | 9 |

### Round 1: precompute feature-selector membership

- Removed work: repeated `HashMap<String, String>` lookups in every hard and
  soft constraint scan.
- Candidate representation: each validated selector owns a dense byte membership
  table indexed by segment ID.
- Fenrin measurements: 267,230; 272,491; 272,501 (median 272,491).
- Japanese measurements: 79,972; 79,623; 80,853 (median 79,972).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all six reported statistics fields match the baseline exactly.
- Decision: accepted; the primary improvement exceeds 5%, and the secondary
  profile improves rather than regresses.

## Final verification

Pending completion of round 10.
