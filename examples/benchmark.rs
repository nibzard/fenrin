// ABOUTME: Throughput benchmark for Fenrin with no benchmark-only dependencies.
// ABOUTME: It supports legacy reports plus fixed raw and distinct-session records.

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use fenrin::session::write_distinct_names;
use fenrin::{BUNDLED_CONFIGS, Grammar, Rng, config, sas};

const USAGE: &str = "Usage: cargo run --release --example benchmark -- [--config <name-or-path>] [count ...]\n       cargo run --release --example benchmark -- -sas|--sas [count ...]\n       benchmark --measure <raw|distinct> [--config <name-or-path>] [--seed <integer>] [--sessions <integer>] <count>";
const DEFAULT_CONFIG: &str = "fenrin";
const DEFAULT_COUNTS: [u64; 3] = [1_000, 10_000, 1_000_000];
const WARMUP_ITEMS: u64 = 1_000;
const MAX_STAT_ITEMS: u64 = 1_000_000;
const MIN_TIMING_TIME: Duration = Duration::from_millis(200);
const MAX_TIMED_ITEMS: u64 = 10_000_000;
const SEED: u64 = 42;
const SAS_MASK: u64 = (1_u64 << sas::SAS_BITS) - 1;
const FIXED_RECORD_VERSION: &str = "fenrin-fixed-v1";
const DEFAULT_FIXED_SESSIONS: u64 = 50;
const SESSION_SEED_STEP: u64 = 0x9e37_79b9_7f4a_7c15;

#[derive(Debug, PartialEq)]
enum Target {
    Names(String),
    Sas,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MeasurementMode {
    Raw,
    Distinct,
}

impl MeasurementMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "raw" => Ok(Self::Raw),
            "distinct" => Ok(Self::Distinct),
            _ => Err(format!(
                "invalid measurement mode `{value}`; expected `raw` or `distinct`"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Distinct => "distinct",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FixedOptions {
    mode: MeasurementMode,
    seed: u64,
    sessions: u64,
}

#[derive(Debug, PartialEq)]
struct Arguments {
    target: Target,
    counts: Vec<u64>,
    fixed: Option<FixedOptions>,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let mut config = None;
    let mut sas = false;
    let mut counts = Vec::new();
    let mut measurement = None;
    let mut seed = None;
    let mut sessions = None;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "-sas" | "--sas" => {
                if sas {
                    return Err("`--sas` specified more than once".to_owned());
                }
                sas = true;
            }
            "--measure" => {
                if measurement.is_some() {
                    return Err("`--measure` specified more than once".to_owned());
                }
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after `--measure`".to_owned())?;
                measurement = Some(MeasurementMode::parse(&value)?);
            }
            "--seed" => {
                if seed.is_some() {
                    return Err("`--seed` specified more than once".to_owned());
                }
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after `--seed`".to_owned())?;
                seed = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| "seed must be a non-negative integer".to_owned())?,
                );
            }
            "--sessions" => {
                if sessions.is_some() {
                    return Err("`--sessions` specified more than once".to_owned());
                }
                let value = args
                    .next()
                    .ok_or_else(|| "missing value after `--sessions`".to_owned())?;
                let value = value
                    .parse::<u64>()
                    .map_err(|_| "sessions must be a positive integer".to_owned())?;
                if value == 0 {
                    return Err("sessions must be a positive integer".to_owned());
                }
                sessions = Some(value);
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

    let target = match (sas, config) {
        (true, Some(_)) => return Err("`--sas` cannot be combined with `--config`".to_owned()),
        (true, None) => Target::Sas,
        (false, config) => Target::Names(config.unwrap_or_else(|| DEFAULT_CONFIG.to_owned())),
    };

    let fixed = match measurement {
        Some(mode) => {
            if sas {
                return Err("`--measure` cannot be combined with `--sas`".to_owned());
            }
            if counts.len() != 1 {
                return Err("fixed measurements require exactly one count".to_owned());
            }
            Some(FixedOptions {
                mode,
                seed: seed.unwrap_or(SEED),
                sessions: sessions.unwrap_or(DEFAULT_FIXED_SESSIONS),
            })
        }
        None => {
            if seed.is_some() {
                return Err("`--seed` requires `--measure`".to_owned());
            }
            if sessions.is_some() {
                return Err("`--sessions` requires `--measure`".to_owned());
            }
            if counts.is_empty() {
                counts.extend(DEFAULT_COUNTS);
            }
            None
        }
    };

    Ok(Some(Arguments {
        target,
        counts,
        fixed,
    }))
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
struct FixedMeasurement {
    mode: MeasurementMode,
    seed: u64,
    count: u64,
    sessions: u64,
    requested: u64,
    completed: u64,
    attempts: u64,
    elapsed_ns: u128,
    output_bytes: u64,
}

impl FixedMeasurement {
    fn names_per_second(&self) -> f64 {
        self.completed as f64 * 1_000_000_000.0 / self.elapsed_ns as f64
    }

    fn record(&self) -> String {
        format!(
            "{FIXED_RECORD_VERSION}\tmode={}\tseed={}\tcount={}\tsessions={}\twarmup_sessions=1\trequested={}\tcompleted={}\tattempts={}\telapsed_ns={}\tnames_per_second={:.6}\toutput_bytes={}",
            self.mode.as_str(),
            self.seed,
            self.count,
            self.sessions,
            self.requested,
            self.completed,
            self.attempts,
            self.elapsed_ns,
            self.names_per_second(),
            self.output_bytes,
        )
    }
}

#[derive(Default)]
struct CountingSink {
    bytes: u64,
}

impl Write for CountingSink {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buffer.len() as u64)
            .ok_or_else(|| io::Error::other("counting sink byte count overflowed"))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
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

fn session_seed(base: u64, session: u64) -> u64 {
    base.wrapping_add(SESSION_SEED_STEP.wrapping_mul(session))
}

fn warmup_seed(base: u64) -> u64 {
    base.wrapping_sub(SESSION_SEED_STEP)
}

fn run_raw_session(grammar: &Grammar, count: u64, seed: u64) -> Result<(), String> {
    let mut rng = Rng::new(seed);
    generate(count, || grammar.generate_name(&mut rng))
        .map_err(|error| format!("generation failed: {error}"))
}

fn run_distinct_session(
    grammar: &Grammar,
    count: usize,
    seed: u64,
) -> Result<(fenrin::session::DistinctSessionStats, u64), String> {
    let mut rng = Rng::new(seed);
    let mut output = BufWriter::new(CountingSink::default());
    let statistics = write_distinct_names(&mut output, count, &mut rng, grammar)
        .map_err(|error| format!("distinct session failed: {error}"))?;
    let output_bytes = black_box(output.get_ref().bytes);
    drop(output);
    Ok((statistics, output_bytes))
}

fn measure_fixed_names(
    grammar: &Grammar,
    options: FixedOptions,
    count: u64,
) -> Result<FixedMeasurement, String> {
    let requested = count
        .checked_mul(options.sessions)
        .ok_or_else(|| "count times sessions overflowed".to_owned())?;

    match options.mode {
        MeasurementMode::Raw => {
            run_raw_session(grammar, count, warmup_seed(options.seed))?;
            let started = Instant::now();
            for session in 0..options.sessions {
                run_raw_session(grammar, count, session_seed(options.seed, session))?;
            }
            let elapsed_ns = started.elapsed().as_nanos();

            Ok(FixedMeasurement {
                mode: options.mode,
                seed: options.seed,
                count,
                sessions: options.sessions,
                requested,
                completed: requested,
                attempts: requested,
                elapsed_ns,
                output_bytes: 0,
            })
        }
        MeasurementMode::Distinct => {
            let count_usize = usize::try_from(count)
                .map_err(|_| "distinct count is too large for this platform".to_owned())?;
            black_box(run_distinct_session(
                grammar,
                count_usize,
                warmup_seed(options.seed),
            )?);
            let started = Instant::now();
            let mut completed = 0_u64;
            let mut attempts = 0_u64;
            let mut output_bytes = 0_u64;
            for session in 0..options.sessions {
                let (statistics, session_bytes) = run_distinct_session(
                    grammar,
                    count_usize,
                    session_seed(options.seed, session),
                )?;
                completed = completed
                    .checked_add(statistics.names as u64)
                    .ok_or_else(|| "completed name count overflowed".to_owned())?;
                attempts = attempts
                    .checked_add(statistics.attempts as u64)
                    .ok_or_else(|| "attempt count overflowed".to_owned())?;
                output_bytes = output_bytes
                    .checked_add(session_bytes)
                    .ok_or_else(|| "formatted byte count overflowed".to_owned())?;
            }
            let elapsed_ns = started.elapsed().as_nanos();

            Ok(FixedMeasurement {
                mode: options.mode,
                seed: options.seed,
                count,
                sessions: options.sessions,
                requested,
                completed,
                attempts,
                elapsed_ns,
                output_bytes,
            })
        }
    }
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

    let Arguments {
        target,
        counts,
        fixed,
    } = arguments;
    let result = match target {
        Target::Names(config) => match load_grammar(&config) {
            Ok(grammar) => match fixed {
                Some(options) => measure_fixed_names(&grammar, options, counts[0]).map(|result| {
                    println!("{}", result.record());
                }),
                None => benchmark_names(&grammar, &config, &counts),
            },
            Err(error) => {
                eprintln!("benchmark: {error}");
                return ExitCode::from(2);
            }
        },
        Target::Sas => benchmark_sas(&counts),
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
                fixed: None,
            }))
        );
        assert_eq!(arguments(&["-sas", "12"]), arguments(&["--sas", "12"]));
        assert_eq!(
            arguments(&["--sas"]),
            Ok(Some(Arguments {
                target: Target::Sas,
                counts: DEFAULT_COUNTS.to_vec(),
                fixed: None,
            }))
        );
    }

    #[test]
    fn sas_mode_rejects_name_config_options() {
        assert!(arguments(&["--sas", "--config", "japanese"]).is_err());
        assert!(arguments(&["--sas", "--sas"]).is_err());
    }

    #[test]
    fn fixed_measurements_accept_a_mode_seed_config_and_one_count() {
        assert_eq!(
            arguments(&[
                "--measure",
                "distinct",
                "--seed",
                "99",
                "--sessions",
                "3",
                "--config",
                "japanese",
                "10000",
            ]),
            Ok(Some(Arguments {
                target: Target::Names("japanese".to_owned()),
                counts: vec![10_000],
                fixed: Some(FixedOptions {
                    mode: MeasurementMode::Distinct,
                    seed: 99,
                    sessions: 3,
                }),
            }))
        );
        assert_eq!(
            arguments(&["--measure", "raw", "1"])
                .unwrap()
                .unwrap()
                .fixed,
            Some(FixedOptions {
                mode: MeasurementMode::Raw,
                seed: SEED,
                sessions: DEFAULT_FIXED_SESSIONS,
            })
        );
    }

    #[test]
    fn fixed_measurements_reject_ambiguous_or_legacy_combinations() {
        assert!(arguments(&["--measure", "other", "10"]).is_err());
        assert!(arguments(&["--measure", "raw"]).is_err());
        assert!(arguments(&["--measure", "raw", "10", "20"]).is_err());
        assert!(arguments(&["--measure", "raw", "--sas", "10"]).is_err());
        assert!(arguments(&["--seed", "7", "10"]).is_err());
        assert!(arguments(&["--sessions", "2", "10"]).is_err());
        assert!(arguments(&["--measure", "raw", "--sessions", "0", "10"]).is_err());
        assert!(arguments(&["--measure", "raw", "--seed", "bad", "10"]).is_err());
    }

    #[test]
    fn fixed_record_is_stable_and_machine_readable() {
        let measurement = FixedMeasurement {
            mode: MeasurementMode::Distinct,
            seed: 7,
            count: 10,
            sessions: 1,
            requested: 10,
            completed: 10,
            attempts: 12,
            elapsed_ns: 2_000,
            output_bytes: 80,
        };

        assert_eq!(
            measurement.record(),
            "fenrin-fixed-v1\tmode=distinct\tseed=7\tcount=10\tsessions=1\twarmup_sessions=1\trequested=10\tcompleted=10\tattempts=12\telapsed_ns=2000\tnames_per_second=5000000.000000\toutput_bytes=80"
        );
    }

    #[test]
    fn fixed_measurement_aggregates_preregistered_sessions_after_one_warmup() {
        let grammar = load_grammar("fenrin").unwrap();
        let raw = measure_fixed_names(
            &grammar,
            FixedOptions {
                mode: MeasurementMode::Raw,
                seed: 11,
                sessions: 3,
            },
            2,
        )
        .unwrap();
        assert_eq!(raw.requested, 6);
        assert_eq!(raw.completed, 6);
        assert_eq!(raw.attempts, 6);

        let distinct = measure_fixed_names(
            &grammar,
            FixedOptions {
                mode: MeasurementMode::Distinct,
                seed: 11,
                sessions: 2,
            },
            2,
        )
        .unwrap();
        assert_eq!(distinct.requested, 4);
        assert_eq!(distinct.completed, 4);
        assert!(distinct.attempts >= 4);
        assert!(distinct.output_bytes > 4);
    }

    #[test]
    fn session_seeds_are_reproducible_and_do_not_reuse_the_warmup() {
        assert_eq!(session_seed(17, 0), 17);
        assert_eq!(session_seed(17, 2), session_seed(17, 2));
        assert_ne!(session_seed(17, 1), session_seed(17, 2));
        assert_ne!(warmup_seed(17), session_seed(17, 0));
    }

    #[test]
    fn counting_sink_accepts_formatted_output_without_storing_it() {
        let mut sink = CountingSink::default();
        writeln!(&mut sink, "alpha").unwrap();
        write!(&mut sink, "beta").unwrap();

        assert_eq!(sink.bytes, 10);
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
