// ABOUTME: Shared distinct-name session used by the CLI and end-to-end benchmarks.
// ABOUTME: It preserves insertion order while deduplicating and writing formatted names.

use std::collections::HashSet;
use std::io::{self, Write};

use crate::grammar::{Grammar, Rng};

const ATTEMPTS_PER_NAME: usize = 100;

/// Work completed while producing a fixed number of distinct names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistinctSessionStats {
    /// Calls made to [`Grammar::generate_name`], including duplicate results.
    pub attempts: usize,
    /// Distinct names formatted and written to the output.
    pub names: usize,
}

/// Generate `count` distinct names and write them in first-seen order.
///
/// This is the production CLI path. It is public so fixed-work benchmarks can
/// time the exact same deduplication, ordering, and formatting work without
/// including grammar parsing or terminal I/O in the measurement.
pub fn write_distinct_names(
    output: &mut impl Write,
    count: usize,
    rng: &mut Rng,
    grammar: &Grammar,
) -> io::Result<DistinctSessionStats> {
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
    output.flush()?;

    Ok(DistinctSessionStats {
        attempts,
        names: count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn session_reports_duplicates_and_preserves_first_seen_order() {
        let grammar = config::parse(
            "segments = B D A\n\
             feature type consonant = B D\n\
             feature type vowel = A\n\
             start = NAME\n\
             rule NAME = 1: B A | 1: D A\n",
        )
        .unwrap();
        let mut output = Vec::new();
        let mut rng = Rng::new(4);

        let stats = write_distinct_names(&mut output, 2, &mut rng, &grammar).unwrap();

        let text = String::from_utf8(output).unwrap();
        let names: Vec<_> = text.lines().collect();
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], names[1]);
        assert_eq!(stats.names, 2);
        assert!(stats.attempts >= stats.names);
    }

    #[test]
    fn session_fails_before_writing_a_partial_result() {
        let grammar = config::parse(
            "segments = B A\n\
             feature type consonant = B\n\
             feature type vowel = A\n\
             start = NAME\n\
             rule NAME = 1: B A\n",
        )
        .unwrap();
        let mut output = Vec::new();

        let error = write_distinct_names(&mut output, 2, &mut Rng::new(9), &grammar).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(output.is_empty());
    }
}
