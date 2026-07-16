// ABOUTME: Browser-facing WebAssembly adapter for Fenrin's native Rust library.
// ABOUTME: It keeps parsed grammar state in Wasm and crosses into JavaScript in batches.

use fenrin::{BUNDLED_CONFIGS, Grammar, Rng, config};
use wasm_bindgen::prelude::*;

const MAX_BATCH_SIZE: u32 = 4_096;
const MAX_BENCHMARK_SIZE: u32 = 250_000;

#[wasm_bindgen]
pub struct NameGenerator {
    grammar: Grammar,
    rng: Rng,
}

impl NameGenerator {
    fn from_profile(profile: &str, seed: u32) -> Result<Self, String> {
        let requested = profile
            .trim()
            .trim_end_matches(".conf")
            .to_ascii_lowercase();
        let filename = format!("{requested}.conf");
        let (_, source) = BUNDLED_CONFIGS
            .iter()
            .find(|(name, _)| *name == filename)
            .ok_or_else(|| format!("unknown bundled profile `{profile}`"))?;
        let grammar = config::parse(source)
            .map_err(|error| format!("could not parse bundled profile `{requested}`: {error}"))?;

        Ok(Self {
            grammar,
            rng: Rng::new(u64::from(seed)),
        })
    }

    fn generate_text(&mut self, count: u32) -> Result<String, String> {
        if !(1..=MAX_BATCH_SIZE).contains(&count) {
            return Err(format!("batch size must be between 1 and {MAX_BATCH_SIZE}"));
        }

        let mut names = String::with_capacity(count as usize * 12);
        for index in 0..count {
            if index > 0 {
                names.push('\n');
            }
            let name = self
                .grammar
                .generate_name(&mut self.rng)
                .map_err(str::to_owned)?;
            names.push_str(&name);
        }
        Ok(names)
    }

    fn run_benchmark(&mut self, count: u32) -> Result<(), String> {
        if !(1..=MAX_BENCHMARK_SIZE).contains(&count) {
            return Err(format!(
                "benchmark size must be between 1 and {MAX_BENCHMARK_SIZE}"
            ));
        }

        for _ in 0..count {
            self.grammar
                .generate_name(&mut self.rng)
                .map_err(str::to_owned)?;
        }
        Ok(())
    }
}

#[wasm_bindgen]
impl NameGenerator {
    #[wasm_bindgen(constructor)]
    pub fn new(profile: &str, seed: u32) -> Result<NameGenerator, JsValue> {
        Self::from_profile(profile, seed).map_err(|message| JsValue::from_str(&message))
    }

    /// Generate a newline-delimited batch with one Wasm-to-JavaScript crossing.
    pub fn generate_batch(&mut self, count: u32) -> Result<String, JsValue> {
        self.generate_text(count)
            .map_err(|message| JsValue::from_str(&message))
    }

    /// Exercise the engine without transferring generated strings to JavaScript.
    pub fn benchmark(&mut self, count: u32) -> Result<(), JsValue> {
        self.run_benchmark(count)
            .map_err(|message| JsValue::from_str(&message))
    }
}

#[wasm_bindgen]
pub fn profile_names() -> String {
    BUNDLED_CONFIGS
        .iter()
        .map(|(name, _)| name.trim_end_matches(".conf"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_batches_are_reproducible() {
        let mut left = NameGenerator::from_profile("fenrin", 42).unwrap();
        let mut right = NameGenerator::from_profile("fenrin.conf", 42).unwrap();

        assert_eq!(left.generate_text(64), right.generate_text(64));
    }

    #[test]
    fn every_bundled_profile_generates_a_complete_batch() {
        for profile in profile_names().lines() {
            let mut generator = NameGenerator::from_profile(profile, 7).unwrap();
            assert_eq!(generator.generate_text(16).unwrap().lines().count(), 16);
        }
    }

    #[test]
    fn adapter_bounds_expensive_calls() {
        let mut generator = NameGenerator::from_profile("fenrin", 9).unwrap();

        assert!(generator.generate_text(0).is_err());
        assert!(generator.generate_text(MAX_BATCH_SIZE + 1).is_err());
        assert!(generator.run_benchmark(MAX_BENCHMARK_SIZE + 1).is_err());
    }
}
