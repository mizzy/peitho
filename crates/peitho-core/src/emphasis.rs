//! Line emphasis for code blocks.
//!
//! Emphasis points at specific lines of a code block — "the line I'm talking
//! about right now" — and is a property of the *talk*, not of the content.
//! It is unrelated to syntax highlighting, which colors code by what the code
//! *is*; emphasis renders on top of and independently of the `hl-*` spans.
//!
//! The notation lives in the fence info string after the language token:
//!
//! ```text
//! ```rust {2-4}        static: always emphasized, consumes no slide steps
//! ```rust {2:"aws.ec2.Vpc.Id"}  static: only the unique matching text on line 2
//! ```rust {|3}         stepped: one group consuming one slide step
//! ```rust {|2:"aws.ec2.Vpc.Id"} stepped: matching text on the next slide step
//! ```rust {2,5-7|9}    stepped: one slide step per `|`-separated group
//! ```rust {1@1|8@3}    stepped: groups assigned to absolute slide steps
//! ```
//!
//! The `|` separator is the sole discriminator between the two modes. See
//! `docs/specs/2026-08-01-code-line-emphasis-design.md`.

use std::{
    collections::BTreeSet,
    num::NonZeroUsize,
    ops::{Range, RangeInclusive},
};

use crate::error::{BuildError, ErrorKind, Result};

/// Resolved line emphasis for one code fragment.
///
/// Every stepped group carries an absolute [`NonZeroUsize`] slide step, so a
/// stepped group without a step is unrepresentable. The parser's slide-end
/// check validates slide-level step contiguity and computes the slide step
/// count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineEmphasis {
    kind: LineEmphasisKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LineEmphasisKind {
    Static(Vec<LineGroup>),
    Stepped(Vec<SteppedGroup>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SteppedGroup {
    group: LineGroup,
    step: NonZeroUsize,
}

/// A parsed emphasis spec whose implicit steps have not been assigned yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnresolvedLineEmphasis {
    groups: Vec<LineGroup>,
    steps: UnresolvedSteps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UnresolvedSteps {
    Static,
    Implicit,
    Explicit(Vec<NonZeroUsize>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnresolvedStepsRef<'a> {
    Static,
    Implicit,
    Explicit(&'a [NonZeroUsize]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EmphasisMark {
    None,
    Static,
    Stepped(Vec<NonZeroUsize>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PartialEmphasis {
    Static,
    Stepped(Vec<NonZeroUsize>),
}

/// One non-overlapping partial-line segment and the emphasis that covers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartialMark {
    range: Range<usize>,
    mark: PartialEmphasis,
}

impl PartialMark {
    pub(crate) fn range(&self) -> &Range<usize> {
        &self.range
    }

    pub(crate) fn mark(&self) -> &PartialEmphasis {
        &self.mark
    }
}

/// Every emphasis membership for one source line.
///
/// Whole-line membership is reported separately. Partial ranges carry only
/// emphasis not already supplied by the whole line, split where their marks
/// change, with adjacent equal marks coalesced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineMark {
    whole: EmphasisMark,
    partials: Vec<PartialMark>,
}

impl LineMark {
    pub(crate) fn whole(&self) -> &EmphasisMark {
        &self.whole
    }

    pub(crate) fn partials(&self) -> &[PartialMark] {
        &self.partials
    }
}

impl LineEmphasis {
    pub(crate) fn is_static(&self) -> bool {
        matches!(self.kind, LineEmphasisKind::Static(_))
    }

    /// How `line` is marked by this emphasis spec.
    ///
    /// Returns whole-line state plus normalized partial segments. Overlap is
    /// legal, so stepped membership carries every matching absolute step in
    /// authored order. This is the sole line-membership accessor.
    pub(crate) fn mark_for_line(&self, line: usize) -> LineMark {
        let (whole, contributions) = match &self.kind {
            LineEmphasisKind::Static(groups) => {
                let whole = if groups.iter().any(|group| group.contains_whole_line(line)) {
                    EmphasisMark::Static
                } else {
                    EmphasisMark::None
                };
                let contributions = groups
                    .iter()
                    .flat_map(|group| group.partial_ranges(line))
                    .map(|range| (range.clone(), PartialContribution::Static))
                    .collect::<Vec<_>>();
                (whole, contributions)
            }
            LineEmphasisKind::Stepped(groups) => {
                let steps = groups
                    .iter()
                    .filter(|group| group.group.contains_whole_line(line))
                    .map(|group| group.step)
                    .collect::<Vec<_>>();
                let whole = if steps.is_empty() {
                    EmphasisMark::None
                } else {
                    EmphasisMark::Stepped(steps)
                };
                let contributions = groups
                    .iter()
                    .flat_map(|group| {
                        group.group.partial_ranges(line).map(move |range| {
                            (range.clone(), PartialContribution::Step(group.step))
                        })
                    })
                    .collect::<Vec<_>>();
                (whole, contributions)
            }
        };
        let partials = partial_marks(&contributions, &whole);
        LineMark { whole, partials }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartialContribution {
    Static,
    Step(NonZeroUsize),
}

fn partial_marks(
    contributions: &[(Range<usize>, PartialContribution)],
    whole: &EmphasisMark,
) -> Vec<PartialMark> {
    let mut marks = Vec::<PartialMark>::new();
    for range in partial_boundaries(contributions.iter().map(|(range, _)| range)) {
        let mut is_static = false;
        let mut steps = Vec::new();
        for (candidate, contribution) in contributions {
            if !covers(candidate, &range) {
                continue;
            }
            match contribution {
                PartialContribution::Static => is_static = true,
                PartialContribution::Step(step) if !steps.contains(step) => steps.push(*step),
                PartialContribution::Step(_) => {}
            }
        }

        let mark = if is_static {
            debug_assert!(steps.is_empty());
            PartialEmphasis::Static
        } else if steps.is_empty() {
            continue;
        } else {
            PartialEmphasis::Stepped(steps)
        };
        let Some(mark) = without_whole_membership(mark, whole) else {
            continue;
        };

        if let Some(previous) = marks.last_mut() {
            if previous.range.end == range.start && previous.mark == mark {
                previous.range.end = range.end;
                continue;
            }
        }
        marks.push(PartialMark { range, mark });
    }
    marks
}

fn without_whole_membership(
    partial: PartialEmphasis,
    whole: &EmphasisMark,
) -> Option<PartialEmphasis> {
    match (partial, whole) {
        (PartialEmphasis::Static, EmphasisMark::Static) => None,
        (PartialEmphasis::Stepped(mut partial_steps), EmphasisMark::Stepped(whole_steps)) => {
            partial_steps.retain(|step| !whole_steps.contains(step));
            (!partial_steps.is_empty()).then_some(PartialEmphasis::Stepped(partial_steps))
        }
        (partial, _) => Some(partial),
    }
}

fn partial_boundaries<'a>(ranges: impl Iterator<Item = &'a Range<usize>>) -> Vec<Range<usize>> {
    let mut boundaries = BTreeSet::new();
    for range in ranges {
        boundaries.insert(range.start);
        boundaries.insert(range.end);
    }
    let boundaries = boundaries.into_iter().collect::<Vec<_>>();
    boundaries
        .windows(2)
        .map(|pair| pair[0]..pair[1])
        .collect::<Vec<_>>()
}

fn covers(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

impl UnresolvedLineEmphasis {
    pub(crate) fn steps(&self) -> UnresolvedStepsRef<'_> {
        match &self.steps {
            UnresolvedSteps::Static => UnresolvedStepsRef::Static,
            UnresolvedSteps::Implicit => UnresolvedStepsRef::Implicit,
            UnresolvedSteps::Explicit(steps) => UnresolvedStepsRef::Explicit(steps),
        }
    }

    /// Resolve every group into the representation that rides the pipeline.
    ///
    /// The callback is invoked only for implicit emphasis. It receives the
    /// number of consecutive steps to reserve and returns their typed first
    /// step. Keeping construction here makes both step zero and an unresolved
    /// stepped group unrepresentable in [`LineEmphasis`].
    pub(crate) fn resolve(
        self,
        implicit_first: impl FnOnce(usize) -> Option<NonZeroUsize>,
    ) -> Option<LineEmphasis> {
        let kind = match self.steps {
            UnresolvedSteps::Static => LineEmphasisKind::Static(self.groups),
            UnresolvedSteps::Implicit => {
                let first = implicit_first(self.groups.len())?;
                let groups = self
                    .groups
                    .into_iter()
                    .enumerate()
                    .map(|(offset, group)| {
                        first
                            .get()
                            .checked_add(offset)
                            .and_then(NonZeroUsize::new)
                            .map(|step| SteppedGroup { group, step })
                    })
                    .collect::<Option<Vec<_>>>()?;
                LineEmphasisKind::Stepped(groups)
            }
            UnresolvedSteps::Explicit(steps) => LineEmphasisKind::Stepped(
                self.groups
                    .into_iter()
                    .zip(steps)
                    .map(|(group, step)| SteppedGroup { group, step })
                    .collect(),
            ),
        };
        Some(LineEmphasis { kind })
    }
}

/// The code text whose lines can carry emphasis.
///
/// Fenced code retains its closing newline and any trailing blank lines, but
/// rendering omits those trailing newlines. Parse-time bounds validation and
/// rendering must both use this slice so they cannot disagree about which
/// line numbers exist.
pub(crate) fn emphasis_code(text: &str) -> &str {
    text.trim_end_matches('\n')
}

/// One resolved entry in an emphasis group.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupEntry {
    WholeLines(RangeInclusive<usize>),
    Partial { line: usize, range: Range<usize> },
}

/// One emphasis group: whole lines and partial ranges emphasized together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineGroup {
    entries: Vec<GroupEntry>,
}

impl LineGroup {
    fn contains_whole_line(&self, line: usize) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry, GroupEntry::WholeLines(range) if range.contains(&line)))
    }

    fn partial_ranges(&self, line: usize) -> impl Iterator<Item = &Range<usize>> {
        self.entries.iter().filter_map(move |entry| match entry {
            GroupEntry::Partial {
                line: entry_line,
                range,
            } if *entry_line == line => Some(range),
            GroupEntry::WholeLines(_) | GroupEntry::Partial { .. } => None,
        })
    }

    #[cfg(test)]
    fn lines(&self) -> impl Iterator<Item = usize> + '_ {
        self.entries.iter().flat_map(|entry| match entry {
            GroupEntry::WholeLines(range) => range.clone().collect::<Vec<_>>(),
            GroupEntry::Partial { line, .. } => vec![*line],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedGroupEntry<'a> {
    WholeLines(RangeInclusive<usize>),
    Partial { line: usize, text: &'a str },
}

impl ParsedGroupEntry<'_> {
    fn line_end(&self) -> usize {
        match self {
            Self::WholeLines(range) => *range.end(),
            Self::Partial { line, .. } => *line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLineGroup<'a> {
    entries: Vec<ParsedGroupEntry<'a>>,
}

/// Parse the text between the braces of an emphasis spec and validate it
/// against the code block whose lines it will emphasize.
///
/// Grammar:
///
/// ```text
/// spec  := "|"? group ("|" group)*
/// group := item ("," item)* ("@" step)?
/// item  := N | N "-" M | N ":" '"' text '"'
/// step  := N                      (1-based)
/// ```
///
/// A spec is stepped iff it contains `|`. Within one stepped spec, either
/// every group has `@step` or none does. A static spec (no `|`) never carries
/// `@step`.
///
/// Every malformed shape is a line-numbered error: silently accepting a spec
/// that does not mean what the author wrote would send them on stage with the
/// wrong line emphasized.
pub(crate) fn parse_emphasis_spec(
    spec: &str,
    code_text: &str,
    line: usize,
) -> Result<UnresolvedLineEmphasis> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(error(
            line,
            "empty emphasis spec",
            "list the lines to emphasize, e.g. `{2-4}`, or remove the braces",
        ));
    }

    let mut raw_groups = split_unquoted(trimmed, '|', line)?;
    let stepped = raw_groups.len() > 1;
    if trimmed.starts_with('|') {
        raw_groups.remove(0);
    }

    let mut parsed_groups = Vec::new();
    let mut authored_steps = Vec::new();
    for raw_group in raw_groups {
        let (group, step) = parse_group(raw_group, line)?;
        parsed_groups.push(group);
        authored_steps.push(step);
    }

    let steps = if !stepped {
        if authored_steps[0].is_some() {
            return Err(error(
                line,
                "explicit emphasis steps require stepped emphasis",
                format!("write `{{|{trimmed}}}` to make this a stepped group, or remove the `@N`"),
            ));
        }
        UnresolvedSteps::Static
    } else {
        let explicit_count = authored_steps.iter().flatten().count();
        if explicit_count == 0 {
            UnresolvedSteps::Implicit
        } else if explicit_count != authored_steps.len() {
            return Err(error(
                line,
                "a code block cannot mix implicit and explicit emphasis steps",
                "add `@N` to every `|`-separated group in this block, or remove every `@N`",
            ));
        } else {
            let steps = authored_steps.into_iter().flatten().collect::<Vec<_>>();
            let mut seen = BTreeSet::new();
            if let Some(repeated) = steps.iter().copied().find(|step| !seen.insert(*step)) {
                return Err(error(
                    line,
                    format!("explicit emphasis step {repeated} is repeated"),
                    "merge the line entries into one group, e.g. `{|2,5@1}`",
                ));
            }
            UnresolvedSteps::Explicit(steps)
        }
    };

    let code_lines = emphasis_code(code_text).lines().collect::<Vec<_>>();
    let line_count = code_lines.len();
    let max = parsed_groups
        .iter()
        .flat_map(|group| group.entries.iter())
        .map(ParsedGroupEntry::line_end)
        .max()
        .expect("line emphasis always has at least one group with one entry");
    if max > line_count {
        return Err(error(
            line,
            format!("emphasis line {max} is past the end of a {line_count}-line code block"),
            format!("this block has {line_count} line(s); emphasize a line in 1-{line_count}"),
        ));
    }

    let groups = parsed_groups
        .into_iter()
        .map(|group| resolve_group(group, &code_lines, line))
        .collect::<Result<Vec<_>>>()?;
    let emphasis = UnresolvedLineEmphasis { groups, steps };

    Ok(emphasis)
}

fn parse_group(raw: &str, line: usize) -> Result<(ParsedLineGroup<'_>, Option<NonZeroUsize>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(error(
            line,
            "empty emphasis group",
            "every `|`-separated group needs at least one line, e.g. `{1|3}`; write `{|1}` for a single stepped group",
        ));
    }

    let at_parts = split_unquoted(trimmed, '@', line)?;
    if at_parts.len() > 2 {
        return Err(error(
            line,
            "emphasis group has more than one `@`",
            "write one step after the group, e.g. `2,5@1`",
        ));
    }
    let group_spec = at_parts[0].trim();
    let raw_step = at_parts.get(1).copied();
    let step = raw_step
        .map(|raw_step| parse_step_number(raw_step.trim(), line))
        .transpose()?;

    if group_spec.is_empty() {
        return Err(error(
            line,
            "empty emphasis group",
            "put the emphasized line numbers before `@`, e.g. `2,5@1`",
        ));
    }

    let mut entries = Vec::new();
    for raw_item in split_unquoted(group_spec, ',', line)? {
        entries.push(parse_item(raw_item, line)?);
    }
    Ok((ParsedLineGroup { entries }, step))
}

fn split_unquoted(text: &str, delimiter: char, line: usize) -> Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            character if character == delimiter && !quoted => {
                parts.push(&text[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if quoted {
        return Err(unclosed_partial_text_error(line));
    }
    parts.push(&text[start..]);
    Ok(parts)
}

fn parse_step_number(text: &str, line: usize) -> Result<NonZeroUsize> {
    parse_positive_integer(
        text,
        line,
        PositiveIntegerDiagnostic::new(
            "explicit emphasis step is missing its number",
            "write a positive step after `@`, e.g. `2,5@1`",
        ),
        PositiveIntegerDiagnostic::new(
            "explicit emphasis step must be a number",
            format!("`{text}` is not a step number; write a positive integer such as `@1`"),
        ),
        PositiveIntegerDiagnostic::new(
            "explicit emphasis step is too large",
            format!("`{text}` exceeds the largest supported step; use a smaller positive integer"),
        ),
        PositiveIntegerDiagnostic::new(
            "emphasis steps start at 1",
            "write `@1` for the first step; step 0 is the initial state",
        ),
    )
}

fn parse_item(raw: &str, line: usize) -> Result<ParsedGroupEntry<'_>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(error(
            line,
            "empty emphasis group",
            "every `,`-separated entry needs a line number, e.g. `{2,5}`",
        ));
    }

    if let Some(colon) = partial_entry_colon(trimmed) {
        return parse_partial_item(trimmed, colon, line);
    }

    match trimmed.split_once('-') {
        Some((start, end)) => {
            let start = parse_line_number(start.trim(), trimmed, line)?;
            let end = parse_line_number(end.trim(), trimmed, line)?;
            if end < start {
                return Err(error(
                    line,
                    "emphasis range end is before its start",
                    format!("write the range as `{end}-{start}`, or check the line numbers"),
                ));
            }
            Ok(ParsedGroupEntry::WholeLines(start..=end))
        }
        None => {
            let only = parse_line_number(trimmed, trimmed, line)?;
            Ok(ParsedGroupEntry::WholeLines(only..=only))
        }
    }
}

fn partial_entry_colon(item: &str) -> Option<usize> {
    item.char_indices().find_map(|(index, character)| {
        (character == ':' && item[index + 1..].trim_start().starts_with('"')).then_some(index)
    })
}

fn parse_partial_item(item: &str, colon: usize, line: usize) -> Result<ParsedGroupEntry<'_>> {
    let line_spec = item[..colon].trim();
    if line_spec.is_empty() {
        return Err(error(
            line,
            "partial emphasis needs a line number",
            r#"write a line number before the text, e.g. `2:"text"`"#,
        ));
    }
    if line_spec.contains('-') {
        return Err(error(
            line,
            "partial emphasis cannot use a line range",
            "name each line separately, e.g. `2:\"text\",3:\"text\",4:\"text\"`",
        ));
    }
    let target_line = parse_line_number(line_spec, line_spec, line)?;

    let quoted = item[colon + 1..]
        .trim_start()
        .strip_prefix('"')
        .expect("partial_entry_colon requires a quote after the colon");
    let Some(close) = quoted.find('"') else {
        return Err(unclosed_partial_text_error(line));
    };
    let text = &quoted[..close];
    if text.is_empty() {
        return Err(error(
            line,
            "partial emphasis text cannot be empty",
            "put the exact text to emphasize between the quotes",
        ));
    }
    let trailing = &quoted[close + 1..];
    if !trailing.trim().is_empty() {
        if trailing.contains('"') {
            return Err(error(
                line,
                "partial emphasis text cannot contain `\"` or `}`",
                r#"partial emphasis text ends at the next `"` and cannot contain `"` or `}`; there is no escape syntax"#,
            ));
        }
        return Err(error(
            line,
            "unexpected text after partial emphasis text",
            "put `,`, `|`, or `@N` after the closing quote",
        ));
    }

    Ok(ParsedGroupEntry::Partial {
        line: target_line,
        text,
    })
}

fn unclosed_partial_text_error(line: usize) -> BuildError {
    error(
        line,
        "unclosed partial emphasis text",
        "close the text with `\"`; emphasis text has no escape syntax",
    )
}

fn resolve_group(
    group: ParsedLineGroup<'_>,
    code_lines: &[&str],
    source_line: usize,
) -> Result<LineGroup> {
    let entries = group
        .entries
        .into_iter()
        .map(|entry| match entry {
            ParsedGroupEntry::WholeLines(range) => Ok(GroupEntry::WholeLines(range)),
            ParsedGroupEntry::Partial { line, text } => {
                let code_line = code_lines
                    .get(line - 1)
                    .expect("partial emphasis line was bounds-checked");
                resolve_partial_entry(line, text, code_line, source_line)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(LineGroup { entries })
}

fn resolve_partial_entry(
    target_line: usize,
    text: &str,
    code_line: &str,
    source_line: usize,
) -> Result<GroupEntry> {
    let mut matches = code_line
        .char_indices()
        .filter_map(|(start, _)| code_line[start..].starts_with(text).then_some(start));
    let Some(start) = matches.next() else {
        return Err(error(
            source_line,
            format!("partial emphasis text was not found on code line {target_line}"),
            format!("choose text that occurs exactly once on code line {target_line}"),
        ));
    };
    if matches.next().is_some() {
        return Err(error(
            source_line,
            format!("partial emphasis text occurs more than once on code line {target_line}"),
            "lengthen the string until it is unique on that line",
        ));
    }
    Ok(GroupEntry::Partial {
        line: target_line,
        range: start..start + text.len(),
    })
}

fn parse_line_number(text: &str, item: &str, line: usize) -> Result<usize> {
    parse_positive_integer(
        text,
        line,
        PositiveIntegerDiagnostic::new(
            "incomplete emphasis range",
            format!("`{item}` is missing a line number; write a range as `2-4`"),
        ),
        PositiveIntegerDiagnostic::new(
            "emphasis spec expects line numbers",
            format!("`{item}` is not a line number or range; write e.g. `2`, `2-4`, or `2,5-7`"),
        ),
        PositiveIntegerDiagnostic::new(
            "emphasis line number is too large",
            format!(
                "`{text}` exceeds the largest supported code line number; use a smaller positive integer"
            ),
        ),
        PositiveIntegerDiagnostic::new(
            "code line numbers start at 1",
            "the first line of a code block is line 1, not line 0",
        ),
    )
    .map(NonZeroUsize::get)
}

struct PositiveIntegerDiagnostic {
    message: String,
    help: String,
}

impl PositiveIntegerDiagnostic {
    fn new(message: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            help: help.into(),
        }
    }
}

fn parse_positive_integer(
    text: &str,
    line: usize,
    missing: PositiveIntegerDiagnostic,
    invalid: PositiveIntegerDiagnostic,
    too_large: PositiveIntegerDiagnostic,
    zero: PositiveIntegerDiagnostic,
) -> Result<NonZeroUsize> {
    if text.is_empty() {
        return Err(error(line, missing.message, missing.help));
    }
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error(line, invalid.message, invalid.help));
    }
    let value = match text.parse::<usize>() {
        Ok(value) => value,
        Err(_) => return Err(error(line, too_large.message, too_large.help)),
    };
    NonZeroUsize::new(value).ok_or_else(|| error(line, zero.message, zero.help))
}

fn error(line: usize, message: impl Into<String>, help: impl Into<String>) -> BuildError {
    BuildError::new(ErrorKind::Parse, Some(line), message, help)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14";

    fn parse(spec: &str, line: usize) -> Result<UnresolvedLineEmphasis> {
        parse_emphasis_spec(spec, CODE, line)
    }

    fn group_lines(emphasis: &UnresolvedLineEmphasis, index: usize) -> Vec<usize> {
        emphasis.groups[index].lines().collect()
    }

    fn stepped_values(mark: LineMark) -> Vec<usize> {
        match mark.whole {
            EmphasisMark::Stepped(steps) => steps.into_iter().map(NonZeroUsize::get).collect(),
            other => panic!("expected stepped line mark, got {other:?}"),
        }
    }

    #[test]
    fn parses_static_and_stepped_specs() {
        let e = parse("2-4", 3).unwrap();
        assert_eq!(e.steps(), UnresolvedStepsRef::Static);
        assert_eq!(e.groups.len(), 1);
        assert_eq!(group_lines(&e, 0), vec![2, 3, 4]);

        let e = parse("2,5-7|9", 3).unwrap();
        assert_eq!(e.steps(), UnresolvedStepsRef::Implicit);
        assert_eq!(e.groups.len(), 2);
        assert_eq!(group_lines(&e, 0), vec![2, 5, 6, 7]);
        assert_eq!(group_lines(&e, 1), vec![9]);
    }

    #[test]
    fn parses_a_single_line_as_a_range() {
        let e = parse("3", 3).unwrap();
        assert_eq!(e.steps(), UnresolvedStepsRef::Static);
        assert_eq!(group_lines(&e, 0), vec![3]);
    }

    #[test]
    fn a_leading_separator_makes_a_single_group_stepped() {
        let e = parse("|1", 3).unwrap();
        assert_eq!(e.steps(), UnresolvedStepsRef::Implicit);
        assert_eq!(e.groups.len(), 1);
        assert_eq!(group_lines(&e, 0), vec![1]);

        let e = parse(" | 2 ", 3).unwrap();
        assert_eq!(e.steps(), UnresolvedStepsRef::Implicit);
        assert_eq!(e.groups.len(), 1);
        assert_eq!(group_lines(&e, 0), vec![2]);
    }

    #[test]
    fn a_leading_separator_can_prefix_multiple_groups() {
        let e = parse("|1|3", 3).unwrap();
        assert_eq!(e.steps(), UnresolvedStepsRef::Implicit);
        assert_eq!(e.groups.len(), 2);
        assert_eq!(group_lines(&e, 0), vec![1]);
        assert_eq!(group_lines(&e, 1), vec![3]);
    }

    #[test]
    fn parses_explicit_steps_on_every_stepped_group() {
        let emphasis = parse("1-3@1|8@3", 3).unwrap();
        let UnresolvedStepsRef::Explicit(steps) = emphasis.steps() else {
            panic!("expected explicit emphasis");
        };
        assert_eq!(
            steps
                .iter()
                .copied()
                .map(NonZeroUsize::get)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert!(parse("|2-4@2", 3).is_ok());
        assert!(parse("8@3|1-3@1", 3).is_ok());
    }

    #[test]
    fn malformed_explicit_steps_are_line_numbered_errors() {
        for (spec, message) in [
            ("1@", "explicit emphasis step is missing its number"),
            ("1@next", "explicit emphasis step must be a number"),
            ("|1@+1", "explicit emphasis step must be a number"),
            ("1@2@3", "emphasis group has more than one `@`"),
            ("1@0", "emphasis steps start at 1"),
        ] {
            let err = parse(spec, 17).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Parse, "spec {spec:?}");
            assert_eq!(err.line, Some(17), "spec {spec:?}");
            assert_eq!(err.message, message, "spec {spec:?}");
            assert!(!err.help.is_empty(), "spec {spec:?}");
        }
    }

    #[test]
    fn explicit_steps_require_stepped_notation() {
        let err = parse("2-4@2", 19).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(19));
        assert_eq!(
            err.message,
            "explicit emphasis steps require stepped emphasis"
        );
        assert!(err.help.contains("{|2-4@2}"), "help: {}", err.help);
    }

    #[test]
    fn one_block_cannot_mix_implicit_and_explicit_steps() {
        let err = parse("1@1|2", 23).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(23));
        assert_eq!(
            err.message,
            "a code block cannot mix implicit and explicit emphasis steps"
        );
        assert!(err.help.contains("@N"), "help: {}", err.help);
    }

    #[test]
    fn one_block_cannot_repeat_an_explicit_step() {
        let err = parse("2@1|5@1", 29).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(29));
        assert_eq!(err.message, "explicit emphasis step 1 is repeated");
        let suggested = "|2,5@1";
        assert!(
            err.help.contains(&format!("{{{suggested}}}")),
            "help: {}",
            err.help
        );
        assert!(parse(suggested, 29).is_ok());
    }

    #[test]
    fn tolerates_whitespace_around_items() {
        let e = parse(" 2 , 5 - 7 | 9 ", 3).unwrap();
        assert_eq!(e.steps(), UnresolvedStepsRef::Implicit);
        assert_eq!(group_lines(&e, 0), vec![2, 5, 6, 7]);
        assert_eq!(group_lines(&e, 1), vec![9]);
    }

    #[test]
    fn quoted_partial_entries_keep_separators_punctuation_and_spaces_literal() {
        let code = "environment: 'dev' | 'stg' | 'prod', owner@example.com - ready\n\
                    value: alpha, beta @ gamma - delta: epsilon\n";
        let emphasis = parse_emphasis_spec(
            r#"1:"environment: 'dev' | 'stg' | 'prod', owner@example.com - ready"@1|2:"alpha, beta @ gamma - delta: epsilon"@2"#,
            code,
            13,
        )
        .unwrap();

        let UnresolvedStepsRef::Explicit(steps) = emphasis.steps() else {
            panic!("expected explicit emphasis");
        };
        assert_eq!(
            steps
                .iter()
                .copied()
                .map(NonZeroUsize::get)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(emphasis.groups.len(), 2);

        let spaced =
            parse_emphasis_spec(r#"1:"  exact text  ""#, "prefix  exact text  suffix\n", 13);
        assert!(spaced.is_ok(), "spaces inside quotes must not be trimmed");

        let static_punctuation =
            parse_emphasis_spec(r#"1:"a|b,c@d-e: f""#, "a|b,c@d-e: f\n", 13).unwrap();
        assert_eq!(static_punctuation.steps(), UnresolvedStepsRef::Static);
    }

    #[test]
    fn whole_line_and_partial_entries_can_share_one_group() {
        let emphasis =
            parse_emphasis_spec(r#"1,2:"string""#, "whole line\ntype = string\n", 11).unwrap();

        assert_eq!(emphasis.steps(), UnresolvedStepsRef::Static);
        assert_eq!(emphasis.groups.len(), 1);
    }

    #[test]
    fn partial_match_errors_distinguish_missing_and_non_unique_text() {
        let missing =
            parse_emphasis_spec(r#"2:"integer""#, "first\ntype = string\n", 17).unwrap_err();
        assert_eq!(missing.kind, ErrorKind::Parse);
        assert_eq!(missing.line, Some(17));
        assert_eq!(
            missing.message,
            "partial emphasis text was not found on code line 2"
        );
        assert!(!missing.help.is_empty());

        let repeated = parse_emphasis_spec(r#"1:"aa""#, "aaaa\n", 19).unwrap_err();
        assert_eq!(repeated.kind, ErrorKind::Parse);
        assert_eq!(repeated.line, Some(19));
        assert_eq!(
            repeated.message,
            "partial emphasis text occurs more than once on code line 1"
        );
        assert!(
            repeated
                .help
                .contains("lengthen the string until it is unique"),
            "help: {}",
            repeated.help
        );
    }

    #[test]
    fn overlapping_partial_matches_count_as_multiple_occurrences() {
        let err = parse_emphasis_spec(r#"1:"aa""#, "aaa\n", 19).unwrap_err();

        assert_eq!(err.line, Some(19));
        assert_eq!(
            err.message,
            "partial emphasis text occurs more than once on code line 1"
        );
    }

    #[test]
    fn malformed_partial_entries_have_focused_errors() {
        for (spec, message) in [
            (r#"1:"""#, "partial emphasis text cannot be empty"),
            (r#"1:"text"#, "unclosed partial emphasis text"),
            (r#"2-4:"text""#, "partial emphasis cannot use a line range"),
        ] {
            let err = parse_emphasis_spec(spec, "text\ntext\ntext\ntext\n", 23).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Parse, "spec {spec:?}");
            assert_eq!(err.line, Some(23), "spec {spec:?}");
            assert_eq!(err.message, message, "spec {spec:?}");
            assert!(!err.help.is_empty(), "spec {spec:?}");
        }

        let escaped_quote =
            parse_emphasis_spec(r#"1:"say \"hi\"""#, "say \"hi\"\n", 23).unwrap_err();
        assert_eq!(escaped_quote.kind, ErrorKind::Parse);
        assert_eq!(escaped_quote.line, Some(23));
        assert_eq!(
            escaped_quote.message,
            "partial emphasis text cannot contain `\"` or `}`"
        );
        assert!(
            escaped_quote.help.contains("ends at the next `\"`"),
            "{}",
            escaped_quote.help
        );
        assert!(
            escaped_quote.help.contains("cannot contain `\"` or `}`"),
            "{}",
            escaped_quote.help
        );
        assert!(
            escaped_quote.help.contains("no escape syntax"),
            "{}",
            escaped_quote.help
        );
    }

    #[test]
    fn partial_emphasis_without_a_line_number_has_a_focused_error() {
        let err = parse_emphasis_spec(r#":"a""#, "a\n", 25).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(25));
        assert_eq!(err.message, "partial emphasis needs a line number");
        assert!(err.help.contains(r#"2:"text""#), "{}", err.help);
    }

    #[test]
    fn partial_emphasis_rejects_non_delimiter_text_after_the_closing_quote() {
        let err = parse_emphasis_spec(r#"2:"a"b"#, "first\na\n", 27).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(27));
        assert_eq!(err.message, "unexpected text after partial emphasis text");
        assert_eq!(err.help, "put `,`, `|`, or `@N` after the closing quote");
    }

    #[test]
    fn partial_entries_keep_the_existing_out_of_range_error_priority() {
        let err = parse_emphasis_spec(r#"2:"text""#, "one line\n", 29).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(29));
        assert_eq!(
            err.message,
            "emphasis line 2 is past the end of a 1-line code block"
        );
    }

    #[test]
    fn a_trailing_separator_is_an_empty_group_error() {
        // `{2|}` is stepped notation with an empty second group: an error,
        // not a silent downgrade to static emphasis.
        let err = parse("2|", 3).unwrap_err();
        assert_eq!(err.message, "empty emphasis group");
    }

    #[test]
    fn emphasis_spec_errors_are_line_numbered() {
        for (spec, message) in [
            ("0", "code line numbers start at 1"),
            ("+2", "emphasis spec expects line numbers"),
            ("2-0", "code line numbers start at 1"),
            ("4-2", "emphasis range end is before its start"),
            ("2-", "incomplete emphasis range"),
            ("-4", "incomplete emphasis range"),
            ("a", "emphasis spec expects line numbers"),
            ("2-x", "emphasis spec expects line numbers"),
            ("", "empty emphasis spec"),
            ("   ", "empty emphasis spec"),
            ("2||4", "empty emphasis group"),
            ("2,,4", "empty emphasis group"),
            ("|", "empty emphasis group"),
            ("||1", "empty emphasis group"),
        ] {
            let err = parse(spec, 7).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Parse, "spec {spec:?}");
            assert_eq!(err.line, Some(7), "spec {spec:?}");
            assert_eq!(err.message, message, "spec {spec:?}");
            assert!(!err.help.is_empty(), "spec {spec:?}");
        }
    }

    #[test]
    fn an_absurd_line_number_errors_instead_of_panicking() {
        // Overflow must surface as a parse error, never a panic.
        let err = parse("99999999999999999999999999", 3).unwrap_err();
        assert_eq!(err.message, "emphasis line number is too large");
    }

    #[test]
    fn oversized_emphasis_steps_have_a_distinct_error() {
        let err = parse("|1@99999999999999999999999", 3).unwrap_err();
        assert_eq!(err.message, "explicit emphasis step is too large");
        assert!(!err.help.is_empty());
    }

    #[test]
    fn highest_referenced_line_is_checked_at_the_parse_seam() {
        assert!(parse("12-14|3", 3).is_ok());

        let err = parse_emphasis_spec("2,5-7|15", CODE, 3).unwrap_err();
        assert_eq!(
            err.message,
            "emphasis line 15 is past the end of a 14-line code block"
        );
    }

    #[test]
    fn validation_and_rendering_share_the_same_emphasizable_code() {
        assert_eq!(emphasis_code("a\n\n\n"), "a");

        let err = parse_emphasis_spec("1|3", "a\n\n\n", 7).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "emphasis line 3 is past the end of a 1-line code block"
        );
        assert_eq!(
            err.help,
            "this block has 1 line(s); emphasize a line in 1-1"
        );
    }

    #[test]
    fn mark_for_line_distinguishes_none_static_and_every_stepped_group() {
        let e = parse("1-5|3", 3)
            .unwrap()
            .resolve(|_| Some(NonZeroUsize::MIN))
            .unwrap();
        assert_eq!(stepped_values(e.mark_for_line(3)), vec![1, 2]);
        assert_eq!(stepped_values(e.mark_for_line(1)), vec![1]);
        assert_eq!(e.mark_for_line(6).whole, EmphasisMark::None);

        let e = parse("1-3|5", 3)
            .unwrap()
            .resolve(|_| NonZeroUsize::new(4))
            .unwrap();
        assert_eq!(e.mark_for_line(4).whole, EmphasisMark::None);
        assert_eq!(stepped_values(e.mark_for_line(5)), vec![5]);

        let e = parse("2-4", 3).unwrap().resolve(|_| None).unwrap();
        assert_eq!(e.mark_for_line(1).whole, EmphasisMark::None);
        assert_eq!(e.mark_for_line(2).whole, EmphasisMark::Static);
    }

    #[test]
    fn mark_for_line_reports_whole_marks_and_boundary_split_partial_segments() {
        let emphasis = parse_emphasis_spec(r#"1,2:"string""#, "whole line\ntype = string\n", 3)
            .unwrap()
            .resolve(|_| None)
            .unwrap();
        assert_eq!(emphasis.mark_for_line(1).whole, EmphasisMark::Static);
        let partial = emphasis.mark_for_line(2);
        assert_eq!(partial.whole, EmphasisMark::None);
        assert_eq!(partial.partials.len(), 1);
        assert_eq!(partial.partials[0].range, 7..13);
        assert_eq!(partial.partials[0].mark, PartialEmphasis::Static);

        let overlapping = parse_emphasis_spec(r#"1:"abcd"|1:"cdef""#, "abcdef\n", 3)
            .unwrap()
            .resolve(|_| Some(NonZeroUsize::MIN))
            .unwrap();
        let mark = overlapping.mark_for_line(1);
        assert_eq!(mark.whole, EmphasisMark::None);
        assert_eq!(
            mark.partials,
            vec![
                PartialMark {
                    range: 0..2,
                    mark: PartialEmphasis::Stepped(vec![NonZeroUsize::new(1).unwrap()]),
                },
                PartialMark {
                    range: 2..4,
                    mark: PartialEmphasis::Stepped(vec![
                        NonZeroUsize::new(1).unwrap(),
                        NonZeroUsize::new(2).unwrap(),
                    ]),
                },
                PartialMark {
                    range: 4..6,
                    mark: PartialEmphasis::Stepped(vec![NonZeroUsize::new(2).unwrap()]),
                },
            ]
        );
    }

    #[test]
    fn nested_partial_ranges_keep_distinct_marks_and_merge_equal_neighbors() {
        let nested = parse_emphasis_spec(r#"1:"abcdef"@1|1:"cd"@2"#, "abcdef\n", 3)
            .unwrap()
            .resolve(|_| panic!("explicit emphasis must not request a step"))
            .unwrap();
        assert_eq!(
            nested.mark_for_line(1).partials,
            vec![
                PartialMark {
                    range: 0..2,
                    mark: PartialEmphasis::Stepped(vec![NonZeroUsize::new(1).unwrap()]),
                },
                PartialMark {
                    range: 2..4,
                    mark: PartialEmphasis::Stepped(vec![
                        NonZeroUsize::new(1).unwrap(),
                        NonZeroUsize::new(2).unwrap(),
                    ]),
                },
                PartialMark {
                    range: 4..6,
                    mark: PartialEmphasis::Stepped(vec![NonZeroUsize::new(1).unwrap()]),
                },
            ]
        );

        let static_adjacent = parse_emphasis_spec(r#"1:"abcd",1:"cd""#, "abcd\n", 3)
            .unwrap()
            .resolve(|_| None)
            .unwrap();
        assert_eq!(
            static_adjacent.mark_for_line(1).partials,
            vec![PartialMark {
                range: 0..4,
                mark: PartialEmphasis::Static,
            }]
        );

        let stepped_adjacent = parse_emphasis_spec(r#"|1:"ab",1:"cd"@1"#, "abcd\n", 3)
            .unwrap()
            .resolve(|_| panic!("explicit emphasis must not request a step"))
            .unwrap();
        assert_eq!(
            stepped_adjacent.mark_for_line(1).partials,
            vec![PartialMark {
                range: 0..4,
                mark: PartialEmphasis::Stepped(vec![NonZeroUsize::new(1).unwrap()]),
            }]
        );
    }

    #[test]
    fn whole_line_marks_remove_duplicate_partial_membership() {
        let static_emphasis = parse_emphasis_spec(r#"1,1:"x""#, "xy\n", 3)
            .unwrap()
            .resolve(|_| None)
            .unwrap();
        let static_mark = static_emphasis.mark_for_line(1);
        assert_eq!(static_mark.whole, EmphasisMark::Static);
        assert!(static_mark.partials.is_empty());

        let stepped_emphasis = parse_emphasis_spec(r#"1,1:"x"@1|1:"y"@2"#, "xy\n", 3)
            .unwrap()
            .resolve(|_| panic!("explicit emphasis must not request a step"))
            .unwrap();
        let stepped_mark = stepped_emphasis.mark_for_line(1);
        assert_eq!(
            stepped_mark.whole,
            EmphasisMark::Stepped(vec![NonZeroUsize::new(1).unwrap()])
        );
        assert_eq!(
            stepped_mark.partials,
            vec![PartialMark {
                range: 1..2,
                mark: PartialEmphasis::Stepped(vec![NonZeroUsize::new(2).unwrap()]),
            }]
        );
    }

    #[test]
    fn resolving_static_and_explicit_emphasis_does_not_request_an_implicit_step() {
        let static_emphasis = parse("2-4", 3)
            .unwrap()
            .resolve(|_| panic!("static emphasis must not request a step"))
            .unwrap();
        assert_eq!(static_emphasis.mark_for_line(2).whole, EmphasisMark::Static);

        let explicit_emphasis = parse("1@2|3@1", 3)
            .unwrap()
            .resolve(|_| panic!("explicit emphasis must not request a step"))
            .unwrap();
        assert_eq!(stepped_values(explicit_emphasis.mark_for_line(1)), vec![2]);
        assert_eq!(stepped_values(explicit_emphasis.mark_for_line(3)), vec![1]);
    }
}
