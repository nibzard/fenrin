use std::collections::HashMap;

const CANDIDATE_POOL: usize = 16;
const ELITE_POOL: usize = 4;
const SHAPE_ATTEMPTS: usize = 8;
const FILL_ATTEMPTS: usize = 64;
const MAX_EXPANSION_DEPTH: usize = 128;
pub(crate) const MAX_UNITS: usize = 64;

#[derive(Clone, Debug)]
pub(crate) struct Segment {
    pub(crate) spelling: String,
    pub(crate) features: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    Segment(usize),
    Boundary,
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

#[derive(Debug)]
pub struct Grammar {
    pub(crate) segments: Vec<Segment>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) start: usize,
    pub(crate) rewrites: Vec<Rewrite>,
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
        let Unit::Segment(segment) = unit else {
            return false;
        };
        let _ = grammar;
        self.members[segment] != 0
    }
}

impl Matcher {
    fn matches(&self, unit: Unit, grammar: &Grammar) -> bool {
        match self {
            Self::Segment(expected) => unit == Unit::Segment(*expected),
            Self::Feature(selector) => selector.matches(unit, grammar),
            Self::Boundary => unit == Unit::Boundary,
            Self::AnySegment => matches!(unit, Unit::Segment(_)),
        }
    }
}

impl Grammar {
    pub fn generate_name(&self, rng: &mut Rng) -> Result<String, &'static str> {
        for _ in 0..SHAPE_ATTEMPTS {
            let start_production = self.pick_production(self.start, rng);
            let mut candidates = Vec::with_capacity(CANDIDATE_POOL);
            let mut units = Vec::new();

            for _ in 0..FILL_ATTEMPTS {
                if !self.generate_underlying(start_production, &mut units, rng) {
                    continue;
                }
                if !self.apply_rewrites(&mut units) || !self.is_well_formed(&units) {
                    continue;
                }

                let spelling = self.render(&units);
                if spelling.is_empty() {
                    continue;
                }

                candidates.push((self.score(&units), spelling));
                if candidates.len() == CANDIDATE_POOL {
                    break;
                }
            }

            if !candidates.is_empty() {
                candidates.sort_by_key(|candidate| candidate.0);
                let elite = candidates.len().min(ELITE_POOL);
                let choice = rng.index(elite);
                return Ok(candidates.swap_remove(choice).1);
            }
        }

        Err("grammar could not produce a well-formed name")
    }

    fn generate_underlying(
        &self,
        start_production: usize,
        units: &mut Vec<Unit>,
        rng: &mut Rng,
    ) -> bool {
        units.clear();
        self.expand_production(self.start, start_production, 0, units, rng)
    }

    fn pick_production(&self, rule: usize, rng: &mut Rng) -> usize {
        let rule = &self.rules[rule];
        let ticket = rng.index(rule.total_weight);
        rule.productions
            .partition_point(|production| production.upper_bound <= ticket)
    }

    fn expand_rule(
        &self,
        rule: usize,
        depth: usize,
        output: &mut Vec<Unit>,
        rng: &mut Rng,
    ) -> bool {
        if depth >= MAX_EXPANSION_DEPTH {
            return false;
        }

        let production = self.pick_production(rule, rng);
        self.expand_production(rule, production, depth, output, rng)
    }

    fn expand_production(
        &self,
        rule: usize,
        production: usize,
        depth: usize,
        output: &mut Vec<Unit>,
        rng: &mut Rng,
    ) -> bool {
        let production = &self.rules[rule].productions[production];

        for symbol in &production.symbols {
            match *symbol {
                Symbol::Segment(segment) => output.push(Unit::Segment(segment)),
                Symbol::Boundary => output.push(Unit::Boundary),
                Symbol::Rule(nested) => {
                    if !self.expand_rule(nested, depth + 1, output, rng) {
                        return false;
                    }
                }
            }

            if output.len() > MAX_UNITS {
                return false;
            }
        }

        true
    }

    fn apply_rewrites(&self, units: &mut Vec<Unit>) -> bool {
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
                            let Unit::Segment(current) = unit else {
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
                        if *unit == Unit::Boundary {
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

    fn score(&self, units: &[Unit]) -> u64 {
        self.soft_constraints
            .iter()
            .fold(0_u64, |score, constraint| {
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
                score.saturating_add(penalty)
            })
    }

    fn render(&self, units: &[Unit]) -> String {
        let capacity = units
            .iter()
            .filter_map(|unit| match unit {
                Unit::Segment(segment) => Some(self.segments[*segment].spelling.len()),
                Unit::Boundary => None,
            })
            .sum();
        let mut output = String::with_capacity(capacity);

        for unit in units {
            if let Unit::Segment(segment) = unit {
                output.push_str(&self.segments[*segment].spelling);
            }
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
        let Unit::Segment(current) = unit else {
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
        .filter(|unit| *unit != Unit::Boundary)
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
