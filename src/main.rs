use std::collections::HashSet;
use std::env;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use fenrin::grammar::{Grammar, Rng};
use fenrin::{BUNDLED_CONFIGS, config, sas};

const USAGE: &str = "Usage: fenrin [--config <name-or-path>] [--seed <integer>] <count>\n       fenrin -sas|--sas [10-hex-digits]\n       fenrin --sas-words";
const CONFIG_DIRECTORY: &str = "configs";
const DEFAULT_CONFIG: &str = "fenrin";
const MAX_COUNT: usize = 10_000;
const ATTEMPTS_PER_NAME: usize = 100;

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Generate {
        count: usize,
        config: PathBuf,
        seed: Option<u64>,
    },
    Sas {
        bytes: Option<[u8; sas::SAS_BYTES]>,
    },
    SasWords,
}

fn parse_sas_hex(value: &str) -> Result<[u8; sas::SAS_BYTES], String> {
    if value.len() != sas::SAS_BYTES * 2 || !value.is_ascii() {
        return Err(format!(
            "SAS value must be exactly {} hexadecimal digits",
            sas::SAS_BYTES * 2
        ));
    }

    let mut bytes = [0_u8; sas::SAS_BYTES];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&value[start..start + 2], 16).map_err(|_| {
            format!(
                "SAS value must be exactly {} hexadecimal digits",
                sas::SAS_BYTES * 2
            )
        })?;
    }
    Ok(bytes)
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let arguments: Vec<_> = args.collect();
    if arguments
        .iter()
        .any(|argument| matches!(argument.as_str(), "-h" | "--help"))
    {
        return Ok(Command::Help);
    }
    let mut args = arguments.into_iter();

    let mut count = None;
    let mut config = None;
    let mut seed = None;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-sas" | "--sas" => {
                if count.is_some() || config.is_some() || seed.is_some() {
                    return Err("`--sas` cannot be combined with name generation".to_owned());
                }
                let bytes = args.next().map(|value| parse_sas_hex(&value)).transpose()?;
                if args.next().is_some() {
                    return Err("`--sas` accepts at most one value".to_owned());
                }
                return Ok(Command::Sas { bytes });
            }
            "--sas-words" => {
                if count.is_some() || config.is_some() || seed.is_some() {
                    return Err("`--sas-words` cannot be combined with name generation".to_owned());
                }
                if args.next().is_some() {
                    return Err("`--sas-words` accepts no value".to_owned());
                }
                return Ok(Command::SasWords);
            }
            "-s" | "--seed" => {
                if seed.is_some() {
                    return Err("seed specified more than once".to_owned());
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
            "-c" | "--config" => {
                if config.is_some() {
                    return Err("config path specified more than once".to_owned());
                }
                let path = args
                    .next()
                    .ok_or_else(|| "missing path after `--config`".to_owned())?;
                config = Some(PathBuf::from(path));
            }
            _ if argument.starts_with('-') => {
                return Err(format!("unknown option `{argument}`"));
            }
            _ => {
                if count.is_some() {
                    return Err("expected exactly one count".to_owned());
                }
                let parsed = argument
                    .parse::<usize>()
                    .map_err(|_| "count must be a non-negative integer".to_owned())?;
                if parsed > MAX_COUNT {
                    return Err(format!("count must not exceed {MAX_COUNT}"));
                }
                count = Some(parsed);
            }
        }
    }

    let count = count.ok_or_else(|| "missing count".to_owned())?;
    Ok(Command::Generate {
        count,
        config: config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG)),
        seed,
    })
}

fn entropy_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    nanos as u64 ^ (nanos >> 64) as u64 ^ u64::from(process::id()).rotate_left(32)
}

fn is_bare_config(requested: &Path) -> bool {
    requested.file_name().is_some()
        && requested
            .parent()
            .is_none_or(|parent| parent.as_os_str().is_empty())
}

fn resolve_config_path(requested: &Path) -> PathBuf {
    if is_bare_config(requested) {
        let mut filename = requested.to_path_buf();
        if filename.extension().is_none() {
            filename.set_extension("conf");
        }
        Path::new(CONFIG_DIRECTORY).join(filename)
    } else {
        requested.to_path_buf()
    }
}

fn load_grammar(requested: &Path) -> Result<Grammar, String> {
    let resolved = resolve_config_path(requested);
    if resolved.exists() || !is_bare_config(requested) {
        return config::load(&resolved);
    }

    let filename = resolved.file_name().and_then(|name| name.to_str());
    if let Some((name, source)) = BUNDLED_CONFIGS
        .iter()
        .find(|(name, _)| Some(*name) == filename)
    {
        return config::parse(source)
            .map_err(|error| format!("built-in profile `{name}`: {error}"));
    }

    config::load(&resolved)
}

fn write_names(
    output: &mut impl Write,
    count: usize,
    rng: &mut Rng,
    grammar: &Grammar,
) -> io::Result<()> {
    let mut names = HashSet::with_capacity(count);
    let mut generated = Vec::with_capacity(count);
    let max_attempts = count.saturating_mul(ATTEMPTS_PER_NAME).max(1_000);
    let mut attempts = 0;

    while names.len() < count {
        if attempts == max_attempts {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "retry limit reached before enough distinct names were produced",
            ));
        }
        attempts += 1;

        let name = grammar
            .generate_name(rng)
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
        if names.insert(name.clone()) {
            generated.push(name);
        }
    }

    for name in generated {
        writeln!(output, "{name}")?;
    }
    output.flush()
}

fn main() -> ExitCode {
    let command = match parse_args(env::args().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("fenrin: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (count, config_path, seed) = match command {
        Command::Help => {
            println!(
                "{USAGE}\n\nGenerate up to {MAX_COUNT} distinct fictional names from a phonological grammar.\nBare names first resolve under ./{CONFIG_DIRECTORY}, then to a bundled profile.\nThe .conf extension is optional; paths containing a directory are loaded unchanged.\nA `--seed` value makes generation deterministic for a given version and profile; `-s` is an alias.\nDefault profile: {DEFAULT_CONFIG}\n\nSAS mode ({}) renders 40 bits as four deterministic fictional words.\nWith no value it obtains 40 fresh bits from the operating system.\n`--sas-words` prints all 1024 codewords in index order (0 through 1023), one per line.",
                sas::VERSION
            );
            return ExitCode::SUCCESS;
        }
        Command::Sas { bytes } => {
            let bytes = match bytes {
                Some(bytes) => bytes,
                None => {
                    let mut bytes = [0_u8; sas::SAS_BYTES];
                    if let Err(error) = getrandom::fill(&mut bytes) {
                        eprintln!("fenrin: could not obtain secure randomness: {error}");
                        return ExitCode::FAILURE;
                    }
                    bytes
                }
            };
            println!("{}", sas::encode(bytes));
            return ExitCode::SUCCESS;
        }
        Command::SasWords => {
            let stdout = io::stdout();
            let mut output = BufWriter::new(stdout.lock());
            for word in sas::wordlist() {
                if let Err(error) = writeln!(output, "{word}") {
                    if error.kind() == io::ErrorKind::BrokenPipe {
                        return ExitCode::SUCCESS;
                    }
                    eprintln!("fenrin: could not write output: {error}");
                    return ExitCode::FAILURE;
                }
            }
            if let Err(error) = output.flush() {
                if error.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("fenrin: could not write output: {error}");
                    return ExitCode::FAILURE;
                }
            }
            return ExitCode::SUCCESS;
        }
        Command::Generate {
            count,
            config,
            seed,
        } => (count, config, seed),
    };
    let grammar = match load_grammar(&config_path) {
        Ok(grammar) => grammar,
        Err(message) => {
            eprintln!("fenrin: {message}");
            return ExitCode::from(2);
        }
    };

    let mut rng = Rng::new(seed.unwrap_or_else(entropy_seed));
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());

    match write_names(&mut output, count, &mut rng, &grammar) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            eprintln!("fenrin: {error}");
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("fenrin: could not write output: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_grammar() -> Grammar {
        config::parse(include_str!("../configs/fenrin.conf")).unwrap()
    }

    fn command(values: &[&str]) -> Result<Command, String> {
        parse_args(values.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn all_bundled_grammars_are_valid() {
        for &(_, source) in BUNDLED_CONFIGS {
            config::parse(source).unwrap();
        }
    }

    #[test]
    fn bundled_profiles_have_stable_bare_names() {
        for &(filename, _) in BUNDLED_CONFIGS {
            let bare = filename.strip_suffix(".conf").unwrap();
            assert_eq!(
                resolve_config_path(Path::new(bare)).file_name(),
                Some(filename.as_ref())
            );
        }
    }

    #[test]
    fn a_seed_reproduces_the_same_names() {
        let grammar = default_grammar();
        let mut left = Rng::new(42);
        let mut right = Rng::new(42);

        for _ in 0..100 {
            assert_eq!(
                grammar.generate_name(&mut left).unwrap(),
                grammar.generate_name(&mut right).unwrap()
            );
        }
    }

    #[test]
    fn japanese_rewrites_apply_contextually() {
        let grammar = config::parse(include_str!("../configs/japanese.conf")).unwrap();
        let mut rng = Rng::new(19);

        for _ in 0..1_000 {
            let name = grammar.generate_name(&mut rng).unwrap();
            for unrevised in ["SI", "TI", "TU", "HU", "ZI", "DI", "DU"] {
                assert!(!name.contains(unrevised), "{name} contains {unrevised}");
            }
        }
    }

    #[test]
    fn roman_names_end_in_latin_final_syllables() {
        let grammar = config::parse(include_str!("../configs/ancient-roman.conf")).unwrap();
        let mut rng = Rng::new(31);
        let mut closed = 0;
        let mut us_um = 0;

        for _ in 0..1_000 {
            let name = grammar.generate_name(&mut rng).unwrap();
            let last = name.chars().next_back().unwrap();
            assert!(
                matches!(last, 'A' | 'E' | 'I' | 'O' | 'U' | 'S' | 'M'),
                "{name}"
            );
            for pair in name.as_bytes().windows(2) {
                assert!(
                    pair[1] != b'H' || matches!(pair[0], b'A' | b'E' | b'I' | b'O' | b'U'),
                    "{name} has H after a consonant"
                );
            }
            if matches!(last, 'S' | 'M') {
                closed += 1;
                let before = name.chars().rev().nth(1).unwrap();
                assert!(matches!(before, 'A' | 'E' | 'I' | 'O' | 'U'), "{name}");
                if name.ends_with("US") || name.ends_with("UM") {
                    us_um += 1;
                }
            }
        }

        assert!(closed > 0);
        assert!(
            us_um * 2 > closed,
            "only {us_um} of {closed} closed endings are -US or -UM"
        );
    }

    #[test]
    fn oceanic_diphthongs_never_form_three_vowel_runs() {
        let grammar = config::parse(include_str!("../configs/oceanic.conf")).unwrap();
        let mut rng = Rng::new(23);

        for _ in 0..1_000 {
            let name = grammar.generate_name(&mut rng).unwrap();
            let mut run = 0;
            for character in name.chars() {
                if matches!(character, 'A' | 'E' | 'I' | 'O' | 'U') {
                    run += 1;
                    assert!(run <= 2, "{name}");
                } else {
                    run = 0;
                }
            }
        }
    }

    #[test]
    fn uralic_and_aurelian_harmony_never_leaks() {
        let uralic = config::parse(include_str!("../configs/uralic.conf")).unwrap();
        let aurelian = config::parse(include_str!("../configs/aurelian.conf")).unwrap();
        let mut rng = Rng::new(29);

        for _ in 0..1_000 {
            let name = uralic.generate_name(&mut rng).unwrap();
            let has_back = name
                .chars()
                .any(|character| matches!(character, 'A' | 'O' | 'U'));
            let has_front = name
                .chars()
                .any(|character| matches!(character, 'Ä' | 'Ö' | 'Ü'));
            assert!(!(has_back && has_front), "{name}");

            let name = aurelian.generate_name(&mut rng).unwrap();
            let has_clear = name
                .chars()
                .any(|character| matches!(character, 'e' | 'i' | 'ē' | 'ī'));
            let has_warm = name
                .chars()
                .any(|character| matches!(character, 'a' | 'o' | 'u' | 'ā' | 'ō' | 'ū'));
            assert!(!(has_clear && has_warm), "{name}");
        }
    }

    #[test]
    fn rewrites_can_target_a_syllable_boundary() {
        let grammar = config::parse(
            "segments = N M P A\n\
             feature type consonant = N M P\n\
             feature type vowel = A\n\
             start = NAME\n\
             rule NAME = 1: N . P A\n\
             rewrite N . P -> M . P\n",
        )
        .unwrap();

        assert_eq!(grammar.generate_name(&mut Rng::new(3)).unwrap(), "MPA");
    }

    #[test]
    fn silent_boundaries_do_not_hide_surface_clusters() {
        let grammar = config::parse(
            "segments = B P A\n\
             feature type consonant = B P\n\
             feature type vowel = A\n\
             start = NAME\n\
             rule NAME = 1: B . P A\n\
             hard max-run type consonant 1\n",
        )
        .unwrap();

        assert!(grammar.generate_name(&mut Rng::new(5)).is_err());
    }

    #[test]
    fn arguments_accept_default_and_custom_configs() {
        assert_eq!(
            command(&["12"]),
            Ok(Command::Generate {
                count: 12,
                config: PathBuf::from(DEFAULT_CONFIG),
                seed: None
            })
        );
        assert_eq!(
            command(&["--config", "other.conf", "12"]),
            Ok(Command::Generate {
                count: 12,
                config: PathBuf::from("other.conf"),
                seed: None
            })
        );
        assert_eq!(command(&["--help"]), Ok(Command::Help));
    }

    #[test]
    fn seed_arguments_parse_into_the_generate_command() {
        assert_eq!(
            command(&["--seed", "42", "5"]),
            Ok(Command::Generate {
                count: 5,
                config: PathBuf::from(DEFAULT_CONFIG),
                seed: Some(42)
            })
        );
        assert_eq!(
            command(&["--config", "japanese", "--seed", "0", "5"]),
            Ok(Command::Generate {
                count: 5,
                config: PathBuf::from("japanese"),
                seed: Some(0)
            })
        );
        assert_eq!(
            command(&["-s", "42", "5"]),
            Ok(Command::Generate {
                count: 5,
                config: PathBuf::from(DEFAULT_CONFIG),
                seed: Some(42)
            })
        );
    }

    #[test]
    fn help_is_global_and_order_independent() {
        assert_eq!(command(&["--sas-words", "--help"]), Ok(Command::Help));
        assert_eq!(command(&["--sas", "--help"]), Ok(Command::Help));
        assert_eq!(command(&["invalid", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn seed_arguments_reject_invalid_or_mixed_input() {
        assert!(command(&["--seed", "abc", "5"]).is_err());
        assert!(command(&["--seed", "-1", "5"]).is_err());
        assert!(command(&["--seed", "1", "--seed", "2", "5"]).is_err());
        assert!(command(&["--seed"]).is_err());
        assert!(command(&["--seed", "1", "--sas"]).is_err());
    }

    #[test]
    fn sas_words_arguments_parse_only_in_isolation() {
        assert_eq!(command(&["--sas-words"]), Ok(Command::SasWords));
        assert!(command(&["--sas-words", "extra"]).is_err());
        assert!(command(&["--sas-words", "--sas"]).is_err());
        assert!(command(&["5", "--sas-words"]).is_err());
        assert!(command(&["--seed", "1", "--sas-words"]).is_err());
    }

    #[test]
    fn sas_arguments_accept_random_and_deterministic_modes() {
        assert_eq!(command(&["-sas"]), Ok(Command::Sas { bytes: None }));
        assert_eq!(command(&["--sas"]), Ok(Command::Sas { bytes: None }));
        assert_eq!(
            command(&["--sas", "00010203fF"]),
            Ok(Command::Sas {
                bytes: Some([0x00, 0x01, 0x02, 0x03, 0xff])
            })
        );
    }

    #[test]
    fn sas_arguments_reject_invalid_or_mixed_input() {
        assert!(command(&["--sas", "0000"]).is_err());
        assert!(command(&["--sas", "00010203xx"]).is_err());
        assert!(command(&["--sas", "0001020304", "extra"]).is_err());
        assert!(command(&["5", "--sas"]).is_err());
        assert!(command(&["--config", "japanese", "--sas"]).is_err());
    }

    #[test]
    fn bare_config_names_resolve_inside_the_config_directory() {
        assert_eq!(
            resolve_config_path(Path::new("japanese")),
            PathBuf::from("configs/japanese.conf")
        );
        assert_eq!(
            resolve_config_path(Path::new("japanese.conf")),
            PathBuf::from("configs/japanese.conf")
        );
        assert_eq!(
            resolve_config_path(Path::new("./custom.conf")),
            PathBuf::from("./custom.conf")
        );
        assert_eq!(
            resolve_config_path(Path::new("custom/other")),
            PathBuf::from("custom/other")
        );
    }

    #[test]
    fn arguments_reject_bad_input() {
        assert!(command(&[]).is_err());
        assert!(command(&["-1"]).is_err());
        assert!(command(&["many"]).is_err());
        assert!(command(&["1", "2"]).is_err());
        assert!(command(&["--config", "a", "--config", "b", "1"]).is_err());
        assert!(command(&[&(MAX_COUNT + 1).to_string()]).is_err());
    }

    #[test]
    fn writer_emits_the_requested_number_of_unique_names() {
        let grammar = default_grammar();
        let mut output = Vec::new();
        let mut rng = Rng::new(7);

        write_names(&mut output, MAX_COUNT, &mut rng, &grammar).unwrap();

        let output = String::from_utf8(output).unwrap();
        let names: Vec<_> = output.lines().collect();
        let unique: HashSet<_> = names.iter().copied().collect();

        assert_eq!(names.len(), MAX_COUNT);
        assert_eq!(unique.len(), MAX_COUNT);
    }

    #[test]
    fn writer_stops_when_a_grammar_has_too_few_names() {
        let grammar = config::parse(
            "segments = B A\n\
             feature type consonant = B\n\
             feature type vowel = A\n\
             start = NAME\n\
             rule NAME = 1: B A\n",
        )
        .unwrap();
        let mut output = Vec::new();
        let mut rng = Rng::new(9);

        let error = write_names(&mut output, 2, &mut rng, &grammar).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(output.is_empty());
    }
}
