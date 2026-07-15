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
| 2 | Compile literal rewrite patterns as `Unit` sequences instead of generic matchers | 146,685 | 165,287 | +12.68% | 488,431 | 498,803 | +2.12% | identical | keep |
| 3 | Specialize two-unit rewrites as direct comparisons and assignments | 165,287 | 173,296 | +4.85% | 498,803 | 479,821 | -3.81% | identical | reject |
| 4 | Fuse statically independent ordered pair rewrites into one adjacency scan | 165,287 | 242,597 | +46.77% | 498,803 | 494,462 | -0.87% | identical | keep |
| 5 | Compact generated units to one machine word using a boundary sentinel | 242,597 | 252,203 | +3.96% | 494,462 | 497,084 | +0.53% | identical | keep |
| 6 | Precompute dense ticket-to-production lookup tables for small weighted rules | 252,203 | 302,755 | +20.04% | 497,084 | 575,520 | +15.78% | identical | keep |
| 7 | Push precompiled units directly for terminal-choice rules | 302,755 | 348,057 | +14.96% | 575,520 | 682,112 | +18.52% | identical | keep |
| 8 | Reserve the statically validated maximum expansion capacity | 348,057 | 314,671 | -9.59% | 682,112 | 620,077 | -9.09% | identical | reject |
| 9 | Retain the stable top-four candidates while fills are generated | 348,057 | 391,908 | +12.60% | 682,112 | 890,363 | +30.53% | identical | keep |
| 10 | Stop monotonic soft scoring once a full elite pool cannot be improved | 391,908 | 410,200 | +4.67% | 890,363 | 968,668 | +8.79% | identical | keep |
| 11 | Render in one pass using unit count as the initial string capacity | 410,200 | 391,638 | -4.53% | 968,668 | 918,765 | -5.15% | identical | reject |
| 12 | Fuse a complete `no-repeat` and `max-run` hard-constraint pair | 410,200 | 413,935 | +0.91% | 968,668 | 1,001,187 | +3.36% | identical | reject |
| 13 | Remove expansion guards made unreachable by validated static bounds | 410,200 | 448,118 | +9.24% | 968,668 | 1,000,200 | +3.25% | identical | keep |
| 14 | Enable ThinLTO for cross-crate release optimization | 448,118 | 449,816 | +0.38% | 1,000,200 | 1,039,748 | +3.95% | identical | reject |
| 15 | Store immutable production symbol lists as boxed slices | 448,118 | 444,522 | -0.80% | 1,000,200 | 1,021,286 | +2.11% | identical | reject |
| 16 | Keep the four elite candidate records in a fixed stack array | 448,118 | 443,295 | -1.08% | 1,000,200 | 999,197 | -0.10% | identical | reject |
| 17 | Narrow generated units from machine words to `u16` indices | 448,118 | 455,591 | +1.67% | 1,000,200 | 995,669 | -0.45% | identical | reject |
| 18 | Drop parse-only feature maps and retain a compact spelling vector | 448,118 | 450,675 | +0.57% | 1,000,200 | 1,057,710 | +5.75% | identical | reject |
| 19 | Encode empty pair-rewrite cells with a private unit sentinel | 448,118 | 454,597 | +1.45% | 1,000,200 | 1,024,671 | +2.45% | identical | reject |
| 20 | Encode pair-rewrite replacements as compact `u16` keys | 448,118 | 455,072 | +1.55% | 1,000,200 | 1,021,440 | +2.12% | identical | reject |

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

### Round 2: compile rewrites as direct unit sequences

- Removed work: generic matcher dispatch and grammar-dependent matching for
  rewrite syntax that permits only literal segments and boundaries.
- Initial Japanese measurements: 163,699; 166,530; 156,795. The 6.21% spread
  triggered a replacement stabilized series.
- Stabilized Japanese measurements: 160,900; 165,287; 166,574
  (median 165,287; spread 3.53%).
- Initial Fenrin measurements: 494,546; 475,429; 480,642.
- Stabilized Fenrin measurements: 495,564; 498,908; 498,803
  (median 498,803; spread 0.67%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. Japanese improves 12.68%, and Fenrin improves 2.12%.

### Round 3: specialize two-unit rewrites

- Proposed work removal: replace generic slice-prefix comparison and slice copy
  with two direct comparisons and two direct assignments for two-unit rules.
- Japanese measurements: 173,147; 174,161; 171,887; 176,558; 173,296
  (five-run median 173,296; spread 2.72%).
- Fenrin measurements: 493,020; 475,560; 497,645; 479,821; 474,960
  (five-run median 479,821; spread 4.78%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. The uncertain-band Japanese gain retained 4.85%, but
  Fenrin regressed 3.81%, beyond the 2% secondary limit.

### Round 4: fuse independent pair rewrites

- Removed work: six of seven complete Japanese rewrite scans. Parse-time
  eligibility proves every rule is two-to-two, preserves its context unit, and
  keeps all contexts disjoint from source and replacement first units.
- The dense lookup composes same-position cascades in declaration order; configs
  that fail the conservative proof use the existing ordered fallback.
- Initial Japanese measurements: 240,152; 250,861; 256,469. The 6.79% spread
  triggered a replacement stabilized series.
- Stabilized Japanese measurements: 242,597; 252,861; 241,844
  (median 242,597; spread 4.56%).
- Initial Fenrin measurements: 489,658; 494,619; 502,998.
- Stabilized Fenrin measurements: 497,441; 494,462; 487,886
  (median 494,462; spread 1.96%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. Japanese improves 46.77%; Fenrin regresses 0.87%, within
  the secondary limit.

### Round 5: compact generated units

- Removed work: halve the hot `Unit` representation from a 16-byte tagged enum
  to one machine word, reserving `usize::MAX` as the private boundary sentinel.
- Japanese measurements: 257,035; 246,808; 252,203; 252,306; 250,255
  (five-run median 252,203; spread 4.14%).
- Initial Fenrin measurements: 508,603; 482,518; 496,688; 495,910; 502,004.
  Their 5.41% spread triggered a replacement stabilized series.
- Stabilized Fenrin measurements: 497,084; 486,276; 500,601
  (median 497,084; spread 2.95%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. The uncertain-band Japanese gain retained 3.96%, and
  stabilized Fenrin improves 0.53%.

### Round 6: precompute weighted-production ticket tables

- Removed work: binary-searching cumulative weights for every grammar expansion.
  Rules with total weight at most 256 map each random ticket directly to a byte
  production index; larger custom rules retain `partition_point`.
- Japanese measurements: 306,958; 302,755; 294,150
  (median 302,755; spread 4.35%).
- Fenrin measurements: 575,520; 575,725; 562,199
  (median 575,520; spread 2.41%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. Japanese improves 20.04%, and Fenrin improves 15.78%.

### Round 7: specialize terminal-choice expansion

- Removed work: entering `expand_production` and iterating a symbol vector after
  selecting rules whose every production emits exactly one literal unit.
- Japanese measurements: 343,851; 351,582; 348,057
  (median 348,057; spread 2.25%).
- Fenrin measurements: 685,921; 682,112; 665,602
  (median 682,112; spread 3.05%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. Japanese improves 14.96%, and Fenrin improves 18.52%.

### Round 8: reserve maximum unit capacity

- Proposed work removal: avoid geometric growth by retaining the parser's exact
  maximum start-rule expansion and reserving that capacity once per name.
- Japanese measurements: 322,595; 314,671; 313,445
  (median 314,671; spread 2.92%).
- Fenrin measurements: 620,077; 635,283; 600,832
  (median 620,077; spread 5.73%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Japanese regresses 9.59%, and Fenrin regresses 9.09%; the
  larger retained allocation costs more than the avoided growth.

### Round 9: retain the stable top-four candidates

- Removed work: rendering and retaining all sixteen accepted candidates, then
  stable-sorting them. The generator now maintains only the best four in stable
  `(score, acceptance order)` order while still consuming all sixteen fills.
- Initial Japanese measurements: 374,280; 390,599; 402,145. Their 7.44% spread
  triggered a replacement series.
- Second Japanese measurements: 398,570; 371,089; 404,697. This series remained
  unstable, so the environment was warmed again.
- Stabilized Japanese measurements: 396,851; 391,908; 389,557
  (median 391,908; spread 1.87%).
- The matching Fenrin guard series also varied during stabilization. Its final
  replacement measurements were 887,317; 890,363; 913,475
  (median 890,363; spread 2.95%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. Japanese improves 12.60%, and Fenrin improves 30.53%.

### Round 10: stop scoring candidates that cannot enter the elite

- Removed work: evaluating later nonnegative soft penalties after the running
  saturated score reaches the full elite pool's worst score. A zero worst score
  skips all soft-constraint scans for later candidates.
- Japanese measurements: 403,230; 410,200; 410,207; 400,680; 412,602
  (five-run median 410,200; spread 2.98%).
- Fenrin measurements: 968,050; 993,611; 967,706; 968,668; 979,932
  (five-run median 968,668; spread 2.68%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. The uncertain-band Japanese gain retains 4.67%, and
  Fenrin improves 8.79%.

### Round 11: render with an approximate capacity

- Proposed work removal: eliminate the exact rendered-byte-length scan and use
  the unit count as the initial `String` capacity before the rendering pass.
- Initial Japanese measurements: 367,397; 392,502; 369,167. Their 6.83% spread
  triggered a replacement stabilized series.
- Stabilized Japanese measurements: 391,638; 399,058; 389,251
  (median 391,638; spread 2.52%).
- Fenrin measurements: 918,765; 924,332; 916,209
  (median 918,765; spread 0.89%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Japanese regresses 4.53%, and Fenrin regresses 5.15%;
  string growth costs more than the removed sizing scan. Exact sizing restored.

### Round 12: fuse the Japanese hard-constraint pair

- Proposed work removal: evaluate a grammar whose complete hard-constraint list
  is `no-repeat` followed by `max-run` in one unit scan instead of two.
- Japanese measurements: 413,935; 411,971; 419,833
  (median 413,935; spread 1.91%).
- Fenrin measurements: 996,758; 1,023,181; 1,001,187
  (median 1,001,187; spread 2.65%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Japanese improves only 0.91%, below the 2% threshold;
  the specialized path was removed.

### Round 13: remove validated expansion guards

- Removed work: checking recursion depth, expanded length, and propagated
  success during every nested rule expansion after parsing has already proved
  an acyclic rule graph and a maximum start expansion of 64 units.
- Japanese measurements: 454,005; 443,743; 448,118
  (median 448,118; spread 2.31%).
- Fenrin measurements: 976,194; 1,011,447; 1,000,200
  (median 1,000,200; spread 3.61%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: accepted. Japanese improves 9.24%, and Fenrin improves 3.25%.

### Round 14: enable ThinLTO

- Proposed work removal: expose the library and benchmark example to cross-crate
  inlining and optimization during release linking.
- Japanese measurements: 465,037; 449,816; 447,353
  (median 449,816; spread 3.95%).
- Fenrin measurements: 1,039,748; 1,032,932; 1,042,380
  (median 1,039,748; spread 0.91%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Japanese improves only 0.38%, below the 2% threshold;
  the release-profile override was removed.

### Round 15: box immutable production symbols

- Proposed work removal: drop the unused capacity word from every immutable
  production symbol list to reduce compiled grammar metadata.
- Japanese measurements: 444,522; 450,867; 439,298
  (median 444,522; spread 2.63%).
- Fenrin measurements: 1,035,938; 1,021,286; 1,013,134
  (median 1,021,286; spread 2.25%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Japanese regresses 0.80%, so the original `Vec` storage
  was restored.

### Round 16: use a fixed elite-candidate array

- Proposed work removal: eliminate the four-record candidate `Vec` allocation
  by maintaining the stable elite prefix in a fixed stack array.
- Initial Japanese measurements: 445,406; 441,155; 412,634. Their 7.94% spread
  triggered stabilization; the second replacement also exceeded 5%.
- Stabilized Japanese measurements: 440,593; 443,295; 447,048
  (median 443,295; spread 1.47%).
- Fenrin measurements: 992,501; 1,009,971; 999,197
  (median 999,197; spread 1.76%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Stabilized Japanese regresses 1.08%, and Fenrin is flat;
  the heap-backed elite vector was restored.

### Round 17: narrow generated units to `u16`

- Proposed work removal: reduce each unit from eight bytes to two while retaining
  `u16::MAX` as the boundary sentinel; the parser permits at most 256 segments.
- Japanese measurements: 452,801; 470,594; 459,673; 453,079; 455,591
  (five-run median 455,591; spread 3.93%).
- Fenrin measurements: 999,404; 989,891; 991,404; 995,669; 1,007,578
  (five-run median 995,669; spread 1.79%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. The uncertain Japanese signal settles at 1.67%, below the
  3% retention requirement, while Fenrin regresses 0.45%.

### Round 18: retain only runtime spellings

- Proposed work removal: discard per-segment feature hash maps after selectors
  compile and render from a compact contiguous spelling vector.
- Japanese measurements: 443,368; 457,422; 450,675
  (median 450,675; spread 3.17%).
- Initial Fenrin measurements exceeded the stability limit; stabilized
  measurements: 1,063,936; 1,045,697; 1,057,710
  (median 1,057,710; spread 1.74%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Japanese improves only 0.57%, below the primary floor,
  despite a 5.75% Fenrin gain; the original representation was restored.

### Round 19: halve pair-rewrite table entries

- Proposed work removal: replace two-word `Option<Unit>` cells with a one-word
  private sentinel representation in the dense fused pair-rewrite table.
- Japanese measurements: 452,394; 455,351; 454,597
  (median 454,597; spread 0.65%).
- Fenrin measurements: 1,041,470; 1,024,671; 1,013,127
  (median 1,024,671; spread 2.80%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Japanese improves 1.45%, below the 2% floor; the sentinel
  representation was removed.

### Round 20: encode pair-rewrite cells as `u16` keys

- Proposed work removal: shrink every fused pair-rewrite table cell from a
  two-word optional unit to a two-byte key, reserving zero for no replacement.
- Japanese measurements: 455,681; 455,072; 455,066
  (median 455,072; spread 0.14%).
- Fenrin measurements: 1,044,401; 1,021,440; 1,013,352
  (median 1,021,440; spread 3.06%).
- Gates: format pass; 53 tests pass; clippy pass; both seeded snapshots identical.
- Quality: all benchmark statistics match the baseline exactly.
- Decision: rejected. Japanese improves 1.55%, below the 2% floor; the compact
  key table was removed.

## Final verification

### Outcome

| Profile | Starting median | Ending accepted median | Cumulative change | Speedup |
| --- | ---: | ---: | ---: | ---: |
| japanese | 107,239 | 448,118 | +317.87% | 4.18x |
| fenrin | 497,796 | 1,000,200 | +100.93% | 2.01x |

Nine hypotheses were accepted:

1. Apply equal-length rewrites in place (round 1).
2. Compile literal rewrite patterns as units (round 2).
3. Fuse independent ordered pair rewrites (round 4).
4. Compact generated units to one machine word (round 5).
5. Precompute weighted-production ticket tables (round 6).
6. Specialize terminal-choice rule expansion (round 7).
7. Retain only the stable four-candidate elite (round 9).
8. Stop soft scoring at the full elite pool's cutoff (round 10).
9. Trust parser-validated expansion bounds (round 13).

Eleven hypotheses were rejected and fully reverted:

1. Directly specialize two-unit fallback rewrites (round 3).
2. Reserve the maximum validated expansion capacity (round 8).
3. Render with approximate instead of exact capacity (round 11).
4. Fuse the Japanese hard-constraint pair (round 12).
5. Enable ThinLTO (round 14).
6. Box immutable production symbol lists (round 15).
7. Put the four elite records in a fixed stack array (round 16).
8. Narrow generated units to `u16` (round 17).
9. Retain only spellings after selector compilation (round 18).
10. Use a private empty-cell sentinel in the pair-rewrite table (round 19).
11. Encode pair-rewrite table cells as `u16` keys (round 20).

### Correctness and compatibility

- `cargo fmt -- --check`: pass.
- `cargo test --all-targets`: pass (53 tests).
- `cargo clippy --all-targets -- -D warnings`: pass.
- Final Fenrin and Japanese 1,000-name snapshots compare byte-for-byte with the
  frozen pre-loop files.
- Fenrin snapshot SHA-256:
  `6da69b54e4638bd55021a2f78405afc0ae3b55b09ddd61135b5358710566a17a`.
- Japanese snapshot SHA-256:
  `133e62c7e1b9d2903fd9dcb9def6d0d9dabe8504400b82c0e96200971ce4d3b9`.
- Every iteration preserved duplicate percentage, pair matches, collision bits,
  effective diversity, and maximum frequency exactly.
- `examples/benchmark.rs`, bundled profile files, generation constants, and the
  public API were unchanged.

### Required large benchmarks

Default profile (`cargo run --release --example benchmark`):

| Names | names/second | ns/name | unique | duplicate % | collision bits | effective diversity | max freq |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 932,787 | 1,072 | 995 | 0.500% | 16.61 | 9.990e4 | 2 |
| 10,000 | 1,007,317 | 993 | 9,714 | 2.860% | 17.34 | 1.655e5 | 4 |
| 1,000,000 | 1,037,625 | 964 | 412,401 | 58.760% | 17.29 | 1.599e5 | 83 |

Japanese profile at 1,000,000 names:

- 454,527 names/second; 2,200 ns/name; 499,551 unique; 50.045%
  duplicates; 2,265,668 matching pairs; 17.75 collision bits; 2.207e5
  effective diversity; maximum frequency 54.

SAS final benchmark:

| Phrases | phrases/second | ns/phrase |
| ---: | ---: | ---: |
| 1,000 | 41,311,530 | 24 |
| 10,000 | 41,097,213 | 24 |
| 1,000,000 | 41,861,352 | 24 |

### Bundled-profile smoke test

Every required 100,000-name release benchmark completed successfully:

| Profile | names/second |
| --- | ---: |
| fenrin | 1,053,065 |
| japanese | 435,277 |
| ancient-roman | 240,364 |
| slavic | 381,137 |
| klingon | 493,199 |
| oceanic | 309,261 |
| uralic | 599,024 |
| caucasian | 352,219 |
| aurelian | 277,199 |
| obsidian | 251,065 |

### Remaining opportunities

The largest remaining measured opportunity is profile-dependent rather than a
clear Japanese-primary win. Dropping parse-only feature maps improved Fenrin by
5.75% in round 18 but moved Japanese only 0.57%; it belongs in a separate
Fenrin-primary loop. Compact pair-rewrite cells (rounds 19 and 20) and `u16`
units (round 17) each produced stable Japanese gains of 1.45–1.67%, below the
retention threshold. A future loop could profile a sparse context-first rewrite
dispatch that removes the dense table entirely, or test a combined constraint
evaluator, but the isolated hard-constraint fusion in round 12 suggests that
constraint scans now offer less than 1% on Japanese.
