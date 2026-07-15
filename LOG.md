# Name-generation optimization log

Date: 2026-07-15  
Branch: `perf/name-generation-10-rounds`  
Primary target: `names`

This run follows `docs/optimization-loop.md`, with one explicit fixed-count override:
the requested ten candidate rounds replace the document's default cap of eight
and its early stop after three consecutive rejections.
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
| 4 | Use cumulative production weights and binary search instead of linear subtraction | 325,193 | 495,813 | +52.47% | 84,071 | 100,015 | +18.96% | identical | keep |
| 5 | Short-circuit max-run validation at the first over-limit unit | 495,813 | 486,634 | -1.85% | 100,015 | 100,946 | +0.93% | identical | reject |
| 6 | Compact generated units to one machine word using a boundary sentinel | 495,813 | 492,432 | -0.68% | 100,015 | 110,604 | +10.59% | identical | reject |
| 7 | Narrow private production-symbol payloads from `usize` to `u8` | 495,813 | 475,875 | -4.02% | 100,015 | 94,794 | -5.22% | identical | reject |
| 8 | Enable ThinLTO for cross-crate release optimization | 495,813 | 492,402 | -0.69% | 100,015 | 106,502 | +6.49% | identical | reject |
| 9 | Compile release artifacts as a single codegen unit | 495,813 | 488,641 | -1.45% | 100,015 | 88,679 | -11.33% | identical | reject |
| 10 | Remove expansion guards already proven unreachable by grammar validation | 495,813 | 496,639 | +0.17% | 100,015 | 105,589 | +5.57% | identical | reject |

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

### Round 4: binary-search cumulative production weights

- Removed work: linearly subtracting weights across as many as 14 production
  alternatives for every nested grammar expansion.
- Candidate representation: productions store cumulative exclusive upper bounds;
  `partition_point` maps the same random ticket to the same production.
- Fenrin measurements: 493,108; 495,813; 498,505
  (median 495,813; spread 1.09%).
- Japanese measurements: 104,082; 100,015; 99,244
  (median 100,015; spread 4.87%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: accepted. Fenrin improves 52.47%, and Japanese improves 18.96%.

### Round 5: short-circuit max-run constraints

- Proposed work removal: stop scanning a candidate at the first over-limit run
  and avoid maintaining a running maximum.
- Fenrin measurements: 486,634; 487,516; 478,228
  (median 486,634; spread 1.94%).
- Japanese measurements: 102,869; 100,946; 99,949
  (median 100,946; spread 2.92%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: rejected. Fenrin regresses 1.85%, and the Japanese gain is only
  0.93%; neither clears the primary 2% floor.

### Round 6: compact the generated-unit representation

- Proposed work removal: halve `Unit` storage from 16 bytes to one machine word
  by reserving `usize::MAX` for boundaries; segment IDs are capped at 256.
- Fenrin measurements: 490,844; 492,432; 511,024
  (median 492,432; spread 4.11%).
- Japanese measurements: 108,965; 111,374; 110,604
  (median 110,604; spread 2.21%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: rejected. Fenrin regresses 0.68% and fails the primary 2% floor,
  despite a 10.59% Japanese improvement. This remains a candidate for a future
  Japanese-primary loop.

### Round 7: narrow compiled production-symbol payloads

- Proposed work removal: reduce the private `Symbol` enum's payload from
  `usize` to `u8`; configured segment and rule limits fit in one byte.
- Fenrin measurements: 476,077; 475,875; 471,419
  (median 475,875; spread 0.99%).
- Japanese measurements: 94,672; 94,994; 94,794
  (median 94,794; spread 0.34%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: rejected. Fenrin regresses 4.02%, and Japanese regresses 5.22%.
- Process note: this is the third consecutive rejection. The requested fixed
  ten-round goal overrides the normal early-stop condition, so rounds 8–10
  continue while all measurement and correctness decision rules remain active.

### Round 8: enable ThinLTO

- Proposed work removal: allow release optimization and inlining across the
  library/example crate boundary and across codegen units.
- Fenrin measurements: 495,706; 485,073; 492,402
  (median 492,402; spread 2.19%).
- Japanese measurements: 106,502; 103,527; 110,553
  (median 106,502; spread 6.79%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: rejected. Fenrin regresses 0.69% and fails the primary 2% floor,
  despite a 6.49% Japanese improvement. The Japanese series also exceeds the
  5% spread limit, reinforcing the rejection rather than requiring an uncertain
  primary-band remeasurement.

### Round 9: use one release codegen unit

- Proposed work removal: expose the complete grammar crate to one optimization
  unit without enabling link-time optimization.
- Fenrin measurements: 488,641; 493,597; 469,785
  (median 488,641; spread 5.07%).
- Japanese measurements: 92,467; 88,679; 88,481
  (median 88,679; spread 4.50%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: rejected. Fenrin regresses 1.45%, and Japanese regresses 11.33%.
  Fenrin's spread is also marginally above the stability limit, with no sample
  showing the 2% gain needed to make further measurement relevant.

### Round 10: remove redundant expansion guards

- Proposed work removal: rely on the parser's acyclic-graph and 64-unit proofs
  instead of checking recursion depth, output length, and propagated success at
  every expansion step. Rewrite growth checks remain separate and unchanged.
- Fenrin measurements: 496,639; 488,300; 514,523
  (median 496,639; spread 5.37%).
- Japanese measurements: 106,818; 99,114; 105,589
  (median 105,589; spread 7.77%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all reported statistics match the baseline exactly.
- Decision: rejected. Fenrin improves only 0.17%, below the 2% floor, despite
  a 5.57% Japanese gain. Both series also exceed the 5% spread limit, but no
  plausible primary median crosses the acceptance threshold.

## Final verification

### Outcome

| Profile | Starting median | Ending accepted median | Cumulative change | Speedup |
| --- | ---: | ---: | ---: | ---: |
| fenrin | 94,987 | 495,813 | +421.98% | 5.22x |
| japanese | 47,757 | 100,015 | +109.42% | 2.09x |

Accepted hypotheses:

1. Precompute dense feature-selector membership tables (round 1).
2. Reuse the underlying-unit buffer across fill attempts (round 3).
3. Store cumulative production weights and binary-search them (round 4).

Rejected hypotheses:

1. Defer rendering until elite selection because Japanese regressed beyond 2%.
2. Short-circuit max-run validation because the primary profile regressed.
3. Compact `Unit` because the primary profile stayed flat.
4. Narrow `Symbol` payloads because both profiles regressed.
5. Enable ThinLTO because the primary profile stayed flat.
6. Use one release codegen unit because both profiles regressed.
7. Remove validated expansion guards because the primary gain stayed below 2%.

### Correctness and compatibility

- `cargo fmt -- --check`: pass.
- `cargo test --all-targets`: pass (53 tests).
- `cargo clippy --all-targets -- -D warnings`: pass.
- Final Fenrin and Japanese 1,000-name snapshots compare byte-for-byte with the
  pre-loop snapshots and retain the same SHA-256 hashes.
- Every iteration preserved the benchmark's duplicate percentage, pair matches,
  collision bits, effective diversity, and maximum frequency exactly.
- The SAS exhaustive tests pass; no SAS implementation was changed.

### Required large benchmarks

Default profile (`cargo run --release --example benchmark`):

| Names | names/second | ns/name | unique | duplicate % | collision bits | effective diversity | max freq |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 491,268 | 2,036 | 995 | 0.500% | 16.61 | 9.990e4 | 2 |
| 10,000 | 502,040 | 1,992 | 9,714 | 2.860% | 17.34 | 1.655e5 | 4 |
| 1,000,000 | 506,945 | 1,973 | 412,401 | 58.760% | 17.29 | 1.599e5 | 83 |

Japanese profile at 1,000,000 names:

- 108,176 names/second; 9,244 ns/name; 499,551 unique; 50.045%
  duplicates; 2,265,668 matching pairs; 17.75 collision bits; 2.207e5
  effective diversity; maximum frequency 54.

SAS final benchmark:

| Phrases | phrases/second | ns/phrase |
| ---: | ---: | ---: |
| 1,000 | 40,708,910 | 25 |
| 10,000 | 40,629,956 | 25 |
| 1,000,000 | 41,277,965 | 24 |

### Bundled-profile smoke test

Every required 100,000-name release benchmark completed successfully:

| Profile | names/second |
| --- | ---: |
| fenrin | 508,979 |
| japanese | 101,396 |
| ancient-roman | 140,684 |
| slavic | 138,024 |
| klingon | 247,339 |
| oceanic | 186,507 |
| uralic | 328,945 |
| caucasian | 224,189 |
| aurelian | 147,712 |
| obsidian | 183,493 |

### Remaining hotspot

Japanese still rebuilds a temporary unit vector for each of seven rewrite rules
on every candidate. A future Japanese-primary loop should test an in-place fast
path for equal-length rewrites or a reusable rewrite scratch buffer. Round 6's
10.59% Japanese-only gain and round 8's 6.49% Japanese-only gain show that this
profile still has optimization headroom, but neither change met this loop's
Fenrin-primary acceptance rule.

---

# Japanese-primary 20-round optimization log

Date: 2026-07-15

Branch: `perf/japanese-generation-20-rounds`

Primary profile: `japanese`

Secondary regression guard: `fenrin`

This second name-generation loop uses the same correctness and measurement
procedure from `docs/optimization-loop.md`. The requested fixed count of twenty
candidates overrides the document's eight-candidate cap and three-rejection
early stop. All candidates remain small and reversible; the 2%/3%/5% decision
thresholds, 2% secondary-regression limit, seeded output checks, and quality
invariants remain enforced.

## Frozen procedure

- Primary command: `cargo run --release --example benchmark -- --config japanese 100000`
- Secondary command: `cargo run --release --example benchmark -- 100000`
- Each series receives one unrecorded invocation of the same command to stabilize
  CPU state, followed by three recorded measurements.
- Primary changes of 2–5% are extended to five recorded measurements and must
  retain at least a 3% median gain.
- The benchmark source, commands, profiles, and generation constants are frozen.

## Starting verification and baseline

- `cargo fmt -- --check`: pass.
- `cargo test --all-targets`: pass (53 tests).
- `cargo clippy --all-targets -- -D warnings`: pass.
- Both 1,000-name seeded snapshots retain the hashes recorded by the first loop.

| Iteration | Hypothesis | Japanese before | Japanese after | Primary change | Fenrin before | Fenrin after | Secondary change | Quality | Decision |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| 0 | Baseline from the accepted first-loop implementation | — | 107,239 | — | — | 497,796 | — | baseline | keep |
| 1 | Apply equal-length rewrites in place instead of rebuilding the unit vector | 107,239 | 146,685 | +36.78% | 497,796 | 488,431 | -1.88% | identical | keep |

Baseline raw measurements and spread:

- Japanese: 106,757; 107,239; 107,676 (median 107,239; spread 0.86%).
- Fenrin: 485,787; 497,796; 507,526 (median 497,796; spread 4.47%).

Baseline quality statistics:

| Profile | duplicate % | pair matches | collision bits | effective diversity | max freq |
| --- | ---: | ---: | ---: | ---: | ---: |
| japanese | 16.447% | 22,833 | 17.74 | 2.190e5 | 9 |
| fenrin | 20.989% | 31,211 | 17.29 | 1.602e5 | 10 |

## Japanese-primary iterations

### Round 1: apply equal-length rewrites in place

- Removed work: seven temporary vector allocations and seven full vector rebuilds
  per Japanese candidate; all Japanese rewrites are two units to two units.
- Japanese measurements: 150,545; 145,258; 146,685
  (median 146,685; spread 3.64%).
- Fenrin measurements: 488,431; 485,535; 496,183
  (median 488,431; spread 2.19%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. Japanese improves 36.78%; Fenrin regresses 1.88%, within
  the 2% secondary limit.
