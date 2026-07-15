// ABOUTME: Throughput benchmark for Fenrin with no benchmark-only dependencies.
// ABOUTME: It times generation alone, then measures name concentration separately.

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use fenrin::{BUNDLED_CONFIGS, Grammar, Rng, config, sas};

const USAGE: &str = "Usage: cargo run --release --example benchmark -- [--config <name-or-path>] [count ...]\n       cargo run --release --example benchmark -- -sas|--sas [count ...]";
const DEFAULT_CONFIG: &str = "fenrin";
const DEFAULT_COUNTS: [u64; 3] = [1_000, 10_000, 1_000_000];
const WARMUP_ITEMS: u64 = 1_000;
const MAX_STAT_ITEMS: u64 = 1_000_000;
const MIN_TIMING_TIME: Duration = Duration::from_millis(200);
const MAX_TIMED_ITEMS: u64 = 10_000_000;
const SEED: u64 = 42;
const SAS_MASK: u64 = (1_u64 << sas::SAS_BITS) - 1;

#[derive(Debug, PartialEq)]
enum Target {
    Names(String),
    Sas,
}

#[derive(Debug, PartialEq)]
struct Arguments {
    target: Target,
    counts: Vec<u64>,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let mut config = None;
    let mut sas = false;
    let mut counts = Vec::new();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "-sas" | "--sas" => {
                if sas {
                    return Err("`--sas` specified more than once".to_owned());
                }
                sas = true;
            }
            "-c" | "--config" => {
                if config.is_some() {
                    return Err("config specified more than once".to_owned());
                }
                config = Some(
                    args.next()
                        .ok_or_else(|| "missing value after `--config`".to_owned())?,
                );
            }
            _ if argument.starts_with('-') => {
                return Err(format!("unknown option `{argument}`"));
            }
            _ => {
                let count = argument
                    .parse::<u64>()
                    .map_err(|_| format!("invalid count `{argument}`"))?;
                if count == 0 {
                    return Err("counts must be greater than zero".to_owned());
                }
                counts.push(count);
            }
        }
    }

    if counts.is_empty() {
        counts.extend(DEFAULT_COUNTS);
    }

    let target = match (sas, config) {
        (true, Some(_)) => return Err("`--sas` cannot be combined with `--config`".to_owned()),
        (true, None) => Target::Sas,
        (false, config) => Target::Names(config.unwrap_or_else(|| DEFAULT_CONFIG.to_owned())),
    };

    Ok(Some(Arguments { target, counts }))
}

fn is_bare_path(path: &Path) -> bool {
    path.file_name().is_some()
        && path
            .parent()
            .is_none_or(|parent| parent.as_os_str().is_empty())
}

fn load_grammar(requested: &str) -> Result<Grammar, String> {
    let requested = Path::new(requested);
    if !is_bare_path(requested) {
        return config::load(requested);
    }

    let mut filename = PathBuf::from(requested);
    if filename.extension().is_none() {
        filename.set_extension("conf");
    }

    let local = Path::new("configs").join(&filename);
    if local.exists() {
        return config::load(&local);
    }

    let filename = filename.file_name().and_then(|name| name.to_str());
    if let Some((name, source)) = BUNDLED_CONFIGS
        .iter()
        .find(|(name, _)| Some(*name) == filename)
    {
        return config::parse(source)
            .map_err(|error| format!("built-in profile `{name}`: {error}"));
    }

    config::load(&local)
}

struct SasInputs(u64);

impl SasInputs {
    fn new() -> Self {
        Self(SEED & SAS_MASK)
    }

    fn next(&mut self) -> [u8; sas::SAS_BYTES] {
        let bytes = self.0.to_be_bytes();
        self.0 = self.0.wrapping_add(1) & SAS_MASK;
        [bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]
    }
}

struct Timing {
    runs: u64,
    items_per_second: f64,
    nanoseconds_per_item: f64,
}

#[derive(Debug, PartialEq)]
struct UniquenessStats {
    sampled: u64,
    unique: u64,
    collision_pairs: u128,
    max_frequency: u64,
}

impl UniquenessStats {
    fn duplicate_count(&self) -> u64 {
        self.sampled - self.unique
    }

    fn duplicate_percentage(&self) -> f64 {
        self.duplicate_count() as f64 * 100.0 / self.sampled as f64
    }

    fn collision_probability(&self) -> Option<f64> {
        if self.sampled < 2 || self.collision_pairs == 0 {
            return None;
        }

        let possible_pairs = self.sampled as f64 * (self.sampled - 1) as f64 / 2.0;
        Some(self.collision_pairs as f64 / possible_pairs)
    }

    fn collision_bits(&self) -> Option<f64> {
        self.collision_probability()
            .map(|probability| -probability.log2())
    }

    fn effective_diversity(&self) -> Option<f64> {
        self.collision_probability()
            .map(|probability| probability.recip())
    }
}

fn generate(
    count: u64,
    mut generate_one: impl FnMut() -> Result<String, &'static str>,
) -> Result<(), &'static str> {
    for _ in 0..count {
        let output = generate_one()?;
        drop(black_box(output));
    }
    Ok(())
}

fn uniqueness_stats(
    count: u64,
    mut generate_one: impl FnMut() -> Result<String, &'static str>,
) -> Result<UniquenessStats, &'static str> {
    let sampled = count.min(MAX_STAT_ITEMS);
    let mut fingerprints = Vec::with_capacity(sampled as usize);

    for _ in 0..sampled {
        let output = generate_one()?;
        let mut hasher = DefaultHasher::new();
        output.hash(&mut hasher);
        fingerprints.push(hasher.finish());
    }

    fingerprints.sort_unstable();

    let mut unique = 0_u64;
    let mut collision_pairs = 0_u128;
    let mut max_frequency = 0_u64;
    let mut start = 0;
    while start < fingerprints.len() {
        let mut end = start + 1;
        while end < fingerprints.len() && fingerprints[end] == fingerprints[start] {
            end += 1;
        }

        let frequency = (end - start) as u64;
        unique += 1;
        collision_pairs += u128::from(frequency) * u128::from(frequency - 1) / 2;
        max_frequency = max_frequency.max(frequency);
        start = end;
    }

    Ok(UniquenessStats {
        sampled,
        unique,
        collision_pairs,
        max_frequency,
    })
}

fn max_timing_runs(count: u64) -> u64 {
    (MAX_TIMED_ITEMS / count).max(1)
}

fn measure_timing<W, T>(count: u64, mut warmup: W, mut timed: T) -> Result<Timing, String>
where
    W: FnMut() -> Result<String, &'static str>,
    T: FnMut() -> Result<String, &'static str>,
{
    generate(count.min(WARMUP_ITEMS), &mut warmup)
        .map_err(|error| format!("warmup failed: {error}"))?;

    let max_runs = max_timing_runs(count);
    let started = Instant::now();
    let mut runs = 0_u64;
    let mut next_chunk = 1_u64;

    loop {
        let chunk = next_chunk.min(max_runs - runs);
        for _ in 0..chunk {
            generate(count, &mut timed).map_err(|error| format!("generation failed: {error}"))?;
        }
        runs += chunk;

        if runs == max_runs || started.elapsed() >= MIN_TIMING_TIME {
            break;
        }
        next_chunk = next_chunk.saturating_mul(2);
    }

    let elapsed = started.elapsed();
    let timed_items = count as f64 * runs as f64;
    let seconds = elapsed.as_secs_f64();

    Ok(Timing {
        runs,
        items_per_second: timed_items / seconds,
        nanoseconds_per_item: seconds * 1_000_000_000.0 / timed_items,
    })
}

fn print_name_header() {
    println!(
        "{:<12} {:>8} {:>15} {:>12} {:>10} {:>10} {:>12} {:>12} {:>14} {:>19} {:>10}",
        "names",
        "runs",
        "names/second",
        "ns/name",
        "sampled",
        "unique",
        "duplicate %",
        "pair matches",
        "collision bits",
        "effective diversity",
        "max freq"
    );
}

fn print_name_measurement(count: u64, timing: &Timing, statistics: &UniquenessStats) {
    let Timing {
        runs,
        items_per_second,
        nanoseconds_per_item,
    } = timing;
    let collision_bits = statistics
        .collision_bits()
        .map(|bits| format!("{bits:.2}"))
        .unwrap_or_else(|| "n/a".to_owned());
    let effective_diversity = statistics
        .effective_diversity()
        .map(|diversity| format!("{diversity:.3e}"))
        .unwrap_or_else(|| "n/a".to_owned());

    println!(
        "{count:<12} {runs:>8} {items_per_second:>15.0} {nanoseconds_per_item:>12.0} {:>10} {:>10} {:>11.3}% {:>12} {collision_bits:>14} {effective_diversity:>19} {:>10}",
        statistics.sampled,
        statistics.unique,
        statistics.duplicate_percentage(),
        statistics.collision_pairs,
        statistics.max_frequency,
    );
}

fn print_sas_header() {
    println!(
        "{:<12} {:>8} {:>15} {:>12}",
        "phrases", "runs", "phrases/second", "ns/phrase"
    );
}

fn print_sas_measurement(count: u64, timing: &Timing) {
    let Timing {
        runs,
        items_per_second,
        nanoseconds_per_item,
    } = timing;

    println!("{count:<12} {runs:>8} {items_per_second:>15.0} {nanoseconds_per_item:>12.0}");
}

fn benchmark_names(grammar: &Grammar, config: &str, counts: &[u64]) -> Result<(), String> {
    println!("profile: {config}");
    println!("seed: {SEED}");
    print_name_header();

    for &count in counts {
        let mut warmup_rng = Rng::new(SEED);
        let mut timed_rng = Rng::new(SEED);
        let mut statistics_rng = Rng::new(SEED);
        let timing = measure_timing(
            count,
            || grammar.generate_name(&mut warmup_rng),
            || grammar.generate_name(&mut timed_rng),
        )?;
        let statistics = uniqueness_stats(count, || grammar.generate_name(&mut statistics_rng))
            .map_err(|error| format!("statistics pass failed: {error}"))?;
        print_name_measurement(count, &timing, &statistics);
    }

    Ok(())
}

fn benchmark_sas(counts: &[u64]) -> Result<(), String> {
    println!("mode: sas ({})", sas::VERSION);
    println!("inputs: distinct sequential 40-bit values starting at {SEED}");
    print_sas_header();

    for &count in counts {
        let mut warmup_inputs = SasInputs::new();
        let mut timed_inputs = SasInputs::new();
        let timing = measure_timing(
            count,
            || Ok(sas::encode(warmup_inputs.next())),
            || Ok(sas::encode(timed_inputs.next())),
        )?;
        print_sas_measurement(count, &timing);
    }

    Ok(())
}

fn main() -> ExitCode {
    let arguments = match parse_args(env::args().skip(1)) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("benchmark: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let result = match arguments.target {
        Target::Names(config) => match load_grammar(&config) {
            Ok(grammar) => benchmark_names(&grammar, &config, &arguments.counts),
            Err(error) => {
                eprintln!("benchmark: {error}");
                return ExitCode::from(2);
            }
        },
        Target::Sas => benchmark_sas(&arguments.counts),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("benchmark: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Result<Option<Arguments>, String> {
        parse_args(values.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn sas_mode_accepts_custom_and_default_counts() {
        assert_eq!(
            arguments(&["--sas", "12"]),
            Ok(Some(Arguments {
                target: Target::Sas,
                counts: vec![12],
            }))
        );
        assert_eq!(arguments(&["-sas", "12"]), arguments(&["--sas", "12"]));
        assert_eq!(
            arguments(&["--sas"]),
            Ok(Some(Arguments {
                target: Target::Sas,
                counts: DEFAULT_COUNTS.to_vec(),
            }))
        );
    }

    #[test]
    fn sas_mode_rejects_name_config_options() {
        assert!(arguments(&["--sas", "--config", "japanese"]).is_err());
        assert!(arguments(&["--sas", "--sas"]).is_err());
    }

    #[test]
    fn sas_inputs_are_sequential_and_wrap_at_forty_bits() {
        let mut inputs = SasInputs::new();
        assert_eq!(inputs.next(), [0, 0, 0, 0, 42]);
        assert_eq!(inputs.next(), [0, 0, 0, 0, 43]);

        let mut wrapping = SasInputs(SAS_MASK);
        assert_eq!(wrapping.next(), [0xff; sas::SAS_BYTES]);
        assert_eq!(wrapping.next(), [0; sas::SAS_BYTES]);
    }

    #[test]
    fn name_statistics_measure_output_concentration() {
        let mut outputs = ["a", "a", "a", "b", "c"].into_iter();
        let statistics = uniqueness_stats(5, || Ok(outputs.next().unwrap().to_owned())).unwrap();

        assert_eq!(statistics.sampled, 5);
        assert_eq!(statistics.unique, 3);
        assert_eq!(statistics.collision_pairs, 3);
        assert_eq!(statistics.max_frequency, 3);
        assert_eq!(statistics.duplicate_count(), 2);
        assert!((statistics.duplicate_percentage() - 40.0).abs() < f64::EPSILON);
        assert!((statistics.collision_probability().unwrap() - 0.3).abs() < f64::EPSILON);
        assert!((statistics.collision_bits().unwrap() - 1.736_965_594).abs() < 1e-9);
        assert!((statistics.effective_diversity().unwrap() - 10.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn timing_caps_total_items_but_always_runs_a_batch() {
        assert_eq!(max_timing_runs(1), MAX_TIMED_ITEMS);
        assert_eq!(max_timing_runs(1_000), 10_000);
        assert_eq!(max_timing_runs(1_000_000), 10);
        assert_eq!(max_timing_runs(MAX_TIMED_ITEMS + 1), 1);
    }
}
