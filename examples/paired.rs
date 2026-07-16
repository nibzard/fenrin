// ABOUTME: Dependency-free paired A/B runner for prebuilt Fenrin benchmark binaries.
// ABOUTME: It preregisters ABBA/BAAB blocks and analyzes paired log throughput ratios.

use std::env;
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "Usage: paired --baseline <benchmark-bin> --candidate <benchmark-bin> [options]\n       paired --aa <benchmark-bin> [options]\n\nOptions:\n  --mode <raw|distinct>       Measurement path (default: distinct)\n  --config <name-or-path>     Grammar profile (default: fenrin)\n  --count <integer>           Names per fixed session (default: 10000)\n  --sessions <integer>        Timed sessions per process (default: 50)\n  --blocks <integer>          Randomized four-run blocks (default: 16)\n  --seed <integer>            Base work seed; repeat to cycle seeds (default: 42)\n  --order-seed <integer>      Reproducible schedule seed (default: 1)\n  --schedule <orders>         Explicit comma-separated ABBA/BAAB schedule\n  --target-speedup <percent>  A/A power target (default: 3)\n  --held-out                  Label a fresh final confirmation run";
const FIXED_RECORD_VERSION: &str = "fenrin-fixed-v1";
const DEFAULT_CONFIG: &str = "fenrin";
const DEFAULT_COUNT: u64 = 10_000;
const DEFAULT_SESSIONS: u64 = 50;
const DEFAULT_BLOCKS: usize = 16;
const DEFAULT_WORK_SEED: u64 = 42;
const DEFAULT_ORDER_SEED: u64 = 1;
const DEFAULT_TARGET_SPEEDUP_PERCENT: f64 = 3.0;
const NORMAL_95_ONE_SIDED: f64 = 1.644_853_626_951_472_2;
const NORMAL_80_POWER: f64 = 0.841_621_233_572_914_3;

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
                "invalid mode `{value}`; expected `raw` or `distinct`"
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
enum Label {
    Baseline,
    Candidate,
}

impl Label {
    fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "A",
            Self::Candidate => "B",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockOrder {
    Abba,
    Baab,
}

impl BlockOrder {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_uppercase().as_str() {
            "ABBA" => Ok(Self::Abba),
            "BAAB" => Ok(Self::Baab),
            _ => Err(format!(
                "invalid block order `{value}`; expected `ABBA` or `BAAB`"
            )),
        }
    }

    fn labels(self) -> [Label; 4] {
        match self {
            Self::Abba => [
                Label::Baseline,
                Label::Candidate,
                Label::Candidate,
                Label::Baseline,
            ],
            Self::Baab => [
                Label::Candidate,
                Label::Baseline,
                Label::Baseline,
                Label::Candidate,
            ],
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Abba => "ABBA",
            Self::Baab => "BAAB",
        }
    }
}

#[derive(Debug, PartialEq)]
struct Arguments {
    baseline: PathBuf,
    candidate: PathBuf,
    calibration: bool,
    mode: MeasurementMode,
    config: String,
    count: u64,
    sessions: u64,
    seeds: Vec<u64>,
    schedule: Vec<BlockOrder>,
    order_seed: Option<u64>,
    target_speedup_percent: f64,
    held_out: bool,
}

fn parse_positive_u64(value: &str, description: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("{description} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{description} must be a positive integer"));
    }
    Ok(parsed)
}

fn parse_schedule(value: &str) -> Result<Vec<BlockOrder>, String> {
    if value.is_empty() {
        return Err("schedule must contain ABBA or BAAB blocks".to_owned());
    }
    value.split(',').map(BlockOrder::parse).collect()
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let mut baseline = None;
    let mut candidate = None;
    let mut aa = None;
    let mut mode = MeasurementMode::Distinct;
    let mut mode_set = false;
    let mut config = None;
    let mut count = None;
    let mut sessions = None;
    let mut blocks = None;
    let mut seeds = Vec::new();
    let mut order_seed = None;
    let mut explicit_schedule = None;
    let mut target_speedup_percent = None;
    let mut held_out = false;

    while let Some(argument) = args.next() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Ok(None);
        }

        let mut next = |option: &str| {
            args.next()
                .ok_or_else(|| format!("missing value after `{option}`"))
        };
        match argument.as_str() {
            "--baseline" => {
                if baseline.is_some() {
                    return Err("`--baseline` specified more than once".to_owned());
                }
                baseline = Some(PathBuf::from(next("--baseline")?));
            }
            "--candidate" => {
                if candidate.is_some() {
                    return Err("`--candidate` specified more than once".to_owned());
                }
                candidate = Some(PathBuf::from(next("--candidate")?));
            }
            "--aa" => {
                if aa.is_some() {
                    return Err("`--aa` specified more than once".to_owned());
                }
                aa = Some(PathBuf::from(next("--aa")?));
            }
            "--mode" => {
                if mode_set {
                    return Err("`--mode` specified more than once".to_owned());
                }
                mode = MeasurementMode::parse(&next("--mode")?)?;
                mode_set = true;
            }
            "--config" => {
                if config.is_some() {
                    return Err("`--config` specified more than once".to_owned());
                }
                config = Some(next("--config")?);
            }
            "--count" => {
                if count.is_some() {
                    return Err("`--count` specified more than once".to_owned());
                }
                count = Some(parse_positive_u64(&next("--count")?, "count")?);
            }
            "--sessions" => {
                if sessions.is_some() {
                    return Err("`--sessions` specified more than once".to_owned());
                }
                sessions = Some(parse_positive_u64(&next("--sessions")?, "sessions")?);
            }
            "--blocks" => {
                if blocks.is_some() {
                    return Err("`--blocks` specified more than once".to_owned());
                }
                let parsed = parse_positive_u64(&next("--blocks")?, "blocks")?;
                blocks = Some(
                    usize::try_from(parsed)
                        .map_err(|_| "block count is too large for this platform".to_owned())?,
                );
            }
            "--seed" => {
                let value = next("--seed")?;
                seeds.push(
                    value
                        .parse::<u64>()
                        .map_err(|_| "seed must be a non-negative integer".to_owned())?,
                );
            }
            "--order-seed" => {
                if order_seed.is_some() {
                    return Err("`--order-seed` specified more than once".to_owned());
                }
                order_seed = Some(
                    next("--order-seed")?
                        .parse::<u64>()
                        .map_err(|_| "order seed must be a non-negative integer".to_owned())?,
                );
            }
            "--schedule" => {
                if explicit_schedule.is_some() {
                    return Err("`--schedule` specified more than once".to_owned());
                }
                explicit_schedule = Some(parse_schedule(&next("--schedule")?)?);
            }
            "--target-speedup" => {
                if target_speedup_percent.is_some() {
                    return Err("`--target-speedup` specified more than once".to_owned());
                }
                let parsed = next("--target-speedup")?
                    .parse::<f64>()
                    .map_err(|_| "target speedup must be a positive percentage".to_owned())?;
                if !parsed.is_finite() || parsed <= 0.0 {
                    return Err("target speedup must be a positive percentage".to_owned());
                }
                target_speedup_percent = Some(parsed);
            }
            "--held-out" => {
                if held_out {
                    return Err("`--held-out` specified more than once".to_owned());
                }
                held_out = true;
            }
            _ => return Err(format!("unknown option `{argument}`")),
        }
    }

    let (baseline, candidate, calibration) = match (aa, baseline, candidate) {
        (Some(binary), None, None) => (binary.clone(), binary, true),
        (Some(_), _, _) => {
            return Err("`--aa` cannot be combined with `--baseline` or `--candidate`".to_owned());
        }
        (None, Some(baseline), Some(candidate)) => (baseline, candidate, false),
        (None, _, _) => {
            return Err("provide both `--baseline` and `--candidate`, or use `--aa`".to_owned());
        }
    };
    if calibration && held_out {
        return Err("`--held-out` cannot be combined with `--aa`".to_owned());
    }

    let (schedule, registered_order_seed) = match explicit_schedule {
        Some(schedule) => {
            if blocks.is_some() || order_seed.is_some() {
                return Err(
                    "`--schedule` cannot be combined with `--blocks` or `--order-seed`".to_owned(),
                );
            }
            (schedule, None)
        }
        None => {
            let order_seed = order_seed.unwrap_or(DEFAULT_ORDER_SEED);
            (
                randomized_schedule(blocks.unwrap_or(DEFAULT_BLOCKS), order_seed),
                Some(order_seed),
            )
        }
    };
    if schedule.len() < 2 {
        return Err("at least two blocks are required for a confidence bound".to_owned());
    }
    if seeds.is_empty() {
        seeds.push(DEFAULT_WORK_SEED);
    }

    Ok(Some(Arguments {
        baseline,
        candidate,
        calibration,
        mode,
        config: config.unwrap_or_else(|| DEFAULT_CONFIG.to_owned()),
        count: count.unwrap_or(DEFAULT_COUNT),
        sessions: sessions.unwrap_or(DEFAULT_SESSIONS),
        seeds,
        schedule,
        order_seed: registered_order_seed,
        target_speedup_percent: target_speedup_percent.unwrap_or(DEFAULT_TARGET_SPEEDUP_PERCENT),
        held_out,
    }))
}

struct ScheduleRng(u64);

impl ScheduleRng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

fn randomized_schedule(blocks: usize, seed: u64) -> Vec<BlockOrder> {
    let mut rng = ScheduleRng(seed);
    (0..blocks)
        .map(|_| {
            if rng.next_u64() & 1 == 0 {
                BlockOrder::Abba
            } else {
                BlockOrder::Baab
            }
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct FixedRecord {
    mode: MeasurementMode,
    seed: u64,
    count: u64,
    sessions: u64,
    warmup_sessions: u64,
    requested: u64,
    completed: u64,
    attempts: u64,
    elapsed_ns: u128,
    output_bytes: u64,
}

fn record_field<'a>(fields: &'a [&'a str], key: &str) -> Result<&'a str, String> {
    let prefix = format!("{key}=");
    let mut matches = fields
        .iter()
        .filter_map(|field| field.strip_prefix(&prefix));
    let value = matches
        .next()
        .ok_or_else(|| format!("measurement record is missing `{key}`"))?;
    if matches.next().is_some() {
        return Err(format!("measurement record repeats `{key}`"));
    }
    Ok(value)
}

fn parse_fixed_record(text: &str) -> Result<FixedRecord, String> {
    let mut lines = text.lines();
    let line = lines
        .next()
        .ok_or_else(|| "benchmark produced no measurement record".to_owned())?;
    if lines.next().is_some() {
        return Err("benchmark produced more than one output line".to_owned());
    }
    let fields: Vec<_> = line.split('\t').collect();
    if fields.first().copied() != Some(FIXED_RECORD_VERSION) {
        return Err(format!(
            "unsupported measurement record version `{}`",
            fields.first().copied().unwrap_or("")
        ));
    }

    let parse_u64 = |key| -> Result<u64, String> {
        record_field(&fields, key)?
            .parse::<u64>()
            .map_err(|_| format!("measurement field `{key}` is not an integer"))
    };
    let parse_u128 = |key| -> Result<u128, String> {
        record_field(&fields, key)?
            .parse::<u128>()
            .map_err(|_| format!("measurement field `{key}` is not an integer"))
    };

    let record = FixedRecord {
        mode: MeasurementMode::parse(record_field(&fields, "mode")?)?,
        seed: parse_u64("seed")?,
        count: parse_u64("count")?,
        sessions: parse_u64("sessions")?,
        warmup_sessions: parse_u64("warmup_sessions")?,
        requested: parse_u64("requested")?,
        completed: parse_u64("completed")?,
        attempts: parse_u64("attempts")?,
        elapsed_ns: parse_u128("elapsed_ns")?,
        output_bytes: parse_u64("output_bytes")?,
    };
    if record.elapsed_ns == 0 {
        return Err("measurement elapsed time must be greater than zero".to_owned());
    }
    Ok(record)
}

fn compact_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.trim()
        .chars()
        .map(|character| {
            if matches!(character, '\n' | '\r' | '\t') {
                ' '
            } else {
                character
            }
        })
        .take(300)
        .collect()
}

fn invoke(
    binary: &PathBuf,
    mode: MeasurementMode,
    config: &str,
    count: u64,
    sessions: u64,
    seed: u64,
) -> Result<FixedRecord, String> {
    let output = Command::new(binary)
        .arg("--measure")
        .arg(mode.as_str())
        .arg("--config")
        .arg(config)
        .arg("--seed")
        .arg(seed.to_string())
        .arg("--sessions")
        .arg(sessions.to_string())
        .arg(count.to_string())
        .output()
        .map_err(|error| format!("could not run {}: {error}", binary.display()))?;

    if !output.status.success() {
        return Err(format!(
            "{} exited with {}; stderr: {}",
            binary.display(),
            output.status,
            compact_output(&output.stderr)
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| format!("{} emitted non-UTF-8 output", binary.display()))?;
    let record = parse_fixed_record(&stdout)?;
    if record.mode != mode {
        return Err(format!(
            "{} reported mode {} instead of {}",
            binary.display(),
            record.mode.as_str(),
            mode.as_str()
        ));
    }
    let requested = count
        .checked_mul(sessions)
        .ok_or_else(|| "count times sessions overflowed".to_owned())?;
    if record.seed != seed
        || record.count != count
        || record.sessions != sessions
        || record.warmup_sessions != 1
        || record.requested != requested
        || record.completed != requested
    {
        return Err(format!(
            "{} reported mismatched work (seed={}, count={}, sessions={}, warmup_sessions={}, requested={}, completed={})",
            binary.display(),
            record.seed,
            record.count,
            record.sessions,
            record.warmup_sessions,
            record.requested,
            record.completed
        ));
    }
    Ok(record)
}

fn throughput(record: &FixedRecord) -> f64 {
    record.completed as f64 * 1_000_000_000.0 / record.elapsed_ns as f64
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Summary {
    blocks: usize,
    mean_log_ratio: f64,
    log_ratio_sd: f64,
    estimate_ratio: f64,
    lower_95_ratio: f64,
}

fn t_critical_95_one_sided(degrees_of_freedom: usize) -> f64 {
    const SMALL_DF: [f64; 30] = [
        6.314, 2.920, 2.353, 2.132, 2.015, 1.943, 1.895, 1.860, 1.833, 1.812, 1.796, 1.782, 1.771,
        1.761, 1.753, 1.746, 1.740, 1.734, 1.729, 1.725, 1.721, 1.717, 1.714, 1.711, 1.708, 1.706,
        1.703, 1.701, 1.699, 1.697,
    ];
    if degrees_of_freedom <= SMALL_DF.len() {
        return SMALL_DF[degrees_of_freedom - 1];
    }

    let df = degrees_of_freedom as f64;
    let z = NORMAL_95_ONE_SIDED;
    z + (z.powi(3) + z) / (4.0 * df)
        + (5.0 * z.powi(5) + 16.0 * z.powi(3) + 3.0 * z) / (96.0 * df.powi(2))
        + (3.0 * z.powi(7) + 19.0 * z.powi(5) + 17.0 * z.powi(3) - 15.0 * z) / (384.0 * df.powi(3))
}

fn summarize(log_ratios: &[f64]) -> Option<Summary> {
    if log_ratios.len() < 2 {
        return None;
    }
    let blocks = log_ratios.len();
    let mean = log_ratios.iter().sum::<f64>() / blocks as f64;
    let squared_deviations = log_ratios
        .iter()
        .map(|ratio| (ratio - mean).powi(2))
        .sum::<f64>();
    let standard_deviation = (squared_deviations / (blocks - 1) as f64).sqrt();
    let standard_error = standard_deviation / (blocks as f64).sqrt();
    let lower_log = mean - t_critical_95_one_sided(blocks - 1) * standard_error;

    Some(Summary {
        blocks,
        mean_log_ratio: mean,
        log_ratio_sd: standard_deviation,
        estimate_ratio: mean.exp(),
        lower_95_ratio: lower_log.exp(),
    })
}

fn approximate_power_blocks(log_ratio_sd: f64, target_speedup_percent: f64) -> usize {
    let target_log_ratio = (1.0 + target_speedup_percent / 100.0).ln();
    let estimate = ((NORMAL_95_ONE_SIDED + NORMAL_80_POWER) * log_ratio_sd / target_log_ratio)
        .powi(2)
        .ceil();
    if estimate.is_finite() {
        (estimate as usize).max(2)
    } else {
        usize::MAX
    }
}

fn percentage(ratio: f64) -> f64 {
    (ratio - 1.0) * 100.0
}

impl fmt::Display for BlockOrder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn execute(arguments: &Arguments) -> Result<bool, String> {
    let scope = if arguments.calibration {
        "calibration"
    } else if arguments.held_out {
        "held_out_confirmation"
    } else {
        "exploratory_per_candidate"
    };
    let schedule = arguments
        .schedule
        .iter()
        .map(|order| order.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let seeds = arguments
        .seeds
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let schedule_source = arguments
        .order_seed
        .map(|seed| format!("random:{seed}"))
        .unwrap_or_else(|| "explicit".to_owned());
    println!(
        "PLAN experiment={} scope={} mode={} config={} count={} sessions={} blocks={} order_source={} schedule={} seeds={}",
        if arguments.calibration { "AA" } else { "AB" },
        scope,
        arguments.mode.as_str(),
        arguments.config,
        arguments.count,
        arguments.sessions,
        arguments.schedule.len(),
        schedule_source,
        schedule,
        seeds,
    );
    io::stdout()
        .flush()
        .map_err(|error| format!("could not flush preregistered plan: {error}"))?;

    let mut log_ratios = Vec::with_capacity(arguments.schedule.len());
    let mut invalid_blocks = 0;

    for (block_index, &order) in arguments.schedule.iter().enumerate() {
        let seed = arguments.seeds[block_index % arguments.seeds.len()];
        let mut baseline_logs = Vec::with_capacity(2);
        let mut candidate_logs = Vec::with_capacity(2);
        let mut valid = true;

        for (position, label) in order.labels().into_iter().enumerate() {
            let binary = match label {
                Label::Baseline => &arguments.baseline,
                Label::Candidate => &arguments.candidate,
            };
            match invoke(
                binary,
                arguments.mode,
                &arguments.config,
                arguments.count,
                arguments.sessions,
                seed,
            ) {
                Ok(record) => {
                    let speed = throughput(&record);
                    println!(
                        "OBS block={} position={} label={} order={} seed={} elapsed_ns={} names_per_second={:.6} attempts={} output_bytes={}",
                        block_index + 1,
                        position + 1,
                        label.as_str(),
                        order,
                        seed,
                        record.elapsed_ns,
                        speed,
                        record.attempts,
                        record.output_bytes,
                    );
                    match label {
                        Label::Baseline => baseline_logs.push(speed.ln()),
                        Label::Candidate => candidate_logs.push(speed.ln()),
                    }
                }
                Err(error) => {
                    valid = false;
                    eprintln!(
                        "INVALID block={} position={} label={} order={} seed={}: {}",
                        block_index + 1,
                        position + 1,
                        label.as_str(),
                        order,
                        seed,
                        error,
                    );
                }
            }
        }

        if valid && baseline_logs.len() == 2 && candidate_logs.len() == 2 {
            let baseline_log = baseline_logs.iter().sum::<f64>() / 2.0;
            let candidate_log = candidate_logs.iter().sum::<f64>() / 2.0;
            let log_ratio = candidate_log - baseline_log;
            log_ratios.push(log_ratio);
            println!(
                "BLOCK block={} valid=true log_ratio={:.9} speedup_ratio={:.6} speedup_percent={:.3}",
                block_index + 1,
                log_ratio,
                log_ratio.exp(),
                percentage(log_ratio.exp()),
            );
        } else {
            invalid_blocks += 1;
            println!("BLOCK block={} valid=false", block_index + 1);
        }
    }

    let summary = summarize(&log_ratios).ok_or_else(|| {
        format!(
            "only {} valid block(s); at least two are required",
            log_ratios.len()
        )
    })?;
    println!(
        "RESULT experiment={} scope={} mode={} valid_blocks={} planned_blocks={} invalid_blocks={} mean_log_ratio={:.9} log_ratio_sd={:.9} speedup_ratio={:.6} speedup_percent={:.3} lower_95_one_sided_ratio={:.6} lower_95_one_sided_percent={:.3} evidence={}",
        if arguments.calibration { "AA" } else { "AB" },
        scope,
        arguments.mode.as_str(),
        summary.blocks,
        arguments.schedule.len(),
        invalid_blocks,
        summary.mean_log_ratio,
        summary.log_ratio_sd,
        summary.estimate_ratio,
        percentage(summary.estimate_ratio),
        summary.lower_95_ratio,
        percentage(summary.lower_95_ratio),
        if arguments.calibration {
            "calibration_only"
        } else if arguments.held_out && summary.lower_95_ratio > 1.0 {
            "candidate_faster"
        } else if arguments.held_out {
            "inconclusive"
        } else if summary.lower_95_ratio > 1.0 {
            "screen_positive"
        } else {
            "screen_inconclusive"
        },
    );
    if arguments.calibration {
        println!(
            "CALIBRATION target_speedup_percent={:.3} approximate_blocks_for_80_percent_power={}",
            arguments.target_speedup_percent,
            approximate_power_blocks(summary.log_ratio_sd, arguments.target_speedup_percent),
        );
    }

    Ok(invalid_blocks != 0)
}

fn main() -> ExitCode {
    let arguments = match parse_args(env::args().skip(1)) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("paired: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match execute(&arguments) {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => {
            eprintln!(
                "paired: one or more preregistered blocks were invalid; no runs were replaced"
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("paired: {error}");
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
    fn explicit_ab_and_aa_arguments_are_unambiguous() {
        let ab = arguments(&[
            "--baseline",
            "/tmp/a",
            "--candidate",
            "/tmp/b",
            "--mode",
            "raw",
            "--config",
            "japanese",
            "--count",
            "123",
            "--sessions",
            "7",
            "--seed",
            "7",
            "--seed",
            "9",
            "--schedule",
            "ABBA,BAAB",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(ab.baseline, PathBuf::from("/tmp/a"));
        assert_eq!(ab.candidate, PathBuf::from("/tmp/b"));
        assert!(!ab.calibration);
        assert_eq!(ab.mode, MeasurementMode::Raw);
        assert_eq!(ab.config, "japanese");
        assert_eq!(ab.count, 123);
        assert_eq!(ab.sessions, 7);
        assert_eq!(ab.seeds, [7, 9]);
        assert_eq!(ab.schedule, [BlockOrder::Abba, BlockOrder::Baab]);
        assert_eq!(ab.order_seed, None);

        let aa = arguments(&["--aa", "/tmp/a", "--blocks", "2"])
            .unwrap()
            .unwrap();
        assert_eq!(aa.baseline, aa.candidate);
        assert!(aa.calibration);
        assert_eq!(aa.seeds, [DEFAULT_WORK_SEED]);
        assert_eq!(aa.sessions, DEFAULT_SESSIONS);
        assert_eq!(aa.order_seed, Some(DEFAULT_ORDER_SEED));
        assert!(!aa.held_out);
    }

    #[test]
    fn parser_rejects_partial_or_conflicting_designs() {
        assert!(arguments(&[]).is_err());
        assert!(arguments(&["--baseline", "a"]).is_err());
        assert!(arguments(&["--aa", "a", "--baseline", "a", "--candidate", "b"]).is_err());
        assert!(arguments(&["--aa", "a", "--blocks", "1"]).is_err());
        assert!(
            arguments(&["--aa", "a", "--schedule", "ABBA,BAAB", "--order-seed", "2",]).is_err()
        );
        assert!(arguments(&["--aa", "a", "--mode", "sas"]).is_err());
        assert!(arguments(&["--aa", "a", "--count", "0"]).is_err());
        assert!(arguments(&["--aa", "a", "--sessions", "0"]).is_err());
        assert!(arguments(&["--aa", "a", "--held-out"]).is_err());
    }

    #[test]
    fn randomized_schedule_is_reproducible() {
        let first = randomized_schedule(32, 91);
        let second = randomized_schedule(32, 91);
        let other = randomized_schedule(32, 92);

        assert_eq!(first, second);
        assert_ne!(first, other);
        assert!(first.contains(&BlockOrder::Abba));
        assert!(first.contains(&BlockOrder::Baab));
    }

    #[test]
    fn machine_record_parser_checks_version_and_fields() {
        let text = "fenrin-fixed-v1\tmode=distinct\tseed=42\tcount=10000\tsessions=50\twarmup_sessions=1\trequested=500000\tcompleted=500000\tattempts=600000\telapsed_ns=500000000\tnames_per_second=1000000.0\toutput_bytes=4500000\n";
        assert_eq!(
            parse_fixed_record(text),
            Ok(FixedRecord {
                mode: MeasurementMode::Distinct,
                seed: 42,
                count: 10_000,
                sessions: 50,
                warmup_sessions: 1,
                requested: 500_000,
                completed: 500_000,
                attempts: 600_000,
                elapsed_ns: 500_000_000,
                output_bytes: 4_500_000,
            })
        );
        assert!(parse_fixed_record(&text.replacen("fenrin-fixed-v1", "old", 1)).is_err());
        assert!(parse_fixed_record(&text.replace("elapsed_ns=500000000", "elapsed_ns=0")).is_err());
        assert!(parse_fixed_record(&format!("{text}extra\n")).is_err());
    }

    #[test]
    fn summary_uses_geometric_speedup_and_a_one_sided_bound() {
        let constant = vec![1.1_f64.ln(); 4];
        let summary = summarize(&constant).unwrap();

        assert!((summary.mean_log_ratio - 1.1_f64.ln()).abs() < 1e-12);
        assert!((summary.estimate_ratio - 1.1).abs() < 1e-12);
        assert!((summary.lower_95_ratio - 1.1).abs() < 1e-12);
        assert_eq!(summary.log_ratio_sd, 0.0);
    }

    #[test]
    fn aa_power_estimate_grows_with_noise_and_smaller_effects() {
        let low_noise = approximate_power_blocks(0.01, 3.0);
        let high_noise = approximate_power_blocks(0.05, 3.0);
        let smaller_effect = approximate_power_blocks(0.05, 1.0);

        assert!(high_noise > low_noise);
        assert!(smaller_effect > high_noise);
    }
}
