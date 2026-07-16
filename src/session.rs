// ABOUTME: Shared distinct-name session used by the CLI and end-to-end benchmarks.
// ABOUTME: It preserves insertion order while deduplicating and writing formatted names.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasher, BuildHasherDefault, Hasher, RandomState};
use std::io::{self, Write};

use crate::grammar::{Grammar, Rng};

const ATTEMPTS_PER_NAME: usize = 100;
const NO_COLLISION: usize = usize::MAX;

#[derive(Default)]
struct IdentityHasher(u64);

impl Hasher for IdentityHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        // This map is private and only receives prehashed u64 keys. Keep a
        // complete implementation so a future key-type mistake remains safe.
        self.0 = bytes
            .iter()
            .fold(0, |hash, byte| hash.rotate_left(8) ^ u64::from(*byte));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

type PrehashedHeads = HashMap<u64, usize, BuildHasherDefault<IdentityHasher>>;

struct OrderedName {
    value: String,
    previous_with_hash: usize,
}

struct OrderedNameSet {
    heads: PrehashedHeads,
    names: Vec<OrderedName>,
}

impl OrderedNameSet {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            heads: PrehashedHeads::with_capacity_and_hasher(capacity, Default::default()),
            names: Vec::with_capacity(capacity),
        }
    }

    fn len(&self) -> usize {
        self.names.len()
    }

    fn insert_prehashed(&mut self, value: String, hash: u64) -> bool {
        let index = self.names.len();
        match self.heads.entry(hash) {
            Entry::Vacant(entry) => {
                entry.insert(index);
                self.names.push(OrderedName {
                    value,
                    previous_with_hash: NO_COLLISION,
                });
            }
            Entry::Occupied(mut entry) => {
                let mut previous = *entry.get();
                loop {
                    let candidate = &self.names[previous];
                    if candidate.value == value {
                        return false;
                    }
                    if candidate.previous_with_hash == NO_COLLISION {
                        break;
                    }
                    previous = candidate.previous_with_hash;
                }

                let previous_with_hash = entry.insert(index);
                self.names.push(OrderedName {
                    value,
                    previous_with_hash,
                });
            }
        }
        true
    }
}

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
    let hash_builder = RandomState::new();
    let mut names = OrderedNameSet::with_capacity(count);
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
        let hash = hash_builder.hash_one(&name);
        names.insert_prehashed(name, hash);
    }

    for name in names.names {
        writeln!(output, "{}", name.value)?;
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
    fn ordered_name_set_resolves_hash_collisions_exactly() {
        let mut names = OrderedNameSet::with_capacity(4);

        assert!(names.insert_prehashed("Lio".to_owned(), 7));
        assert!(names.insert_prehashed("Mara".to_owned(), 7));
        assert!(!names.insert_prehashed("Lio".to_owned(), 7));
        assert!(names.insert_prehashed("Tov".to_owned(), 7));
        assert!(!names.insert_prehashed("Mara".to_owned(), 7));

        assert_eq!(names.len(), 3);
        assert_eq!(names.names[0].value, "Lio");
        assert_eq!(names.names[1].value, "Mara");
        assert_eq!(names.names[2].value, "Tov");
    }

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
