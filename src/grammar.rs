use std::collections::HashMap;

const CANDIDATE_POOL: usize = 16;
const ELITE_POOL: usize = 4;
const SHAPE_ATTEMPTS: usize = 8;
const FILL_ATTEMPTS: usize = 64;
pub(crate) const MAX_UNITS: usize = 64;

#[derive(Clone, Debug)]
pub(crate) struct Segment {
    pub(crate) spelling: String,
    pub(crate) features: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unit(usize);

impl Unit {
    pub(crate) const BOUNDARY: Self = Self(usize::MAX);

    pub(crate) fn segment(index: usize) -> Self {
        debug_assert_ne!(index, usize::MAX);
        Self(index)
    }

    fn segment_index(self) -> Option<usize> {
        (self != Self::BOUNDARY).then_some(self.0)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Symbol {
    Segment(usize),
    Rule(usize),
    Boundary,
}

#[derive(Clone, Debug)]
pub(crate) struct Production {
    pub(crate) upper_bound: usize,
    pub(crate) symbols: Vec<Symbol>,
}

#[derive(Clone, Debug)]
pub(crate) struct Rule {
    pub(crate) productions: Vec<Production>,
    pub(crate) total_weight: usize,
    pub(crate) production_by_ticket: Option<Box<[u8]>>,
    pub(crate) terminal_by_ticket: Option<Box<[Unit]>>,
    pub(crate) terminal_units: Option<Box<[Unit]>>,
}

#[derive(Clone, Debug)]
pub(crate) struct Selector {
    pub(crate) members: Box<[u8]>,
}

#[derive(Clone, Debug)]
pub(crate) enum Matcher {
    Segment(usize),
    Feature(Selector),
    Boundary,
    AnySegment,
}

#[derive(Clone, Debug)]
pub(crate) struct Rewrite {
    pub(crate) pattern: Vec<Unit>,
    pub(crate) replacement: Vec<Unit>,
}

#[derive(Debug)]
pub(crate) struct PairRewriteTable {
    width: usize,
    replacements: Box<[Option<Unit>]>,
}

impl PairRewriteTable {
    pub(crate) fn compile(rewrites: &[Rewrite], segment_count: usize) -> Option<Self> {
        if rewrites.is_empty()
            || rewrites.iter().any(|rewrite| {
                rewrite.pattern.len() != 2
                    || rewrite.replacement.len() != 2
                    || rewrite.pattern[1] != rewrite.replacement[1]
            })
        {
            return None;
        }

        let contexts: Vec<_> = rewrites.iter().map(|rewrite| rewrite.pattern[1]).collect();
        let first_units: Vec<_> = rewrites
            .iter()
            .flat_map(|rewrite| [rewrite.pattern[0], rewrite.replacement[0]])
            .collect();
        if contexts.iter().any(|context| first_units.contains(context)) {
            return None;
        }

        let width = segment_count + 1;
        let mut replacements = vec![None; width * width];
        for first_key in 0..width {
            let original = Self::unit(first_key, segment_count);
            for context_key in 0..width {
                let context = Self::unit(context_key, segment_count);
                let mut first = original;
                for rewrite in rewrites {
                    if rewrite.pattern == [first, context] {
                        first = rewrite.replacement[0];
                    }
                }
                if first != original {
                    replacements[first_key * width + context_key] = Some(first);
                }
            }
        }

        Some(Self {
            width,
            replacements: replacements.into_boxed_slice(),
        })
    }

    fn unit(key: usize, segment_count: usize) -> Unit {
        if key == segment_count {
            Unit::BOUNDARY
        } else {
            Unit::segment(key)
        }
    }

    fn key(&self, unit: Unit) -> usize {
        unit.segment_index().unwrap_or(self.width - 1)
    }

    fn apply(&self, units: &mut [Unit]) {
        for index in 0..units.len().saturating_sub(1) {
            let first = self.key(units[index]);
            let context = self.key(units[index + 1]);
            if let Some(replacement) = self.replacements[first * self.width + context] {
                units[index] = replacement;
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum HardConstraint {
    Max { selector: Selector, limit: usize },
    NoRepeat { selector: Selector },
    MaxRun { selector: Selector, limit: usize },
    Forbid { pattern: Vec<Matcher> },
}

#[derive(Clone, Debug)]
pub(crate) enum SoftConstraint {
    Repeat {
        selector: Selector,
        weight: u64,
    },
    Excess {
        selector: Selector,
        free: usize,
        weight: u64,
    },
    Sequence {
        pattern: Vec<Matcher>,
        weight: u64,
    },
}

#[derive(Clone, Copy, Debug)]
struct EliteEntry {
    score: u64,
    slot: u8,
    segment_count: u8,
}

impl EliteEntry {
    const EMPTY: Self = Self {
        score: 0,
        slot: 0,
        segment_count: 0,
    };
}

#[derive(Debug)]
struct ElitePool {
    entries: [EliteEntry; ELITE_POOL],
    segments: [[u8; MAX_UNITS]; ELITE_POOL],
    len: usize,
}

impl ElitePool {
    fn new() -> Self {
        Self {
            entries: [EliteEntry::EMPTY; ELITE_POOL],
            segments: [[0; MAX_UNITS]; ELITE_POOL],
            len: 0,
        }
    }

    fn cutoff(&self) -> Option<u64> {
        (self.len == ELITE_POOL).then(|| self.entries[ELITE_POOL - 1].score)
    }

    fn consider(&mut self, score: u64, units: &[Unit]) {
        let insertion = self.entries[..self.len].partition_point(|entry| entry.score <= score);
        if insertion == ELITE_POOL {
            return;
        }

        let slot = if self.len < ELITE_POOL {
            self.len
        } else {
            usize::from(self.entries[ELITE_POOL - 1].slot)
        };
        let mut segment_count = 0;
        for segment in units.iter().filter_map(|unit| unit.segment_index()) {
            self.segments[slot][segment_count] =
                u8::try_from(segment).expect("segment limit fits in u8");
            segment_count += 1;
        }
        let entry = EliteEntry {
            score,
            slot: u8::try_from(slot).expect("elite pool size fits in u8"),
            segment_count: u8::try_from(segment_count).expect("unit limit fits in u8"),
        };

        if self.len < ELITE_POOL {
            self.entries.copy_within(insertion..self.len, insertion + 1);
            self.len += 1;
        } else {
            self.entries
                .copy_within(insertion..ELITE_POOL - 1, insertion + 1);
        }
        self.entries[insertion] = entry;
    }

    fn segments(&self, index: usize) -> &[u8] {
        let entry = self.entries[index];
        &self.segments[usize::from(entry.slot)][..usize::from(entry.segment_count)]
    }
}

#[derive(Debug)]
pub struct Grammar {
    pub(crate) segments: Vec<Segment>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) start: usize,
    pub(crate) rewrites: Vec<Rewrite>,
    pub(crate) pair_rewrites: Option<PairRewriteTable>,
    pub(crate) hard_constraints: Vec<HardConstraint>,
    pub(crate) soft_constraints: Vec<SoftConstraint>,
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    pub(crate) fn index(&mut self, len: usize) -> usize {
        debug_assert!(len > 0);
        ((u128::from(self.next_u64()) * len as u128) >> 64) as usize
    }
}

impl Selector {
    fn matches(&self, unit: Unit, grammar: &Grammar) -> bool {
        let Some(segment) = unit.segment_index() else {
            return false;
        };
        let _ = grammar;
        self.members[segment] != 0
    }
}

impl Matcher {
    fn matches(&self, unit: Unit, grammar: &Grammar) -> bool {
        match self {
            Self::Segment(expected) => unit == Unit::segment(*expected),
            Self::Feature(selector) => selector.matches(unit, grammar),
            Self::Boundary => unit == Unit::BOUNDARY,
            Self::AnySegment => unit != Unit::BOUNDARY,
        }
    }
}

impl Grammar {
    pub fn generate_name(&self, rng: &mut Rng) -> Result<String, &'static str> {
        for _ in 0..SHAPE_ATTEMPTS {
            let start_production = self.pick_production(self.start, rng);
            if let Some(name) = self.generate_shape(start_production, rng) {
                return Ok(name);
            }
        }

        Err("grammar could not produce a well-formed name")
    }

    fn generate_shape(&self, start_production: usize, rng: &mut Rng) -> Option<String> {
        let mut candidates = ElitePool::new();
        let mut accepted = 0;
        let mut units = Vec::new();

        for _ in 0..FILL_ATTEMPTS {
            self.generate_underlying(start_production, &mut units, rng);
            if !self.apply_rewrites(&mut units) || !self.is_well_formed(&units) {
                continue;
            }

            if !units.iter().any(|unit| unit.segment_index().is_some()) {
                continue;
            }

            let cutoff = candidates.cutoff();
            let score = self.score(&units, cutoff);
            // Soft scores are sums of nonnegative penalties. The first valid
            // zero is therefore globally optimal and retains the raw weighted
            // identity distribution conditional on validity and score zero.
            if score == 0 {
                return Some(self.render_units(&units));
            }
            candidates.consider(score, &units);

            accepted += 1;
            if accepted == CANDIDATE_POOL {
                break;
            }
        }

        if candidates.len == 0 {
            return None;
        }
        let choice = rng.index(candidates.len);
        Some(self.render_segments(candidates.segments(choice)))
    }

    fn generate_underlying(&self, start_production: usize, units: &mut Vec<Unit>, rng: &mut Rng) {
        units.clear();
        self.expand_production(self.start, start_production, units, rng);
    }

    fn pick_production(&self, rule: usize, rng: &mut Rng) -> usize {
        let rule = &self.rules[rule];
        let ticket = rng.index(rule.total_weight);
        if let Some(productions) = &rule.production_by_ticket {
            return usize::from(productions[ticket]);
        }
        rule.productions
            .partition_point(|production| production.upper_bound <= ticket)
    }

    fn expand_rule(&self, rule: usize, output: &mut Vec<Unit>, rng: &mut Rng) {
        if let Some(terminals) = &self.rules[rule].terminal_by_ticket {
            let ticket = rng.index(self.rules[rule].total_weight);
            output.push(terminals[ticket]);
            return;
        }

        let production = self.pick_production(rule, rng);
        if let Some(terminals) = &self.rules[rule].terminal_units {
            output.push(terminals[production]);
            return;
        }
        self.expand_production(rule, production, output, rng);
    }

    fn expand_production(
        &self,
        rule: usize,
        production: usize,
        output: &mut Vec<Unit>,
        rng: &mut Rng,
    ) {
        let production = &self.rules[rule].productions[production];

        for symbol in &production.symbols {
            match *symbol {
                Symbol::Segment(segment) => output.push(Unit::segment(segment)),
                Symbol::Boundary => output.push(Unit::BOUNDARY),
                Symbol::Rule(nested) => self.expand_rule(nested, output, rng),
            }
        }
    }

    fn apply_rewrites(&self, units: &mut Vec<Unit>) -> bool {
        if let Some(rewrites) = &self.pair_rewrites {
            rewrites.apply(units);
            return true;
        }

        for rewrite in &self.rewrites {
            if rewrite.pattern.is_empty() {
                return false;
            }

            if rewrite.pattern.len() == rewrite.replacement.len() {
                let mut index = 0;
                while index < units.len() {
                    if units[index..].starts_with(&rewrite.pattern) {
                        units[index..index + rewrite.pattern.len()]
                            .copy_from_slice(&rewrite.replacement);
                        index += rewrite.pattern.len();
                    } else {
                        index += 1;
                    }
                }
                continue;
            }

            let mut rewritten = Vec::with_capacity(units.len());
            let mut index = 0;

            while index < units.len() {
                if units[index..].starts_with(&rewrite.pattern) {
                    rewritten.extend_from_slice(&rewrite.replacement);
                    index += rewrite.pattern.len();
                } else {
                    rewritten.push(units[index]);
                    index += 1;
                }
                if rewritten.len() > MAX_UNITS {
                    return false;
                }
            }
            *units = rewritten;
        }

        true
    }

    fn is_well_formed(&self, units: &[Unit]) -> bool {
        self.hard_constraints
            .iter()
            .all(|constraint| match constraint {
                HardConstraint::Max { selector, limit } => {
                    count_matches(selector, units, self) <= *limit
                }
                HardConstraint::NoRepeat { selector } => {
                    let mut previous = None;
                    units
                        .iter()
                        .copied()
                        .filter(|unit| selector.matches(*unit, self))
                        .all(|unit| {
                            let Some(current) = unit.segment_index() else {
                                return true;
                            };
                            let differs = previous != Some(current);
                            previous = Some(current);
                            differs
                        })
                }
                HardConstraint::MaxRun { selector, limit } => {
                    let mut run = 0;
                    let mut longest = 0;
                    for unit in units {
                        if *unit == Unit::BOUNDARY {
                            continue;
                        }
                        if selector.matches(*unit, self) {
                            run += 1;
                            longest = longest.max(run);
                        } else {
                            run = 0;
                        }
                    }
                    longest <= *limit
                }
                HardConstraint::Forbid { pattern } => !contains_pattern(pattern, units, self),
            })
    }

    fn score(&self, units: &[Unit], cutoff: Option<u64>) -> u64 {
        if cutoff == Some(0) {
            return 0;
        }

        let mut score = 0_u64;
        for constraint in &self.soft_constraints {
            let penalty = match constraint {
                SoftConstraint::Repeat { selector, weight } => {
                    repeated_matches(selector, units, self).saturating_mul(*weight)
                }
                SoftConstraint::Excess {
                    selector,
                    free,
                    weight,
                } => (count_matches(selector, units, self).saturating_sub(*free) as u64)
                    .saturating_mul(*weight),
                SoftConstraint::Sequence { pattern, weight } => {
                    pattern_count(pattern, units, self).saturating_mul(*weight)
                }
            };
            score = score.saturating_add(penalty);
            if cutoff.is_some_and(|cutoff| score >= cutoff) {
                break;
            }
        }
        score
    }

    fn render_segments(&self, segments: &[u8]) -> String {
        let capacity = segments
            .iter()
            .map(|segment| self.segments[usize::from(*segment)].spelling.len())
            .sum();
        let mut output = String::with_capacity(capacity);

        for segment in segments {
            output.push_str(&self.segments[usize::from(*segment)].spelling);
        }

        output
    }

    fn render_units(&self, units: &[Unit]) -> String {
        let capacity = units
            .iter()
            .filter_map(|unit| unit.segment_index())
            .map(|segment| self.segments[segment].spelling.len())
            .sum();
        let mut output = String::with_capacity(capacity);

        for segment in units.iter().filter_map(|unit| unit.segment_index()) {
            output.push_str(&self.segments[segment].spelling);
        }

        output
    }
}

fn count_matches(selector: &Selector, units: &[Unit], grammar: &Grammar) -> usize {
    units
        .iter()
        .filter(|unit| selector.matches(**unit, grammar))
        .count()
}

fn repeated_matches(selector: &Selector, units: &[Unit], grammar: &Grammar) -> u64 {
    let mut previous = None;
    let mut repeats = 0_u64;

    for unit in units
        .iter()
        .copied()
        .filter(|unit| selector.matches(*unit, grammar))
    {
        let Some(current) = unit.segment_index() else {
            continue;
        };
        if previous == Some(current) {
            repeats += 1;
        }
        previous = Some(current);
    }

    repeats
}

fn contains_pattern(pattern: &[Matcher], units: &[Unit], grammar: &Grammar) -> bool {
    pattern_count(pattern, units, grammar) > 0
}

fn pattern_count(pattern: &[Matcher], units: &[Unit], grammar: &Grammar) -> u64 {
    if pattern.is_empty() {
        return 0;
    }

    if pattern
        .iter()
        .any(|matcher| matches!(matcher, Matcher::Boundary))
    {
        return raw_pattern_count(pattern, units, grammar);
    }

    let surface: Vec<_> = units
        .iter()
        .copied()
        .filter(|unit| *unit != Unit::BOUNDARY)
        .collect();
    raw_pattern_count(pattern, &surface, grammar)
}

fn raw_pattern_count(pattern: &[Matcher], units: &[Unit], grammar: &Grammar) -> u64 {
    if pattern.len() > units.len() {
        return 0;
    }

    (0..=units.len() - pattern.len())
        .filter(|index| pattern_matches(pattern, units, *index, grammar))
        .count() as u64
}

fn pattern_matches(pattern: &[Matcher], units: &[Unit], start: usize, grammar: &Grammar) -> bool {
    start + pattern.len() <= units.len()
        && pattern
            .iter()
            .zip(&units[start..])
            .all(|(matcher, unit)| matcher.matches(*unit, grammar))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::fmt::Write as _;

    use super::*;
    use crate::{BUNDLED_CONFIGS, config};

    #[derive(Debug)]
    struct ShapeDiagnostics {
        accepted: usize,
        score_counts: BTreeMap<u64, usize>,
        selected_score_counts: BTreeMap<u64, usize>,
        saturation_accepted_index: Option<usize>,
        saturation_fill_index: Option<usize>,
    }

    fn diagnose_shape(
        grammar: &Grammar,
        start_production: usize,
        rng: &mut Rng,
    ) -> ShapeDiagnostics {
        let mut candidates = ElitePool::new();
        let mut accepted = 0;
        let mut units = Vec::new();
        let mut score_counts = BTreeMap::new();
        let mut saturation_accepted_index = None;
        let mut saturation_fill_index = None;

        for fill in 1..=FILL_ATTEMPTS {
            grammar.generate_underlying(start_production, &mut units, rng);
            if !grammar.apply_rewrites(&mut units) || !grammar.is_well_formed(&units) {
                continue;
            }
            if !units.iter().any(|unit| unit.segment_index().is_some()) {
                continue;
            }

            // Diagnostics deliberately compute the complete score rather than
            // using the elite cutoff. This pass is never part of timed code.
            let score = grammar.score(&units, None);
            *score_counts.entry(score).or_insert(0) += 1;
            candidates.consider(score, &units);
            accepted += 1;

            if saturation_accepted_index.is_none() && candidates.cutoff() == Some(0) {
                saturation_accepted_index = Some(accepted);
                saturation_fill_index = Some(fill);
            }
            if accepted == CANDIDATE_POOL {
                break;
            }
        }

        let mut selected_score_counts = BTreeMap::new();
        for entry in &candidates.entries[..candidates.len] {
            *selected_score_counts.entry(entry.score).or_insert(0) += 1;
        }
        ShapeDiagnostics {
            accepted,
            score_counts,
            selected_score_counts,
            saturation_accepted_index,
            saturation_fill_index,
        }
    }

    #[derive(Clone, Copy)]
    enum TestPolicy {
        FirstZero,
        SaturatedElite,
    }

    struct TestSelection {
        name: String,
        start_production: usize,
        score: u64,
    }

    fn generate_name_with_policy(
        grammar: &Grammar,
        rng: &mut Rng,
        policy: TestPolicy,
    ) -> Result<TestSelection, &'static str> {
        for _ in 0..SHAPE_ATTEMPTS {
            let start_production = grammar.pick_production(grammar.start, rng);
            let mut candidates = ElitePool::new();
            let mut accepted = 0;
            let mut units = Vec::new();

            for _ in 0..FILL_ATTEMPTS {
                grammar.generate_underlying(start_production, &mut units, rng);
                if !grammar.apply_rewrites(&mut units) || !grammar.is_well_formed(&units) {
                    continue;
                }
                if !units.iter().any(|unit| unit.segment_index().is_some()) {
                    continue;
                }

                let score = grammar.score(&units, candidates.cutoff());
                if matches!(policy, TestPolicy::FirstZero) && score == 0 {
                    return Ok(TestSelection {
                        name: grammar.render_units(&units),
                        start_production,
                        score,
                    });
                }
                candidates.consider(score, &units);
                accepted += 1;
                if accepted == CANDIDATE_POOL
                    || matches!(policy, TestPolicy::SaturatedElite)
                        && candidates.cutoff() == Some(0)
                {
                    break;
                }
            }

            if candidates.len != 0 {
                let choice = rng.index(candidates.len);
                return Ok(TestSelection {
                    name: grammar.render_segments(candidates.segments(choice)),
                    start_production,
                    score: candidates.entries[choice].score,
                });
            }
        }
        Err("grammar could not produce a well-formed name")
    }

    fn generate_name_with_saturation(
        grammar: &Grammar,
        rng: &mut Rng,
    ) -> Result<String, &'static str> {
        generate_name_with_policy(grammar, rng, TestPolicy::SaturatedElite)
            .map(|selection| selection.name)
    }

    fn pool_snapshot(pool: &ElitePool) -> Vec<(u64, Vec<u8>)> {
        (0..pool.len)
            .map(|index| (pool.entries[index].score, pool.segments(index).to_vec()))
            .collect()
    }

    #[derive(Debug)]
    struct Quality {
        sampled: usize,
        unique: usize,
        collision_pairs: u64,
        bytes: usize,
        shape_counts: Vec<usize>,
        score_counts: BTreeMap<u64, usize>,
    }

    fn quality(
        grammar: &Grammar,
        seeds: &[u64],
        count_per_seed: usize,
        policy: TestPolicy,
    ) -> (Quality, Vec<Quality>) {
        let mut frequencies = HashMap::<String, u64>::new();
        let mut bytes = 0;
        let mut shape_counts = vec![0; grammar.rules[grammar.start].productions.len()];
        let mut score_counts = BTreeMap::new();
        let mut batches = Vec::with_capacity(seeds.len());
        for &seed in seeds {
            let mut rng = Rng::new(seed);
            let mut batch_frequencies = HashMap::<String, u64>::new();
            let mut batch_bytes = 0;
            let mut batch_shape_counts = vec![0; grammar.rules[grammar.start].productions.len()];
            let mut batch_score_counts = BTreeMap::new();
            for _ in 0..count_per_seed {
                let selection = generate_name_with_policy(grammar, &mut rng, policy).unwrap();
                bytes += selection.name.len();
                batch_bytes += selection.name.len();
                shape_counts[selection.start_production] += 1;
                batch_shape_counts[selection.start_production] += 1;
                *score_counts.entry(selection.score).or_insert(0) += 1;
                *batch_score_counts.entry(selection.score).or_insert(0) += 1;
                *frequencies.entry(selection.name.clone()).or_insert(0) += 1;
                *batch_frequencies.entry(selection.name).or_insert(0) += 1;
            }
            let batch_collision_pairs = batch_frequencies
                .values()
                .map(|frequency| frequency * (frequency - 1) / 2)
                .sum();
            batches.push(Quality {
                sampled: count_per_seed,
                unique: batch_frequencies.len(),
                collision_pairs: batch_collision_pairs,
                bytes: batch_bytes,
                shape_counts: batch_shape_counts,
                score_counts: batch_score_counts,
            });
        }
        let collision_pairs = frequencies
            .values()
            .map(|frequency| frequency * (frequency - 1) / 2)
            .sum();
        (
            Quality {
                sampled: seeds.len() * count_per_seed,
                unique: frequencies.len(),
                collision_pairs,
                bytes,
                shape_counts,
                score_counts,
            },
            batches,
        )
    }

    fn paired_t_interval(
        left: &[Quality],
        right: &[Quality],
        critical_value: f64,
        metric: impl Fn(&Quality) -> f64,
    ) -> (f64, f64) {
        assert_eq!(left.len(), right.len());
        assert!(left.len() > 1);
        let differences: Vec<_> = left
            .iter()
            .zip(right)
            .map(|(left, right)| metric(left) - metric(right))
            .collect();
        let mean = differences.iter().sum::<f64>() / differences.len() as f64;
        let variance = differences
            .iter()
            .map(|difference| (difference - mean).powi(2))
            .sum::<f64>()
            / (differences.len() - 1) as f64;
        let half_width = critical_value * (variance / differences.len() as f64).sqrt();
        (mean, half_width)
    }

    #[test]
    fn dense_terminal_table_preserves_weighted_tickets_and_rng_advance() {
        let grammar = config::parse(
            "segments = A B C\n\
             start = NAME\n\
             rule NAME = 1: @T\n\
             rule T = 2: A | 1: B | 3: C\n",
        )
        .unwrap();
        let terminal_rule = 1;
        let expected_by_ticket = [
            Unit::segment(0),
            Unit::segment(0),
            Unit::segment(1),
            Unit::segment(2),
            Unit::segment(2),
            Unit::segment(2),
        ];
        assert_eq!(
            grammar.rules[terminal_rule].terminal_by_ticket.as_deref(),
            Some(expected_by_ticket.as_slice())
        );

        for seed in 0..256 {
            let mut reference_rng = Rng::new(seed);
            let expected = expected_by_ticket[reference_rng.index(expected_by_ticket.len())];

            let mut actual_rng = Rng::new(seed);
            let mut output = Vec::new();
            grammar.expand_rule(terminal_rule, &mut output, &mut actual_rng);

            assert_eq!(output, [expected]);
            assert_eq!(actual_rng.0, reference_rng.0);
        }
    }

    #[test]
    fn elite_pool_keeps_stable_top_four_in_packed_slots() {
        let mut pool = ElitePool::new();
        pool.consider(2, &[Unit::segment(0), Unit::BOUNDARY, Unit::segment(255)]);
        pool.consider(1, &[Unit::segment(1)]);
        pool.consider(2, &[Unit::segment(2)]);
        pool.consider(0, &[Unit::segment(3)]);

        assert_eq!(pool.cutoff(), Some(2));
        assert_eq!(pool.segments(0), [3]);
        assert_eq!(pool.segments(1), [1]);
        assert_eq!(pool.segments(2), [0, 255]);
        assert_eq!(pool.segments(3), [2]);

        pool.consider(2, &[Unit::segment(4)]);
        assert_eq!(pool.segments(3), [2], "later score ties stay outside");

        pool.consider(1, &[Unit::segment(5)]);
        assert_eq!(pool.segments(0), [3]);
        assert_eq!(pool.segments(1), [1]);
        assert_eq!(pool.segments(2), [5]);
        assert_eq!(pool.segments(3), [0, 255]);
    }

    #[test]
    fn four_zero_early_stop_is_exhaustively_equivalent_for_every_accepted_count() {
        // Invalid fills only affect how many accepted candidates reach the
        // tournament. Exhausting every binary score sequence for N=0..16 thus
        // covers the finite 64-fill cap as well as the 16-accepted cap.
        let mut selected_zero_votes = 0_u64;
        for accepted in 0..=CANDIDATE_POOL {
            for zero_mask in 0_u32..1_u32 << accepted {
                let mut full = ElitePool::new();
                let mut stopped = ElitePool::new();
                let mut saturated = false;

                for index in 0..accepted {
                    let score = if zero_mask & (1 << index) == 0 {
                        1 + (index % 3) as u64
                    } else {
                        0
                    };
                    let units = [Unit::segment(index)];
                    full.consider(score, &units);
                    if !saturated {
                        stopped.consider(score, &units);
                        saturated = stopped.cutoff() == Some(0);
                    }
                }

                assert_eq!(pool_snapshot(&stopped), pool_snapshot(&full));
                if accepted == CANDIDATE_POOL {
                    selected_zero_votes += full.entries[..full.len]
                        .iter()
                        .filter(|entry| entry.score == 0)
                        .count() as u64;
                }
            }
        }

        // For sixteen iid equiprobable zero/positive scores, the exact chance
        // that a uniformly selected elite entry has score zero is
        // 261292 / (2^16 * 4). The exhaustive tournament above independently
        // recovers that ideal-distribution numerator.
        assert_eq!(selected_zero_votes, 261_292);
    }

    #[test]
    fn first_zero_policy_exhaustively_preserves_weighted_zero_ticket_law() {
        // Tickets 0, 1, and 2 are zero-score names: one spelling has one
        // ticket and the other has two. Ticket 3 is positive. Exhaust every
        // length-eight ticket stream and select its first zero-score ticket.
        // The selected zero spellings must retain their exact 1:2 weight ratio;
        // suffix tickets cannot bias the identity of the first success.
        const STREAM_LEN: usize = 8;
        const STREAMS: usize = 4_usize.pow(STREAM_LEN as u32);
        let mut selected = [0_u64; 2];
        let mut streams_without_zero = 0_u64;

        for mut encoded in 0..STREAMS {
            let mut choice = None;
            for _ in 0..STREAM_LEN {
                let ticket = encoded % 4;
                encoded /= 4;
                match ticket {
                    0 => {
                        choice = Some(0);
                        break;
                    }
                    1 | 2 => {
                        choice = Some(1);
                        break;
                    }
                    3 => {}
                    _ => unreachable!(),
                }
            }
            if let Some(choice) = choice {
                selected[choice] += 1;
            } else {
                streams_without_zero += 1;
            }
        }

        assert_eq!(selected[1], selected[0] * 2);
        assert_eq!(
            selected.iter().sum::<u64>() + streams_without_zero,
            STREAMS as u64
        );
        assert_eq!(streams_without_zero, 1);
    }

    #[test]
    fn first_zero_runtime_matches_explicit_bounded_policy() {
        let grammar = config::parse(
            "segments = A B C\n\
             feature awkward yes = A C\n\
             start = NAME\n\
             rule NAME = 1: @T\n\
             rule T = 1: A | 2: B | 1: C\n\
             soft excess awkward yes 0 1\n",
        )
        .unwrap();

        for seed in 0..4096 {
            let mut runtime_rng = Rng::new(seed);
            let mut reference_rng = Rng::new(seed);
            let runtime = grammar.generate_name(&mut runtime_rng);
            let reference =
                generate_name_with_policy(&grammar, &mut reference_rng, TestPolicy::FirstZero)
                    .map(|selection| selection.name);
            assert_eq!(runtime, reference, "seed {seed}");
            assert_eq!(runtime_rng.0, reference_rng.0, "seed {seed}");
        }
    }

    #[test]
    fn no_zero_path_retains_the_packed_elite_fallback() {
        let grammar = config::parse(
            "segments = A B C\n\
             feature awkward yes = A B C\n\
             start = NAME\n\
             rule NAME = 1: @T\n\
             rule T = 1: A | 2: B | 1: C\n\
             soft excess awkward yes 0 1\n",
        )
        .unwrap();

        for seed in 0..4096 {
            let mut runtime_rng = Rng::new(seed);
            let mut saturation_rng = Rng::new(seed);
            assert_eq!(
                grammar.generate_name(&mut runtime_rng),
                generate_name_with_saturation(&grammar, &mut saturation_rng),
                "seed {seed}"
            );
            assert_eq!(runtime_rng.0, saturation_rng.0, "seed {seed}");
        }
    }

    #[test]
    #[ignore = "diagnostic: cargo test --lib bundled_start_score_and_saturation_diagnostics -- --ignored --nocapture"]
    fn bundled_start_score_and_saturation_diagnostics() {
        const SAMPLES_PER_START: usize = 256;

        for (profile_index, &(profile, source)) in BUNDLED_CONFIGS.iter().enumerate() {
            let grammar = config::parse(source).unwrap();
            let start_count = grammar.rules[grammar.start].productions.len();
            for start_production in 0..start_count {
                let mut successes = 0;
                let mut total_accepted = 0;
                let mut total_elite = 0;
                let mut saturated_at = Vec::new();
                let mut saturated_fill = Vec::new();
                let mut score_counts = BTreeMap::<u64, usize>::new();
                let mut selected_score_counts = BTreeMap::<u64, usize>::new();

                for sample in 0..SAMPLES_PER_START {
                    let seed = ((profile_index as u64) << 48)
                        ^ ((start_production as u64) << 32)
                        ^ sample as u64;
                    let diagnostics =
                        diagnose_shape(&grammar, start_production, &mut Rng::new(seed));
                    successes += usize::from(diagnostics.accepted != 0);
                    total_accepted += diagnostics.accepted;
                    total_elite += diagnostics.accepted.min(ELITE_POOL);
                    if let Some(index) = diagnostics.saturation_accepted_index {
                        saturated_at.push(index);
                        saturated_fill.push(
                            diagnostics
                                .saturation_fill_index
                                .expect("accepted and fill saturation occur together"),
                        );
                    }
                    for (score, count) in diagnostics.score_counts {
                        *score_counts.entry(score).or_insert(0) += count;
                    }
                    for (score, count) in diagnostics.selected_score_counts {
                        *selected_score_counts.entry(score).or_insert(0) += count;
                    }
                }

                saturated_at.sort_unstable();
                saturated_fill.sort_unstable();
                let saturation_rate = saturated_at.len() as f64 / SAMPLES_PER_START as f64;
                let mean_index = (!saturated_at.is_empty())
                    .then(|| saturated_at.iter().sum::<usize>() as f64 / saturated_at.len() as f64);
                let percentile = |values: &[usize], percent: usize| {
                    (!values.is_empty()).then(|| values[(values.len() - 1) * percent / 100])
                };
                eprintln!(
                    "{profile} start={start_production} success={successes}/{SAMPLES_PER_START} saturation={saturation_rate:.3} mean-index={mean_index:?} p50-index={:?} p90-index={:?} p50-fill={:?} p90-fill={:?} raw-score-pmf={score_counts:?} selected-score-pmf={selected_score_counts:?}",
                    percentile(&saturated_at, 50),
                    percentile(&saturated_at, 90),
                    percentile(&saturated_fill, 50),
                    percentile(&saturated_fill, 90),
                );

                assert!(successes > 0, "{profile} start {start_production}");
                assert_eq!(
                    score_counts.values().sum::<usize>(),
                    total_accepted,
                    "{profile} start {start_production} score PMF lost accepted fills"
                );
                assert_eq!(selected_score_counts.values().sum::<usize>(), total_elite);
                if matches!(profile, "fenrin.conf" | "japanese.conf") {
                    assert_eq!(
                        saturated_at.len(),
                        SAMPLES_PER_START,
                        "{profile} start {start_production} did not saturate"
                    );
                }
                assert!(
                    saturated_at
                        .iter()
                        .all(|index| (ELITE_POOL..=CANDIDATE_POOL).contains(index))
                );
            }
        }
    }

    #[test]
    fn unsamplable_shapes_retain_all_finite_attempt_limits() {
        let grammar = config::parse(
            "segments = A\n\
             feature rejected yes = A\n\
             start = NAME\n\
             rule NAME = 1: A\n\
             hard max rejected yes 0\n",
        )
        .unwrap();

        for seed in 0..64 {
            let mut optimized_rng = Rng::new(seed);
            let mut legacy_rng = Rng::new(seed);
            assert_eq!(
                grammar.generate_name(&mut optimized_rng),
                generate_name_with_saturation(&grammar, &mut legacy_rng)
            );
            assert_eq!(optimized_rng.0, legacy_rng.0);
        }
    }

    #[test]
    #[ignore = "campaign check: cargo test --lib first_zero_meets_preregistered_all_profile_quality_bounds -- --ignored --nocapture"]
    fn first_zero_meets_preregistered_all_profile_quality_bounds() {
        const SEEDS: [u64; 8] = [
            0x243f_6a88_85a3_08d3,
            0x1319_8a2e_0370_7344,
            0xa409_3822_299f_31d0,
            0x082e_fa98_ec4e_6c89,
            0x4528_21e6_38d0_1377,
            0xbe54_66cf_34e9_0c6c,
            0xc0ac_29b7_c97c_50dd,
            0x3f84_d5b5_b547_0917,
        ];
        const COUNT_PER_SEED: usize = 80_000;
        const MAX_DUPLICATE_RATE_DELTA: f64 = 0.005;
        const MAX_COLLISION_BITS_DELTA: f64 = 0.15;
        const MAX_MEAN_BYTES_DELTA: f64 = 0.05;
        const MAX_SHAPE_SHARE_DELTA: f64 = 0.01;
        const MAX_ZERO_SCORE_SHARE_REGRESSION: f64 = 0.001;
        const T_TWO_SIDED_95_DF7: f64 = 2.364_624_252;
        const T_ONE_SIDED_95_DF7: f64 = 1.894_578_605;

        for &(profile, source) in BUNDLED_CONFIGS {
            let grammar = config::parse(source).unwrap();
            let (first_zero, first_zero_batches) =
                quality(&grammar, &SEEDS, COUNT_PER_SEED, TestPolicy::FirstZero);
            let (saturation, saturation_batches) =
                quality(&grammar, &SEEDS, COUNT_PER_SEED, TestPolicy::SaturatedElite);

            let duplicate_rate = |quality: &Quality| {
                (quality.sampled - quality.unique) as f64 / quality.sampled as f64
            };
            let collision_bits = |quality: &Quality| {
                let pairs = quality.sampled as f64 * (quality.sampled - 1) as f64 / 2.0;
                -(quality.collision_pairs as f64 / pairs).log2()
            };
            let mean_bytes = |quality: &Quality| quality.bytes as f64 / quality.sampled as f64;
            let zero_score_share = |quality: &Quality| {
                quality.score_counts.get(&0).copied().unwrap_or(0) as f64 / quality.sampled as f64
            };

            eprintln!(
                "{profile}: first-zero={first_zero:?}, saturation={saturation:?} duplicate={:.6}/{:.6} collision-bits={:.6}/{:.6} mean-bytes={:.6}/{:.6} zero-score={:.6}/{:.6}",
                duplicate_rate(&first_zero),
                duplicate_rate(&saturation),
                collision_bits(&first_zero),
                collision_bits(&saturation),
                mean_bytes(&first_zero),
                mean_bytes(&saturation),
                zero_score_share(&first_zero),
                zero_score_share(&saturation),
            );

            assert!(
                (duplicate_rate(&first_zero) - duplicate_rate(&saturation)).abs()
                    < MAX_DUPLICATE_RATE_DELTA,
                "{profile}: first-zero={first_zero:?}, saturation={saturation:?}"
            );
            assert!(
                (collision_bits(&first_zero) - collision_bits(&saturation)).abs()
                    < MAX_COLLISION_BITS_DELTA,
                "{profile}: first-zero={first_zero:?}, saturation={saturation:?}"
            );
            assert!(
                (mean_bytes(&first_zero) - mean_bytes(&saturation)).abs() < MAX_MEAN_BYTES_DELTA,
                "{profile}: first-zero={first_zero:?}, saturation={saturation:?}"
            );
            for (start, (&first_zero_count, &saturation_count)) in first_zero
                .shape_counts
                .iter()
                .zip(&saturation.shape_counts)
                .enumerate()
            {
                let first_zero_share = first_zero_count as f64 / first_zero.sampled as f64;
                let saturation_share = saturation_count as f64 / saturation.sampled as f64;
                assert!(
                    (first_zero_share - saturation_share).abs() < MAX_SHAPE_SHARE_DELTA,
                    "{profile} start {start}: first-zero={first_zero:?}, saturation={saturation:?}"
                );
            }
            assert!(
                zero_score_share(&first_zero) + MAX_ZERO_SCORE_SHARE_REGRESSION
                    >= zero_score_share(&saturation),
                "{profile}: first-zero={first_zero:?}, saturation={saturation:?}"
            );

            let require_equivalence = |metric: &str, mean: f64, half_width: f64, margin: f64| {
                eprintln!(
                    "{profile}: paired-{metric}-delta={mean:.6} two-sided-95-half={half_width:.6} equivalence-margin={margin:.6}"
                );
                assert!(
                    mean.abs() + half_width < margin,
                    "{profile} {metric}: delta={mean}, half={half_width}, margin={margin}"
                );
            };

            let (mean, half_width) = paired_t_interval(
                &first_zero_batches,
                &saturation_batches,
                T_TWO_SIDED_95_DF7,
                duplicate_rate,
            );
            require_equivalence("duplicate-rate", mean, half_width, MAX_DUPLICATE_RATE_DELTA);

            let (mean, half_width) = paired_t_interval(
                &first_zero_batches,
                &saturation_batches,
                T_TWO_SIDED_95_DF7,
                collision_bits,
            );
            require_equivalence("collision-bits", mean, half_width, MAX_COLLISION_BITS_DELTA);

            let (mean, half_width) = paired_t_interval(
                &first_zero_batches,
                &saturation_batches,
                T_TWO_SIDED_95_DF7,
                mean_bytes,
            );
            require_equivalence("mean-bytes", mean, half_width, MAX_MEAN_BYTES_DELTA);

            for start in 0..first_zero.shape_counts.len() {
                let shape_share =
                    |quality: &Quality| quality.shape_counts[start] as f64 / quality.sampled as f64;
                let (mean, half_width) = paired_t_interval(
                    &first_zero_batches,
                    &saturation_batches,
                    T_TWO_SIDED_95_DF7,
                    shape_share,
                );
                require_equivalence(
                    &format!("shape-{start}-share"),
                    mean,
                    half_width,
                    MAX_SHAPE_SHARE_DELTA,
                );
            }

            let (mean_regression, one_sided_half_width) = paired_t_interval(
                &saturation_batches,
                &first_zero_batches,
                T_ONE_SIDED_95_DF7,
                zero_score_share,
            );
            eprintln!(
                "{profile}: paired-zero-score-regression={mean_regression:.6} one-sided-95-half={one_sided_half_width:.6} noninferiority-margin={MAX_ZERO_SCORE_SHARE_REGRESSION:.6}"
            );
            assert!(
                mean_regression + one_sided_half_width < MAX_ZERO_SCORE_SHARE_REGRESSION,
                "{profile} zero-score: regression={mean_regression}, half={one_sided_half_width}"
            );
        }
    }

    #[test]
    fn packed_rendering_handles_boundaries_multibyte_spelling_and_segment_255() {
        let mut source = String::from("segments =");
        for index in 0..=255 {
            write!(source, " S{index}").unwrap();
        }
        source.push_str(
            "\nspell S0 = Å\n\
             spell S255 = Ö\n\
             start = NAME\n\
             rule NAME = 1: S0 . S255\n",
        );

        let grammar = config::parse(&source).unwrap();
        assert_eq!(grammar.generate_name(&mut Rng::new(7)).unwrap(), "ÅÖ");
    }
}
