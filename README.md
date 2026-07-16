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

<details>
<summary>SAS protocol integration</summary>

The stable `fenrin-sas-v1` format reads five bytes in big-endian order, splits
them into four 10-bit values, and maps each value to one algorithmic CVCVC word.
The final consonant is parity for easier comparison; it adds no entropy. The
name profiles and configuration files never affect this mapping.

Any two codewords differ in at least two of their five letters: whenever a
single core symbol changes, the parity coda changes with it. A single misread
letter therefore never turns one valid codeword into another. The test suite
proves this bound over all codeword pairs.

Paired applications should derive the five uniform bytes with a
protocol-specific KDF over their shared key-exchange secret and a canonical
transcript. That transcript should bind identities, roles, session ID, protocol
version, and ephemeral public keys. Compare all four words in order.

The active-forgery bound is approximately `q / 2^40` for `q` allowed attempts,
so the surrounding protocol must commit before revealing the phrase and limit
retries. A phrase entered on another device needs a one-shot, rate-limited PAKE
rather than direct use as key material.

</details>

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
randomly chooses among the four lowest-scoring well-formed fillings seen. It
tries at most eight shapes before reporting that the grammar could not produce
a name.

## Benchmark

Run the core benchmark in release mode; it adds no benchmark-specific
dependencies:

```sh
cargo run --release --example benchmark
cargo run --release --example benchmark -- 1000 10000 1000000
cargo run --release --example benchmark -- --config japanese 1000000
cargo run --release --example benchmark -- --sas 1000000
```

### Reference run

> **Million-name experiment:** With seed 42, the default profile produced
> 412,401 distinct observed forms in 1,000,000 draws. The most frequent form
> appeared only 83 times, or 0.0083% of the sample.

| Workload | Throughput | Time per output | Observed diversity |
| --- | ---: | ---: | --- |
| Default names, 1,000,000 draws | ~95,000 names/s | ~10.5 µs | 17.29 collision bits; ~159,900 effective diversity |
| SAS encoder, 1,000,000 inputs | ~27 million phrases/s | ~37 ns | Not applicable: the 40-bit mapping is injective |

Measured with Fenrin 0.2.1 in release mode on a 15-core Apple M5 Pro using
Rust 1.90. These are rounded, point-in-time core results; config parsing and
terminal output are excluded. Effective diversity measures output
concentration, not the total number of names a profile can produce.

With no counts, it measures batches of 1,000, 10,000, and 1,000,000 outputs.
After warming up, short batches repeat until their combined timing reaches
roughly 200 ms; long batches run once. Timing is capped at ten million outputs,
unless one requested batch is already larger. The `runs` column makes this
explicit. The timed pass discards each result and excludes config parsing,
uniqueness statistics, and terminal output.

Name mode reports the duplicate percentage, matching output pairs, most
frequent output, collision bits, and effective diversity. Collision bits are
`-log2(p)`, where `p` is the observed probability that two sampled outputs
match; effective diversity is `1/p`. These frequency-aware measures expose a
profile that concentrates too much probability on a small set of names. They
display `n/a` when the sample contains no collision. Statistics use at most one
million 64-bit fingerprints, bounding memory to roughly 8 MB; consequently,
accidental fingerprint collisions are extremely unlikely, but the results are
estimates rather than exact string comparisons.

`--sas` measures phrase encoding over distinct sequential 40-bit inputs, so it
does not benchmark the operating system's random source. SAS mode reports only
performance: injectivity and codeword-distance properties are deterministic
invariants covered exhaustively by the test suite, not statistical benchmark
results.

Adaptive timing stabilizes short runs but remains a point-in-time measurement.
Repeat the command when comparing code changes so ordinary system noise is easy
to spot.

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
