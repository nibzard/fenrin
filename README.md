# fenrin

![Fenrin creature against a red landscape](assets/fenrin.png)

Generate pronounceable fictional names and short verification phrases from sound rules—not dictionaries or corpora.

## Quick start

```sh
cargo run -- 10
cargo run -- --config japanese 10
cargo run -- --sas
```

To install the binary from this checkout:

```sh
cargo install --path .
```

Fenrin requires Rust 1.85 or newer.

## Generate names

```sh
fenrin 10
fenrin --config japanese 10
fenrin --config ./my-profile.conf 10
fenrin --seed 42 10
```

Fenrin prints 0–10,000 distinct names, one per line. It uses the `fenrin`
profile by default.

The limit is deliberate: distinct generation retains every name and may need
increasingly many retries as a profile's output space fills. Bounding the count
also bounds memory use and worst-case runtime.

`--seed` (or `-s`) fixes the generator's starting state. Within the same Fenrin
version, the same seed, exact profile contents, and count reproduce the same
names. Seeded output may change between releases. Without a seed, Fenrin starts
from the clock and process ID.

A bare name such as `japanese` or `japanese.conf` first resolves to
`./configs/japanese.conf`; if that file is absent, Fenrin uses its bundled copy.
A path containing a directory, such as `./my-profile.conf`, is loaded exactly
as written.

## Generate comparison phrases

```sh
fenrin --sas
fenrin --sas 0123456789
fenrin --sas-words
```

`--sas` produces four fictional words representing 40 bits. With no value,
those bits come from the operating system's cryptographic random source. Ten
hexadecimal digits produce a deterministic phrase, allowing two applications
with the same bytes to display the same words. `-sas` is accepted as an alias.

`--sas-words` prints all 1,024 codewords in index order, one per line, for
protocol documentation and review. The first line is index 0 and the final line
is index 1023.

> Fenrin only renders a short authentication string. It does not perform the
> key exchange or authenticate either application. Do not use the phrase as a
> password, recovery seed, encryption key, or bearer token.

See [SAS protocol integration](docs/sas-protocol.md) for the stable format,
security bounds, and guidance for deriving and comparing phrases.

## Bundled profiles

Profile labels describe structural inspiration, not authentic names or
vocabulary from those languages.

| Profile | Structural character |
| --- | --- |
| `fenrin` | Nordic-style initial stress, marked vowels, controlled clusters |
| `japanese` | Open morae and contextual consonant rewrites |
| `ancient-roman` | Heavy syllables, stop-liquid onsets, nasal assimilation |
| `slavic` | One complex cluster locus and contextual palatalization |
| `klingon` | Case-sensitive compact CVC structure |
| `oceanic` | Four-to-six morae and licensed diphthongs |
| `uralic` | Front/back harmony and medial geminates |
| `caucasian` | Ejectives and licensed complex onsets |
| `aurelian` | Fictional clear/warm harmony, long vowels, open forms |
| `obsidian` | Fictional uvular/ejective inventory with one complex edge |

## How it works

```text
weighted acyclic grammar
    -> underlying segment sequence
    -> ordered contextual rewrites
    -> hard phonotactic constraints
    -> soft markedness score
    -> orthographic spelling
```

Fenrin samples a weighted top-level shape and returns the first well-formed
filling with a zero soft-constraint score. Each shape has a finite budget of 64
total or 16 well-formed fillings. If none scores zero within that budget, it
randomly chooses among up to four lowest-scoring well-formed fillings seen. It
tries at most eight shapes before reporting that the grammar could not produce
a name.

## Performance

The optimization metric is distinct names completed per second through the
production session: generation, exact deduplication, ordering, formatting, and
buffering. Run a fixed-work measurement with:

```sh
cargo build --release --example benchmark

target/release/examples/benchmark \
  --measure distinct --config fenrin --seed 42 --sessions 50 10000
```

Build a profile-guided artifact across all bundled grammars with:

```sh
scripts/build-pgo.sh <unique-run-name>
```

### Results

The July 2026 architecture campaign used fixed production-distinct work,
randomized ABBA/BAAB blocks, and fresh held-out seeds. Its final PGO artifact
was 7.497x faster than the campaign start on Fenrin (95% one-sided lower bound
7.439x) and 10.388x faster on Japanese (lower bound 10.314x). All 16 planned
blocks were retained for each profile. The campaign's output-preserving changes
matched 40 profile/seed streams byte for byte, while its intentional first-zero
sampling-policy change passed powered quality bounds over 12.8 million draws.

An Apple M5 Pro smoke run on macOS used source `418610f`, Rust 1.90, and LLVM
profile tools 20.1.8. Each raw result measured ten 100,000-name sessions; each
production-distinct result measured fifty 10,000-name sessions. The PGO
artifact matched the normal release output for all ten bundled profiles at
seed 424242 and count 1,000.

| Profile and mode | Normal release | PGO release | Observed change |
| --- | ---: | ---: | ---: |
| Fenrin raw | 7.93M names/s | 8.46M names/s | +6.7% |
| Japanese raw | 4.49M names/s | 4.81M names/s | +7.0% |
| Fenrin distinct | 5.89M names/s | 5.98M names/s | +1.6% |
| Japanese distinct | 3.80M names/s | 4.03M names/s | +5.8% |

These Mac figures are single point-in-time smoke measurements, not randomized
paired evidence. Their percentages describe those runs only and should not be
used as optimization acceptance claims. See the
[optimization method](docs/optimization-loop.md) and [full campaign log](LOG.md)
for the paired runner, A/A calibration, all-profile diagnostics, quality
intervals, and retained and rejected experiments.

## Create a profile

Profiles declare sounds, weighted shapes, rewrites, and constraints. Copy a
file from `configs/` or start with a small grammar:

```conf
segments = P T K S SH N M A I U

feature type consonant = P T K S SH N M
feature type vowel = A I U

spell SH = sh
start = NAME

rule NAME = 3: @CV . @CV . @CVC | 1: @CV . @CV
rule CV = 1: @ONSET @V
rule CVC = 1: @ONSET @V @CODA
rule ONSET = 4: P | 4: T | 3: K | 2: S | 2: N | 2: M
rule CODA = 3: N | 1: K
rule V = 3: A | 2: I | 2: U

rewrite S I -> SH I

hard no-repeat type vowel
hard max-run type consonant 2
soft repeat type consonant 2
```

Run it with `fenrin --config ./my-profile.conf 10`.

## Use as a library

The crate also builds as a library, so applications can embed the generator
instead of shelling out to the binary:

```rust
use fenrin::{config, Rng};

let (_, source) = fenrin::BUNDLED_CONFIGS
    .iter()
    .find(|(name, _)| *name == "japanese.conf")
    .expect("bundled profile");
let grammar = config::parse(source).expect("valid profile");

let mut rng = Rng::new(42);
let name = grammar.generate_name(&mut rng).expect("generated name");
```

`config::load` reads a profile from disk, `fenrin::BUNDLED_CONFIGS` holds the
bundled profile sources, and `sas::encode` and `sas::wordlist` expose the SAS
mapping. Equal seeds produce equal names for the same exact profile source and
Fenrin version.

## Run in a browser

The static demo in `web/` runs the real Rust generator in WebAssembly. The small
`web-wasm/` adapter keeps a parsed grammar and seeded RNG in a Web Worker;
the page requests newline-delimited batches so name generation never competes
with animation on the main thread. Its pace control ranges from 12 names per
second through a 420-per-second maximum-flip mode, plus transition-free 60 Hz
and 120 Hz display ceilings. A rolling readout separates the actual display
rate from the engine's measured raw Wasm throughput.

Build the browser package and serve the directory over HTTP:

```sh
scripts/build-web.sh
python3 -m http.server 4173 --directory web
```

Then open <http://localhost:4173>. The generated `web/pkg/` directory is
ignored by Git and can be deployed with the rest of `web/` to any static host.
Opening `web/index.html` directly with `file://` does not work because browsers
block module workers and Wasm fetches outside an HTTP origin.

The adapter can also be embedded directly in another site:

```js
import init, { NameGenerator } from "./pkg/fenrin_web.js";

await init();
const generator = new NameGenerator("aurelian", 42);
const names = generator.generate_batch(100).split("\n");
```

Browser batches are capped at 4,096 names per call to protect the UI thread.
Run adapter tests with
`cargo test --manifest-path web-wasm/Cargo.toml --locked`.

<details>
<summary>Grammar syntax reference</summary>

- Bare symbols in rules are segments. `@NAME` expands another rule.
- Integer weights precede alternatives. Larger weights make an alternative
  more likely; they are not percentages.
- `.` is a silent prosodic boundary. `_` is an empty production or rewrite.
- `spell` separates the sound representation from its printed form. It defaults
  to the segment identifier.
- Rewrites match exact segments, once per rule in file order, from left to
  right without overlap.
- Constraint patterns accept a segment, `[feature=value]`, `*` for any segment,
  and `.` for an explicit boundary.
- `no-repeat` compares neighboring members of a feature stream, so it can
  reject the same vowel in neighboring syllables.
- Patterns without `.` operate on the rendered surface sequence. Patterns with
  `.` can target a syllable boundary.
- Soft constraints add penalties. Lower scores are preferred, but a soft
  constraint never makes a form illegal.

The parser rejects unknown references, duplicate declarations, recursive or
unreachable rules, impossible feature selectors, empty patterns, zero weights,
and expansions beyond the runtime limits.

</details>

## Limits

Fenrin never consults real vocabulary, so it does not intentionally emit an
existing word or name. It cannot guarantee that a generated form has never
occurred in reality; making that claim would require a dictionary or corpus.

Ordinary name generation is creative output, not cryptographic randomness. Use
SAS mode only as one display component inside a properly designed verification
protocol.

## License

MIT. See [LICENSE](LICENSE).
