// ABOUTME: Integration tests exercising fenrin's public library API:
// ABOUTME: profile parsing, seeded name generation, and SAS encoding.

use std::collections::HashSet;

use fenrin::{Grammar, Rng, config, sas};

fn default_grammar() -> Grammar {
    let (_, source) = fenrin::BUNDLED_CONFIGS
        .iter()
        .find(|(name, _)| *name == "fenrin.conf")
        .unwrap();
    config::parse(source).unwrap()
}

#[test]
fn bundled_profiles_parse_and_generate_names() {
    assert_eq!(fenrin::BUNDLED_CONFIGS.len(), 10);

    for &(name, source) in fenrin::BUNDLED_CONFIGS {
        let grammar =
            config::parse(source).unwrap_or_else(|error| panic!("{name} failed: {error}"));
        let generated = grammar.generate_name(&mut Rng::new(1)).unwrap();
        assert!(!generated.is_empty(), "{name} produced an empty name");
    }
}

#[test]
fn seeded_generation_is_reproducible_across_grammar_instances() {
    let left_grammar = default_grammar();
    let right_grammar = default_grammar();
    let mut left = Rng::new(1234);
    let mut right = Rng::new(1234);

    for _ in 0..100 {
        assert_eq!(
            left_grammar.generate_name(&mut left).unwrap(),
            right_grammar.generate_name(&mut right).unwrap()
        );
    }
}

#[test]
fn config_load_reads_a_profile_from_disk() {
    let grammar = config::load(std::path::Path::new("configs/japanese.conf")).unwrap();
    grammar.generate_name(&mut Rng::new(7)).unwrap();
}

#[test]
fn sas_encoding_matches_the_frozen_version_one_format() {
    assert_eq!(sas::VERSION, "fenrin-sas-v1");
    assert_eq!(sas::SAS_BITS, 40);
    assert_eq!(sas::SAS_BYTES, 5);
    assert_eq!(sas::encode([0; sas::SAS_BYTES]), "badaf badaf badaf badaf");
    assert_eq!(
        sas::encode([0x01, 0x23, 0x45, 0x67, 0x89]),
        "bafan kemap fonen ragem"
    );
}

#[test]
fn sas_wordlist_enumerates_every_codeword_in_index_order() {
    let words = sas::wordlist();

    assert_eq!(words.len(), 1024);
    assert_eq!(words.first().map(String::as_str), Some("badaf"));
    assert_eq!(words.last().map(String::as_str), Some("rusus"));

    let unique: HashSet<_> = words.iter().collect();
    assert_eq!(unique.len(), 1024);

    let zero_phrase = sas::encode([0; sas::SAS_BYTES]);
    assert!(zero_phrase.split(' ').all(|word| words[0] == word));
}
