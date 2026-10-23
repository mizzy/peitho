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
//! ```rust {|3}         stepped: one group consuming one slide step
//! ```rust {2,5-7|9}    stepped: one slide step per `|`-separated group
//! ```rust {1@1|8@3}    stepped: groups assigned to absolute slide steps
//! ```
//!
//! The `|` separator is the sole discriminator between the two modes. See
//! `docs/specs/2026-08-01-code-line-emphasis-design.md`.

use std::{num::NonZeroUsize, ops::RangeInclusive};

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
pub(crate) enum LineMark {
    None,
    Static,
    Stepped(Vec<NonZeroUsize>),
}

impl LineEmphasis {
    pub(crate) fn is_static(&self) -> bool {
        matches!(self.kind, LineEmphasisKind::Static(_))
    }

    /// How `line` is marked by this emphasis spec.
    ///
    /// Overlap is legal, so stepped membership carries every matching
    /// absolute step in authored order. This is the sole line-membership
    /// accessor.
    pub(crate) fn mark_for_line(&self, line: usize) -> LineMark {
        match &self.kind {
            LineEmphasisKind::Static(groups) => {
                if groups.iter().any(|group| group.contains(line)) {
                    LineMark::Static
                } else {
                    LineMark::None
                }
            }
            LineEmphasisKind::Stepped(groups) => {
                let steps = groups
                    .iter()
                    .filter(|group| group.group.contains(line))
                    .map(|group| group.step)
                    .collect::<Vec<_>>();
                if steps.is_empty() {
                    LineMark::None
                } else {
                    LineMark::Stepped(steps)
                }
            }
        }
    }
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

    /// The highest line number referenced by any group.
    ///
    /// Validation at construction compares this with the block's emphasizable
    /// line count, so an out-of-range emphasis can never reach the renderer.
    fn max_line(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|group| group.ranges.iter())
            .map(|range| *range.end())
            .max()
            .expect("line emphasis always has at least one group with one range")
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

/// One emphasis group: the set of lines emphasized together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineGroup {
    ranges: Vec<RangeInclusive<usize>>,
}

impl LineGroup {
    fn contains(&self, line: usize) -> bool {
        self.ranges.iter().any(|range| range.contains(&line))
    }

    #[cfg(test)]
    fn lines(&self) -> impl Iterator<Item = usize> + '_ {
        self.ranges.iter().flat_map(|range| range.clone())
    }
}

/// Parse the text between the braces of an emphasis spec and validate it
/// against the code block whose lines it will emphasize.
///
/// Grammar:
///
/// ```text
/// spec  := "|"? group ("|" group)*
/// group := item ("," item)* ("@" step)?
/// item  := N | N "-" M          (1-based, inclusive, N <= M)
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

    let stepped = trimmed.contains('|');
    let groups_spec = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let mut groups = Vec::new();
    let mut authored_steps = Vec::new();
    for raw_group in groups_spec.split('|') {
        let (group, step) = parse_group(raw_group, line)?;
        groups.push(group);
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
            let mut seen = std::collections::BTreeSet::new();
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

    let emphasis = UnresolvedLineEmphasis { groups, steps };
    let line_count = emphasis_code(code_text).lines().count();
    if emphasis.max_line() > line_count {
        let max = emphasis.max_line();
        return Err(error(
            line,
            format!("emphasis line {max} is past the end of a {line_count}-line code block"),
            format!("this block has {line_count} line(s); emphasize a line in 1-{line_count}"),
        ));
    }

    Ok(emphasis)
}

fn parse_group(raw: &str, line: usize) -> Result<(LineGroup, Option<NonZeroUsize>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(error(
            line,
            "empty emphasis group",
            "every `|`-separated group needs at least one line, e.g. `{1|3}`; write `{|1}` for a single stepped group",
        ));
    }

    let mut at_parts = trimmed.split('@');
    let group_spec = at_parts
        .next()
        .expect("split always yields one part")
        .trim();
    let raw_step = at_parts.next();
    if at_parts.next().is_some() {
        return Err(error(
            line,
            "emphasis group has more than one `@`",
            "write one step after the group, e.g. `2,5@1`",
        ));
    }
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

    let mut ranges = Vec::new();
    for raw_item in group_spec.split(',') {
        ranges.push(parse_item(raw_item, line)?);
    }
    Ok((LineGroup { ranges }, step))
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

fn parse_item(raw: &str, line: usize) -> Result<RangeInclusive<usize>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(error(
            line,
            "empty emphasis group",
            "every `,`-separated entry needs a line number, e.g. `{2,5}`",
        ));
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
            Ok(start..=end)
        }
        None => {
            let only = parse_line_number(trimmed, trimmed, line)?;
            Ok(only..=only)
        }
    }
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
        match mark {
            LineMark::Stepped(steps) => steps.into_iter().map(NonZeroUsize::get).collect(),
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
    fn max_line_reports_the_highest_referenced_line() {
        let e = parse("2,5-7|9", 3).unwrap();
        assert_eq!(e.max_line(), 9);

        let e = parse("12-14|3", 3).unwrap();
        assert_eq!(e.max_line(), 14);
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
        assert_eq!(e.mark_for_line(6), LineMark::None);

        let e = parse("1-3|5", 3)
            .unwrap()
            .resolve(|_| NonZeroUsize::new(4))
            .unwrap();
        assert_eq!(e.mark_for_line(4), LineMark::None);
        assert_eq!(stepped_values(e.mark_for_line(5)), vec![5]);

        let e = parse("2-4", 3).unwrap().resolve(|_| None).unwrap();
        assert_eq!(e.mark_for_line(1), LineMark::None);
        assert_eq!(e.mark_for_line(2), LineMark::Static);
    }

    #[test]
    fn resolving_static_and_explicit_emphasis_does_not_request_an_implicit_step() {
        let static_emphasis = parse("2-4", 3)
            .unwrap()
            .resolve(|_| panic!("static emphasis must not request a step"))
            .unwrap();
        assert_eq!(static_emphasis.mark_for_line(2), LineMark::Static);

        let explicit_emphasis = parse("1@2|3@1", 3)
            .unwrap()
            .resolve(|_| panic!("explicit emphasis must not request a step"))
            .unwrap();
        assert_eq!(stepped_values(explicit_emphasis.mark_for_line(1)), vec![2]);
        assert_eq!(stepped_values(explicit_emphasis.mark_for_line(3)), vec![1]);
    }
}
