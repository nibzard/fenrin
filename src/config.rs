use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::grammar::{
    Grammar, HardConstraint, MAX_UNITS, Matcher, PairRewriteTable, Production, Rewrite, Rule,
    Segment, Selector, SoftConstraint, Symbol, Unit,
};

const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_LINE_BYTES: usize = 4 * 1024;
const MAX_SEGMENTS: usize = 256;
const MAX_RULES: usize = 128;
const MAX_ALTERNATIVES: usize = 64;
const MAX_SYMBOLS: usize = 64;
const MAX_REWRITES: usize = 64;
const MAX_CONSTRAINTS: usize = 128;
const MAX_WEIGHT: usize = 1_000_000;
const MAX_DENSE_RULE_WEIGHT: usize = 256;

type RuleNames = HashMap<String, usize>;
type RawRule = (usize, String, String);

#[derive(Clone, Debug)]
struct RawDirective {
    line: usize,
    kind: RawKind,
}

#[derive(Clone, Debug)]
enum RawKind {
    Segments(String),
    Feature {
        key: String,
        value: String,
        members: String,
    },
    Spell {
        segment: String,
        spelling: String,
    },
    Start(String),
    Rule {
        name: String,
        body: String,
    },
    Rewrite {
        pattern: String,
        replacement: String,
    },
    Hard(String),
    Soft(String),
}

pub fn load(path: &Path) -> Result<Grammar, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    parse(&source).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn parse(source: &str) -> Result<Grammar, String> {
    if source.len() > MAX_CONFIG_BYTES {
        return Err(format!(
            "config exceeds the {MAX_CONFIG_BYTES}-byte size limit"
        ));
    }

    let directives = scan_directives(source)?;
    let (mut segments, segment_ids) = build_segments(&directives)?;
    apply_features_and_spellings(&directives, &mut segments, &segment_ids)?;

    let (rule_names, raw_rules) = collect_rules(&directives)?;
    let rules = compile_rules(&raw_rules, &rule_names, &segment_ids)?;
    let start = compile_start(&directives, &rule_names)?;

    validate_rule_graph(&rules, &raw_rules, start)?;

    let rewrites = compile_rewrites(&directives, &segment_ids)?;
    let pair_rewrites = PairRewriteTable::compile(&rewrites, segments.len());
    let rewrites_preserve_length = rewrites
        .iter()
        .all(|rewrite| rewrite.pattern.len() == rewrite.replacement.len());
    let hard_constraints = compile_hard_constraints(&directives, &segment_ids, &segments)?;
    let soft_constraints = compile_soft_constraints(&directives, &segment_ids, &segments)?;

    Ok(Grammar {
        segments,
        rules,
        start,
        rewrites,
        pair_rewrites,
        rewrites_preserve_length,
        hard_constraints,
        soft_constraints,
    })
}

fn scan_directives(source: &str) -> Result<Vec<RawDirective>, String> {
    let mut directives = Vec::new();
    let mut constraint_count = 0;

    for (index, raw_line) in source.lines().enumerate() {
        let line = index + 1;
        if raw_line.len() > MAX_LINE_BYTES {
            return Err(format!(
                "line {line}: line exceeds the {MAX_LINE_BYTES}-byte limit"
            ));
        }

        let content = raw_line
            .split_once('#')
            .map_or(raw_line, |(before, _)| before)
            .trim();
        if content.is_empty() {
            continue;
        }

        let kind = if let Some(rest) = content.strip_prefix("rewrite ") {
            let rest = rest.trim();
            let (pattern, replacement) = rest
                .split_once("->")
                .ok_or_else(|| format!("line {line}: expected `rewrite ... -> ...`"))?;
            RawKind::Rewrite {
                pattern: pattern.trim().to_owned(),
                replacement: replacement.trim().to_owned(),
            }
        } else if let Some(rest) = content.strip_prefix("hard ") {
            RawKind::Hard(rest.trim().to_owned())
        } else if let Some(rest) = content.strip_prefix("soft ") {
            RawKind::Soft(rest.trim().to_owned())
        } else {
            let (left, right) = content
                .split_once('=')
                .ok_or_else(|| format!("line {line}: expected a declaration or constraint"))?;
            let left = left.trim();
            let right = right.trim();

            if left == "segments" {
                RawKind::Segments(right.to_owned())
            } else if left == "start" {
                RawKind::Start(right.to_owned())
            } else if let Some(rest) = left.strip_prefix("feature ") {
                let fields: Vec<_> = rest.split_whitespace().collect();
                if fields.len() != 2 {
                    return Err(format!(
                        "line {line}: expected `feature <key> <value> = <segments>`"
                    ));
                }
                RawKind::Feature {
                    key: fields[0].to_owned(),
                    value: fields[1].to_owned(),
                    members: right.to_owned(),
                }
            } else if let Some(rest) = left.strip_prefix("spell ") {
                let fields: Vec<_> = rest.split_whitespace().collect();
                if fields.len() != 1 {
                    return Err(format!("line {line}: expected `spell <segment> = <text>`"));
                }
                RawKind::Spell {
                    segment: fields[0].to_owned(),
                    spelling: right.to_owned(),
                }
            } else if let Some(rest) = left.strip_prefix("rule ") {
                let fields: Vec<_> = rest.split_whitespace().collect();
                if fields.len() != 1 {
                    return Err(format!(
                        "line {line}: expected `rule <name> = <weighted alternatives>`"
                    ));
                }
                RawKind::Rule {
                    name: fields[0].to_owned(),
                    body: right.to_owned(),
                }
            } else {
                return Err(format!("line {line}: unknown declaration `{left}`"));
            }
        };

        if matches!(kind, RawKind::Hard(_) | RawKind::Soft(_)) {
            constraint_count += 1;
            if constraint_count > MAX_CONSTRAINTS {
                return Err(format!(
                    "line {line}: constraint count exceeds the {MAX_CONSTRAINTS}-constraint limit"
                ));
            }
        }

        directives.push(RawDirective { line, kind });
    }

    Ok(directives)
}

fn build_segments(
    directives: &[RawDirective],
) -> Result<(Vec<Segment>, HashMap<String, usize>), String> {
    let declarations: Vec<_> = directives
        .iter()
        .filter_map(|directive| match &directive.kind {
            RawKind::Segments(value) => Some((directive.line, value)),
            _ => None,
        })
        .collect();

    if declarations.is_empty() {
        return Err("missing `segments` declaration".to_owned());
    }
    if declarations.len() > 1 {
        return Err(format!(
            "line {}: duplicate `segments` declaration",
            declarations[1].0
        ));
    }

    let (line, value) = declarations[0];
    let ids: Vec<_> = value.split_whitespace().collect();
    if ids.is_empty() {
        return Err(format!("line {line}: `segments` must not be empty"));
    }
    if ids.len() > MAX_SEGMENTS {
        return Err(format!(
            "line {line}: `segments` exceeds the {MAX_SEGMENTS}-segment limit"
        ));
    }

    let mut segment_ids = HashMap::with_capacity(ids.len());
    let mut segments = Vec::with_capacity(ids.len());
    for id in ids {
        validate_identifier(id, line, "segment")?;
        if segment_ids.contains_key(id) {
            return Err(format!("line {line}: duplicate segment `{id}`"));
        }
        let index = segments.len();
        segment_ids.insert(id.to_owned(), index);
        segments.push(Segment {
            spelling: id.to_owned(),
            features: HashMap::new(),
        });
    }

    Ok((segments, segment_ids))
}

fn apply_features_and_spellings(
    directives: &[RawDirective],
    segments: &mut [Segment],
    segment_ids: &HashMap<String, usize>,
) -> Result<(), String> {
    let mut spelled = HashSet::new();

    for directive in directives {
        match &directive.kind {
            RawKind::Feature {
                key,
                value,
                members,
            } => {
                validate_feature_atom(key, directive.line)?;
                validate_feature_atom(value, directive.line)?;
                let members: Vec<_> = members.split_whitespace().collect();
                if members.is_empty() {
                    return Err(format!(
                        "line {}: feature class must not be empty",
                        directive.line
                    ));
                }
                let mut local = HashSet::new();
                for member in members {
                    if !local.insert(member) {
                        return Err(format!(
                            "line {}: duplicate segment `{member}` in feature class",
                            directive.line
                        ));
                    }
                    let segment = resolve_segment(member, directive.line, segment_ids)?;
                    if let Some(previous) = segments[segment]
                        .features
                        .insert(key.clone(), value.clone())
                    {
                        return Err(format!(
                            "line {}: segment `{member}` already has `{key}={previous}`",
                            directive.line
                        ));
                    }
                }
            }
            RawKind::Spell { segment, spelling } => {
                let index = resolve_segment(segment, directive.line, segment_ids)?;
                if !spelled.insert(index) {
                    return Err(format!(
                        "line {}: duplicate spelling for segment `{segment}`",
                        directive.line
                    ));
                }
                if spelling.is_empty()
                    || spelling
                        .chars()
                        .any(|character| character.is_whitespace() || character.is_control())
                {
                    return Err(format!(
                        "line {}: spelling must be one printable token",
                        directive.line
                    ));
                }
                segments[index].spelling.clone_from(spelling);
            }
            _ => {}
        }
    }

    Ok(())
}

fn collect_rules(directives: &[RawDirective]) -> Result<(RuleNames, Vec<RawRule>), String> {
    let mut names = HashMap::new();
    let mut raw_rules = Vec::new();

    for directive in directives {
        if let RawKind::Rule { name, body } = &directive.kind {
            validate_identifier(name, directive.line, "rule")?;
            if names.contains_key(name) {
                return Err(format!("line {}: duplicate rule `{name}`", directive.line));
            }
            if raw_rules.len() == MAX_RULES {
                return Err(format!(
                    "line {}: rule count exceeds the {MAX_RULES}-rule limit",
                    directive.line
                ));
            }
            names.insert(name.clone(), raw_rules.len());
            raw_rules.push((directive.line, name.clone(), body.clone()));
        }
    }

    if raw_rules.is_empty() {
        return Err("at least one `rule` declaration is required".to_owned());
    }
    Ok((names, raw_rules))
}

fn compile_rules(
    raw_rules: &[RawRule],
    rule_names: &RuleNames,
    segment_ids: &HashMap<String, usize>,
) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::with_capacity(raw_rules.len());

    for (line, _, body) in raw_rules {
        let alternatives: Vec<_> = body.split('|').collect();
        if alternatives.is_empty() || alternatives.len() > MAX_ALTERNATIVES {
            return Err(format!(
                "line {line}: rule must have 1 to {MAX_ALTERNATIVES} alternatives"
            ));
        }

        let mut productions = Vec::with_capacity(alternatives.len());
        let mut total_weight = 0_usize;
        for alternative in alternatives {
            let (weight, symbols) = alternative.split_once(':').ok_or_else(|| {
                format!("line {line}: each alternative needs a `<weight>:` prefix")
            })?;
            let weight = parse_bounded_usize(weight.trim(), *line, "weight", 1, MAX_WEIGHT)?;
            total_weight = total_weight
                .checked_add(weight)
                .ok_or_else(|| format!("line {line}: total rule weight overflows"))?;

            let tokens: Vec<_> = symbols.split_whitespace().collect();
            if tokens.is_empty() {
                return Err(format!("line {line}: use `_` for an empty production"));
            }
            if tokens.len() > MAX_SYMBOLS {
                return Err(format!(
                    "line {line}: production exceeds the {MAX_SYMBOLS}-symbol limit"
                ));
            }
            if tokens.contains(&"_") && tokens.as_slice() != ["_"] {
                return Err(format!(
                    "line {line}: `_` must be the only symbol in an empty production"
                ));
            }

            let mut compiled = Vec::with_capacity(tokens.len());
            if tokens.as_slice() != ["_"] {
                for token in tokens {
                    let symbol = if token == "." {
                        Symbol::Boundary
                    } else if let Some(name) = token.strip_prefix('@') {
                        let rule = rule_names.get(name).copied().ok_or_else(|| {
                            format!("line {line}: unknown rule reference `@{name}`")
                        })?;
                        Symbol::Rule(rule)
                    } else {
                        Symbol::Segment(resolve_segment(token, *line, segment_ids)?)
                    };
                    compiled.push(symbol);
                }
            }

            productions.push(Production {
                upper_bound: total_weight,
                symbols: compiled,
            });
        }

        let production_by_ticket = (total_weight <= MAX_DENSE_RULE_WEIGHT).then(|| {
            let mut tickets = Vec::with_capacity(total_weight);
            for (production, compiled) in productions.iter().enumerate() {
                tickets.resize(
                    compiled.upper_bound,
                    u8::try_from(production).expect("production limit fits in u8"),
                );
            }
            tickets.into_boxed_slice()
        });
        let terminal_units: Option<Vec<_>> = productions
            .iter()
            .map(|production| match production.symbols.as_slice() {
                [Symbol::Segment(segment)] => Some(Unit::segment(*segment)),
                [Symbol::Boundary] => Some(Unit::BOUNDARY),
                _ => None,
            })
            .collect();
        let terminal_by_ticket = production_by_ticket
            .as_deref()
            .zip(terminal_units.as_deref())
            .map(|(production_by_ticket, terminal_units)| {
                production_by_ticket
                    .iter()
                    .map(|production| terminal_units[usize::from(*production)])
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            });

        rules.push(Rule {
            productions,
            total_weight,
            production_by_ticket,
            terminal_by_ticket,
            terminal_units: terminal_units.map(Vec::into_boxed_slice),
        });
    }

    Ok(rules)
}

fn compile_start(directives: &[RawDirective], rule_names: &RuleNames) -> Result<usize, String> {
    let starts: Vec<_> = directives
        .iter()
        .filter_map(|directive| match &directive.kind {
            RawKind::Start(name) => Some((directive.line, name)),
            _ => None,
        })
        .collect();

    if starts.is_empty() {
        return Err("missing `start` declaration".to_owned());
    }
    if starts.len() > 1 {
        return Err(format!(
            "line {}: duplicate `start` declaration",
            starts[1].0
        ));
    }
    let (line, name) = starts[0];
    if name.split_whitespace().count() != 1 {
        return Err(format!("line {line}: `start` must name exactly one rule"));
    }
    rule_names
        .get(name)
        .copied()
        .ok_or_else(|| format!("line {line}: unknown start rule `{name}`"))
}

fn validate_rule_graph(rules: &[Rule], raw_rules: &[RawRule], start: usize) -> Result<(), String> {
    let mut state = vec![0_u8; rules.len()];
    let mut stack = Vec::new();
    for rule in 0..rules.len() {
        detect_cycle(rule, rules, raw_rules, &mut state, &mut stack)?;
    }

    let mut reachable = vec![false; rules.len()];
    mark_reachable(start, rules, &mut reachable);
    if let Some(unreachable) = reachable.iter().position(|reached| !reached) {
        return Err(format!(
            "line {}: rule `{}` is unreachable from the start rule",
            raw_rules[unreachable].0, raw_rules[unreachable].1
        ));
    }

    let mut memo = vec![None; rules.len()];
    let (_, maximum, max_segments) = expansion_bounds(start, rules, &mut memo)?;
    if maximum > MAX_UNITS {
        return Err(format!(
            "start rule can expand to {maximum} units; maximum is {MAX_UNITS}"
        ));
    }
    if max_segments == 0 {
        return Err("start rule cannot emit any segments".to_owned());
    }

    Ok(())
}

fn detect_cycle(
    rule: usize,
    rules: &[Rule],
    raw_rules: &[RawRule],
    state: &mut [u8],
    stack: &mut Vec<usize>,
) -> Result<(), String> {
    match state[rule] {
        2 => return Ok(()),
        1 => {
            let begin = stack.iter().position(|entry| *entry == rule).unwrap_or(0);
            let mut names: Vec<_> = stack[begin..]
                .iter()
                .map(|entry| raw_rules[*entry].1.as_str())
                .collect();
            names.push(raw_rules[rule].1.as_str());
            return Err(format!(
                "line {}: recursive rule cycle: {}",
                raw_rules[rule].0,
                names.join(" -> ")
            ));
        }
        _ => {}
    }

    state[rule] = 1;
    stack.push(rule);
    for nested in referenced_rules(&rules[rule]) {
        detect_cycle(nested, rules, raw_rules, state, stack)?;
    }
    stack.pop();
    state[rule] = 2;
    Ok(())
}

fn referenced_rules(rule: &Rule) -> impl Iterator<Item = usize> + '_ {
    rule.productions.iter().flat_map(|production| {
        production.symbols.iter().filter_map(|symbol| match symbol {
            Symbol::Rule(rule) => Some(*rule),
            _ => None,
        })
    })
}

fn mark_reachable(rule: usize, rules: &[Rule], reachable: &mut [bool]) {
    if reachable[rule] {
        return;
    }
    reachable[rule] = true;
    for nested in referenced_rules(&rules[rule]) {
        mark_reachable(nested, rules, reachable);
    }
}

fn expansion_bounds(
    rule: usize,
    rules: &[Rule],
    memo: &mut [Option<(usize, usize, usize)>],
) -> Result<(usize, usize, usize), String> {
    if let Some(bounds) = memo[rule] {
        return Ok(bounds);
    }

    let mut minimum = usize::MAX;
    let mut maximum = 0_usize;
    let mut max_segments = 0_usize;
    for production in &rules[rule].productions {
        let mut production_min = 0_usize;
        let mut production_max = 0_usize;
        let mut production_segments = 0_usize;
        for symbol in &production.symbols {
            let (symbol_min, symbol_max, symbol_segments) = match symbol {
                Symbol::Segment(_) => (1, 1, 1),
                Symbol::Boundary => (1, 1, 0),
                Symbol::Rule(nested) => expansion_bounds(*nested, rules, memo)?,
            };
            production_min = production_min
                .checked_add(symbol_min)
                .ok_or_else(|| "static expansion size overflows the platform integer".to_owned())?;
            production_max = production_max
                .checked_add(symbol_max)
                .ok_or_else(|| "static expansion size overflows the platform integer".to_owned())?;
            production_segments = production_segments
                .checked_add(symbol_segments)
                .ok_or_else(|| "static segment count overflows the platform integer".to_owned())?;
        }
        minimum = minimum.min(production_min);
        maximum = maximum.max(production_max);
        max_segments = max_segments.max(production_segments);
    }

    let bounds = (minimum, maximum, max_segments);
    memo[rule] = Some(bounds);
    Ok(bounds)
}

fn compile_rewrites(
    directives: &[RawDirective],
    segment_ids: &HashMap<String, usize>,
) -> Result<Vec<Rewrite>, String> {
    let mut rewrites = Vec::new();

    for directive in directives {
        let RawKind::Rewrite {
            pattern,
            replacement,
        } = &directive.kind
        else {
            continue;
        };

        if rewrites.len() == MAX_REWRITES {
            return Err(format!(
                "line {}: rewrite count exceeds the {MAX_REWRITES}-rewrite limit",
                directive.line
            ));
        }

        let pattern_tokens: Vec<_> = pattern.split_whitespace().collect();
        if pattern_tokens.is_empty() || pattern_tokens == ["_"] {
            return Err(format!(
                "line {}: rewrite pattern must not be empty",
                directive.line
            ));
        }
        if pattern_tokens.len() > MAX_SYMBOLS {
            return Err(format!(
                "line {}: rewrite pattern exceeds the {MAX_SYMBOLS}-symbol limit",
                directive.line
            ));
        }
        let mut compiled_pattern = Vec::with_capacity(pattern_tokens.len());
        for token in pattern_tokens {
            compiled_pattern.push(if token == "." {
                Unit::BOUNDARY
            } else {
                Unit::segment(resolve_segment(token, directive.line, segment_ids)?)
            });
        }

        let replacement_tokens: Vec<_> = replacement.split_whitespace().collect();
        if replacement_tokens.is_empty() {
            return Err(format!(
                "line {}: use `_` for an empty rewrite replacement",
                directive.line
            ));
        }
        if replacement_tokens.contains(&"_") && replacement_tokens.as_slice() != ["_"] {
            return Err(format!(
                "line {}: `_` must be the entire rewrite replacement",
                directive.line
            ));
        }
        if replacement_tokens.len() > MAX_SYMBOLS {
            return Err(format!(
                "line {}: rewrite replacement exceeds the {MAX_SYMBOLS}-symbol limit",
                directive.line
            ));
        }

        let mut compiled_replacement = Vec::new();
        if replacement_tokens.as_slice() != ["_"] {
            for token in replacement_tokens {
                compiled_replacement.push(if token == "." {
                    Unit::BOUNDARY
                } else {
                    Unit::segment(resolve_segment(token, directive.line, segment_ids)?)
                });
            }
        }

        rewrites.push(Rewrite {
            pattern: compiled_pattern,
            replacement: compiled_replacement,
        });
    }

    Ok(rewrites)
}

fn compile_hard_constraints(
    directives: &[RawDirective],
    segment_ids: &HashMap<String, usize>,
    segments: &[Segment],
) -> Result<Vec<HardConstraint>, String> {
    let mut constraints = Vec::new();

    for directive in directives {
        let RawKind::Hard(body) = &directive.kind else {
            continue;
        };
        let fields: Vec<_> = body.split_whitespace().collect();
        let constraint = match fields.first().copied() {
            Some("max") if fields.len() == 4 => HardConstraint::Max {
                selector: compile_selector(fields[1], fields[2], directive.line, segments)?,
                limit: parse_usize(fields[3], directive.line, "limit")?,
            },
            Some("no-repeat") if fields.len() == 3 => HardConstraint::NoRepeat {
                selector: compile_selector(fields[1], fields[2], directive.line, segments)?,
            },
            Some("max-run") if fields.len() == 4 => HardConstraint::MaxRun {
                selector: compile_selector(fields[1], fields[2], directive.line, segments)?,
                limit: parse_usize(fields[3], directive.line, "limit")?,
            },
            Some("forbid") if fields.len() >= 2 => HardConstraint::Forbid {
                pattern: compile_matchers(&fields[1..], directive.line, segment_ids, segments)?,
            },
            Some(kind) => {
                return Err(format!(
                    "line {}: malformed or unknown hard constraint `{kind}`",
                    directive.line
                ));
            }
            None => {
                return Err(format!(
                    "line {}: hard constraint must not be empty",
                    directive.line
                ));
            }
        };
        constraints.push(constraint);
    }

    Ok(constraints)
}

fn compile_soft_constraints(
    directives: &[RawDirective],
    segment_ids: &HashMap<String, usize>,
    segments: &[Segment],
) -> Result<Vec<SoftConstraint>, String> {
    let mut constraints = Vec::new();

    for directive in directives {
        let RawKind::Soft(body) = &directive.kind else {
            continue;
        };
        let fields: Vec<_> = body.split_whitespace().collect();
        let constraint = match fields.first().copied() {
            Some("repeat") if fields.len() == 4 => SoftConstraint::Repeat {
                selector: compile_selector(fields[1], fields[2], directive.line, segments)?,
                weight: parse_positive_u64(fields[3], directive.line, "weight")?,
            },
            Some("excess") if fields.len() == 5 => SoftConstraint::Excess {
                selector: compile_selector(fields[1], fields[2], directive.line, segments)?,
                free: parse_usize(fields[3], directive.line, "free allowance")?,
                weight: parse_positive_u64(fields[4], directive.line, "weight")?,
            },
            Some("sequence") if fields.len() >= 3 => SoftConstraint::Sequence {
                weight: parse_positive_u64(fields[1], directive.line, "weight")?,
                pattern: compile_matchers(&fields[2..], directive.line, segment_ids, segments)?,
            },
            Some(kind) => {
                return Err(format!(
                    "line {}: malformed or unknown soft constraint `{kind}`",
                    directive.line
                ));
            }
            None => {
                return Err(format!(
                    "line {}: soft constraint must not be empty",
                    directive.line
                ));
            }
        };
        constraints.push(constraint);
    }

    Ok(constraints)
}

fn compile_matchers(
    tokens: &[&str],
    line: usize,
    segment_ids: &HashMap<String, usize>,
    segments: &[Segment],
) -> Result<Vec<Matcher>, String> {
    if tokens.is_empty() || tokens.len() > MAX_SYMBOLS {
        return Err(format!(
            "line {line}: pattern must have 1 to {MAX_SYMBOLS} matchers"
        ));
    }

    tokens
        .iter()
        .map(|token| {
            if *token == "." {
                Ok(Matcher::Boundary)
            } else if *token == "*" {
                Ok(Matcher::AnySegment)
            } else if token.starts_with('[') && token.ends_with(']') {
                let inside = &token[1..token.len() - 1];
                let (key, value) = inside.split_once('=').ok_or_else(|| {
                    format!("line {line}: expected feature matcher `[key=value]`")
                })?;
                Ok(Matcher::Feature(compile_selector(
                    key, value, line, segments,
                )?))
            } else {
                Ok(Matcher::Segment(resolve_segment(token, line, segment_ids)?))
            }
        })
        .collect()
}

fn compile_selector(
    key: &str,
    value: &str,
    line: usize,
    segments: &[Segment],
) -> Result<Selector, String> {
    validate_feature_atom(key, line)?;
    validate_feature_atom(value, line)?;
    let members: Box<[_]> = segments
        .iter()
        .map(|segment| {
            u8::from(
                segment
                    .features
                    .get(key)
                    .is_some_and(|found| found == value),
            )
        })
        .collect();
    if !members.contains(&1) {
        return Err(format!(
            "line {line}: feature selector `{key}={value}` matches no segment"
        ));
    }
    Ok(Selector { members })
}

fn resolve_segment(
    id: &str,
    line: usize,
    segment_ids: &HashMap<String, usize>,
) -> Result<usize, String> {
    segment_ids
        .get(id)
        .copied()
        .ok_or_else(|| format!("line {line}: unknown segment `{id}`"))
}

fn validate_identifier(identifier: &str, line: usize, kind: &str) -> Result<(), String> {
    let invalid = identifier.is_empty()
        || matches!(identifier, "." | "_" | "*")
        || identifier.contains("->")
        || identifier.starts_with('@')
        || identifier.starts_with('[')
        || identifier.chars().any(|character| {
            character.is_whitespace() || character.is_control() || "|:=#".contains(character)
        });
    if invalid {
        return Err(format!(
            "line {line}: invalid {kind} identifier `{identifier}`"
        ));
    }
    Ok(())
}

fn validate_feature_atom(atom: &str, line: usize) -> Result<(), String> {
    if atom.is_empty()
        || atom.chars().any(|character| {
            character.is_whitespace() || character.is_control() || "[]=#".contains(character)
        })
    {
        return Err(format!("line {line}: invalid feature token `{atom}`"));
    }
    Ok(())
}

fn parse_usize(value: &str, line: usize, label: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("line {line}: {label} must be a non-negative integer"))
}

fn parse_bounded_usize(
    value: &str,
    line: usize,
    label: &str,
    minimum: usize,
    maximum: usize,
) -> Result<usize, String> {
    let parsed = parse_usize(value, line, label)?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(format!(
            "line {line}: {label} must be between {minimum} and {maximum}"
        ));
    }
    Ok(parsed)
}

fn parse_positive_u64(value: &str, line: usize, label: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("line {line}: {label} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("line {line}: {label} must be positive"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY: &str = "\
segments = P M A I\n\
feature type consonant = P M\n\
feature type vowel = A I\n\
feature place labial = P M\n\
spell P = p\n\
start = NAME\n\
rule NAME = 2: @CV . @CV | 1: @CV M\n\
rule CV = 1: @C @V\n\
rule C = 1: P | 1: M\n\
rule V = 1: A | 1: I\n\
rewrite P I -> M I\n\
hard no-repeat type vowel\n\
hard forbid [place=labial] [place=labial]\n\
soft repeat type consonant 2\n\
soft sequence 3 P M\n";

    #[test]
    fn parses_complete_grammar() {
        let grammar = parse(TINY).unwrap();

        assert_eq!(grammar.segments.len(), 4);
        assert_eq!(grammar.rules.len(), 4);
        assert_eq!(grammar.rewrites.len(), 1);
        assert_eq!(grammar.hard_constraints.len(), 2);
        assert_eq!(grammar.soft_constraints.len(), 2);
    }

    #[test]
    fn rejects_unknown_references_and_selectors() {
        assert!(parse(&TINY.replace("1: P | 1: M", "1: X")).is_err());
        assert!(parse(&TINY.replace("place=labial", "place=dorsal")).is_err());
        assert!(parse(&TINY.replace("@CV . @CV", "@MISSING")).is_err());
    }

    #[test]
    fn rejects_cycles_and_unreachable_rules() {
        assert!(parse(&TINY.replace("rule CV = 1: @C @V", "rule CV = 1: @NAME")).is_err());
        assert!(parse(&format!("{TINY}rule UNUSED = 1: P\n")).is_err());
    }

    #[test]
    fn rejects_empty_patterns_and_zero_weights() {
        assert!(parse(&TINY.replace("rewrite P I -> M I", "rewrite _ -> M")).is_err());
        assert!(parse(&TINY.replace("rule C = 1: P | 1: M", "rule C = 1:")).is_err());
        assert!(parse(&TINY.replace("rule C = 1: P", "rule C = 0: P")).is_err());
        assert!(
            parse(&TINY.replace(
                "soft repeat type consonant 2",
                "soft repeat type consonant 0"
            ))
            .is_err()
        );
    }

    #[test]
    fn rejects_ambiguous_identifiers_and_control_spellings() {
        assert!(parse(&TINY.replace("segments = P M A I", "segments = P->M M A I")).is_err());
        assert!(parse(&TINY.replace("spell P = p", "spell P = \u{1b}[31m")).is_err());
    }
}
