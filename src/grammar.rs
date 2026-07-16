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
                candidates.consider(score, &units);

                accepted += 1;
                if accepted == CANDIDATE_POOL {
                    break;
                }
            }

            if candidates.len != 0 {
                let choice = rng.index(candidates.len);
                return Ok(self.render_segments(candidates.segments(choice)));
            }
        }

        Err("grammar could not produce a well-formed name")
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
    use std::fmt::Write as _;

    use super::*;
    use crate::config;

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
