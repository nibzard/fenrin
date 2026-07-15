// ABOUTME: End-to-end tests running the fenrin binary and asserting on exit
// ABOUTME: codes and output for name generation, seeding, and SAS modes.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn fenrin(args: &[&str]) -> Output {
    fenrin_in(args, Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn fenrin_in(args: &[&str], current_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fenrin"))
        .args(args)
        .current_dir(current_dir)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn help_succeeds_and_documents_every_mode() {
    for arguments in [
        &["--help"][..],
        &["--sas-words", "--help"][..],
        &["--sas", "--help"][..],
    ] {
        let output = fenrin(arguments);

        assert!(output.status.success());
        let text = stdout(&output);
        assert!(text.contains("Usage"));
        assert!(text.contains("--seed"));
        assert!(text.contains("`-s` is an alias"));
        assert!(text.contains("--sas-words"));
    }
}

#[test]
fn bundled_profiles_work_outside_the_repository() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("fenrin-cli-{}-{nonce}", std::process::id()));
    fs::create_dir(&directory).unwrap();

    let default = fenrin_in(&["--seed", "42", "3"], &directory);
    let japanese = fenrin_in(&["--config", "japanese", "--seed", "42", "3"], &directory);

    fs::remove_dir(&directory).unwrap();
    assert!(default.status.success());
    assert!(japanese.status.success());
    assert_eq!(stdout(&default).lines().count(), 3);
    assert_eq!(stdout(&japanese).lines().count(), 3);
}

#[test]
fn generation_emits_the_requested_number_of_distinct_names() {
    let output = fenrin(&["--seed", "42", "25"]);

    assert!(output.status.success());
    let text = stdout(&output);
    let names: Vec<_> = text.lines().collect();
    let unique: HashSet<_> = names.iter().copied().collect();
    assert_eq!(names.len(), 25);
    assert_eq!(unique.len(), 25);
}

#[test]
fn the_same_seed_reproduces_identical_output() {
    let first = fenrin(&["--seed", "7", "--config", "japanese", "50"]);
    let second = fenrin(&["--seed", "7", "--config", "japanese", "50"]);

    assert!(first.status.success());
    assert!(second.status.success());
    assert_eq!(stdout(&first), stdout(&second));
    assert!(!stdout(&first).is_empty());
}

#[test]
fn different_seeds_produce_different_output() {
    let first = fenrin(&["--seed", "1", "50"]);
    let second = fenrin(&["--seed", "2", "50"]);

    assert!(first.status.success());
    assert!(second.status.success());
    assert_ne!(stdout(&first), stdout(&second));
}

#[test]
fn seed_cannot_be_combined_with_sas_mode() {
    let output = fenrin(&["--seed", "1", "--sas"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
}

#[test]
fn sas_mode_renders_hex_input_deterministically() {
    let output = fenrin(&["--sas", "0123456789"]);

    assert!(output.status.success());
    assert_eq!(stdout(&output), "bafan kemap fonen ragem\n");
}

#[test]
fn sas_mode_without_input_renders_four_five_letter_words() {
    let output = fenrin(&["--sas"]);

    assert!(output.status.success());
    let text = stdout(&output);
    let words: Vec<_> = text.trim_end().split(' ').collect();
    assert_eq!(words.len(), 4);
    for word in words {
        assert_eq!(word.len(), 5);
        assert!(word.bytes().all(|byte| byte.is_ascii_lowercase()));
    }
}

#[test]
fn sas_words_dumps_the_full_wordlist_in_index_order() {
    let output = fenrin(&["--sas-words"]);

    assert!(output.status.success());
    let text = stdout(&output);
    let words: Vec<_> = text.lines().collect();
    let unique: HashSet<_> = words.iter().copied().collect();
    assert_eq!(words.len(), 1024);
    assert_eq!(unique.len(), 1024);
    assert_eq!(words.first(), Some(&"badaf"));
    assert_eq!(words.last(), Some(&"rusus"));
}

#[test]
fn invalid_arguments_exit_with_code_two() {
    for arguments in [
        vec!["many"],
        vec!["--seed", "abc", "5"],
        vec!["--seed", "1", "--seed", "2", "5"],
        vec!["--sas-words", "extra"],
        vec!["--sas-words", "--sas"],
    ] {
        let output = fenrin(&arguments);
        assert_eq!(output.status.code(), Some(2), "args: {arguments:?}");
    }
}
