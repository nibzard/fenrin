// ABOUTME: Library crate root exposing fenrin's phonological name generator,
// ABOUTME: profile parsing, and SAS phrase encoding as an embeddable API.

pub mod config;
pub mod grammar;
pub mod sas;
pub mod session;

pub use grammar::{Grammar, Rng};

/// Bundled profile sources as `(file name, configuration source)` pairs.
pub static BUNDLED_CONFIGS: &[(&str, &str)] = &[
    ("fenrin.conf", include_str!("../configs/fenrin.conf")),
    ("japanese.conf", include_str!("../configs/japanese.conf")),
    (
        "ancient-roman.conf",
        include_str!("../configs/ancient-roman.conf"),
    ),
    ("slavic.conf", include_str!("../configs/slavic.conf")),
    ("klingon.conf", include_str!("../configs/klingon.conf")),
    ("oceanic.conf", include_str!("../configs/oceanic.conf")),
    ("uralic.conf", include_str!("../configs/uralic.conf")),
    ("caucasian.conf", include_str!("../configs/caucasian.conf")),
    ("aurelian.conf", include_str!("../configs/aurelian.conf")),
    ("obsidian.conf", include_str!("../configs/obsidian.conf")),
];
