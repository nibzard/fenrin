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
- After round 3 exposed CPU ramp-up, each later profile series is preceded by one
  unrecorded invocation of the same frozen benchmark command. Only the following
  three runs are used for decisions.
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
| 2 | Render only the selected elite candidate instead of all candidates | 272,491 | 283,881 | +4.18% | 79,972 | 77,455 | -3.15% | identical | reject |
| 3 | Reuse the underlying-unit allocation across fill attempts | 272,491 | 325,193 | +19.34% | 79,972 | 84,071 | +5.13% | identical | keep |

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

### Round 2: defer rendering until elite selection

- Removed work: rendering and `String` allocation for the 15 candidates not
  selected from a full 16-candidate pool.
- Fenrin measurements: 273,703; 282,622; 283,881; 300,412; 293,242
  (five-run median 283,881).
- Japanese measurements: 76,755; 79,483; 74,976; 80,113; 77,455
  (five-run median 77,455).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: rejected. The uncertain-band primary gain passed the five-run +3%
  requirement, but Japanese regressed 3.15%, beyond the allowed 2%.
- Stability note: the five-run spreads were 9.76% for Fenrin and 6.85% for
  Japanese. Read-only analysis processes were finished before later rounds;
  no benchmarks had been run by those processes.

### Round 3: reuse the underlying-unit buffer

- Removed work: allocating and growing a new `Vec<Unit>` for every fill attempt,
  including attempts later rejected by hard constraints.
- Initial Fenrin measurements: 299,443; 320,239; 337,910. Their 12.85% spread
  triggered stabilization rather than an immediate decision.
- Stabilized Fenrin measurements: 325,193; 324,129; 330,882
  (median 325,193; spread 2.08%).
- Initial Japanese measurements: 85,686; 84,607; 84,442.
- Stabilized Japanese measurements: 84,071; 82,965; 85,034
  (median 84,071; spread 2.49%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: accepted. The stabilized primary gain is 19.34%, and Japanese
  improves 5.13%.

## Final verification

Pending completion of round 10.
