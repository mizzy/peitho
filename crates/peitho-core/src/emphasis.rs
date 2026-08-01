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
//! ```rust {2-4}        static: always emphasized, consumes no reveal steps
//! ```rust {2,5-7|9}    stepped: one reveal step per `|`-separated group
//! ```
//!
//! The `|` separator is the sole discriminator between the two modes. See
//! `docs/specs/2026-08-01-code-line-emphasis-design.md`.

use std::ops::RangeInclusive;

use crate::error::{BuildError, ErrorKind, Result};

/// Line emphasis groups for one code fragment.
///
/// `groups` is never empty. `stepped` records whether the author wrote `|`:
/// stepped emphasis consumes one reveal step per group, static emphasis
/// consumes none and is always visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineEmphasis {
    groups: Vec<LineGroup>,
    stepped: bool,
}

impl LineEmphasis {
    pub(crate) fn groups(&self) -> &[LineGroup] {
        &self.groups
    }

    pub(crate) fn stepped(&self) -> bool {
        self.stepped
    }

    /// The highest line number referenced by any group.
    ///
    /// Used to validate the spec against the block's actual line count: an
    /// emphasis pointing past the end of the block is a build error, never a
    /// silently ignored no-op.
    pub(crate) fn max_line(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|group| group.ranges.iter())
            .map(|range| *range.end())
            .max()
            .expect("LineEmphasis always has at least one group with one range")
    }

    /// The 0-based index of the group emphasizing `line`, if any.
    ///
    /// Groups are authored as an ordered sequence, and for stepped emphasis
    /// the index is the step offset. A line listed in more than one group
    /// resolves to the first — overlap is legal and means "emphasized again".
    pub(crate) fn group_of(&self, line: usize) -> Option<usize> {
        self.groups.iter().position(|group| group.contains(line))
    }
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

/// Parse the text between the braces of an emphasis spec.
///
/// Grammar:
///
/// ```text
/// spec  := group ("|" group)*
/// group := item ("," item)*
/// item  := N | N "-" M          (1-based, inclusive, N <= M)
/// ```
///
/// Every malformed shape is a line-numbered error: silently accepting a spec
/// that does not mean what the author wrote would send them on stage with the
/// wrong line emphasized.
pub(crate) fn parse_emphasis_spec(spec: &str, line: usize) -> Result<LineEmphasis> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(error(
            line,
            "empty emphasis spec",
            "list the lines to emphasize, e.g. `{2-4}`, or remove the braces",
        ));
    }

    let stepped = trimmed.contains('|');
    let mut groups = Vec::new();
    for raw_group in trimmed.split('|') {
        groups.push(parse_group(raw_group, line)?);
    }

    Ok(LineEmphasis { groups, stepped })
}

fn parse_group(raw: &str, line: usize) -> Result<LineGroup> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(error(
            line,
            "empty emphasis group",
            "every `|`-separated group needs at least one line, e.g. `{1|3}`",
        ));
    }

    let mut ranges = Vec::new();
    for raw_item in trimmed.split(',') {
        ranges.push(parse_item(raw_item, line)?);
    }
    Ok(LineGroup { ranges })
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
    if text.is_empty() {
        return Err(error(
            line,
            "incomplete emphasis range",
            format!("`{item}` is missing a line number; write a range as `2-4`"),
        ));
    }
    let value: usize = text.parse().map_err(|_| {
        error(
            line,
            "emphasis spec expects line numbers",
            format!("`{item}` is not a line number or range; write e.g. `2`, `2-4`, or `2,5-7`"),
        )
    })?;
    if value == 0 {
        return Err(error(
            line,
            "code line numbers start at 1",
            "the first line of a code block is line 1, not line 0",
        ));
    }
    Ok(value)
}

fn error(line: usize, message: impl Into<String>, help: impl Into<String>) -> BuildError {
    BuildError::new(ErrorKind::Parse, Some(line), message, help)
}

/// The two halves of a fence info string: the language tag and the emphasis
/// spec, either of which may be absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InfoString<'a> {
    pub(crate) language: Option<&'a str>,
    pub(crate) emphasis: Option<&'a str>,
}

/// Split a fence info string into its language token and emphasis spec.
///
/// The language is the first whitespace-delimited token unless it starts with
/// `{`, which claims the token for emphasis instead. Everything after the
/// language must be a single `{…}` group; trailing junk is an error rather
/// than something silently ignored.
///
/// Positional detection means peitho claims a leading `{…}` for itself, which
/// is where Pandoc puts *attributes* (```` ```{.rust} ````). Those decks
/// already fail today (the whole info string is read as a language name), but
/// the error must name the case so the author is not sent hunting for a
/// line-number bug.
pub(crate) fn split_info_string(info: &str, line: usize) -> Result<InfoString<'_>> {
    let trimmed = info.trim();
    if trimmed.is_empty() {
        return Ok(InfoString {
            language: None,
            emphasis: None,
        });
    }

    let (language, rest) = if trimmed.starts_with('{') {
        (None, trimmed)
    } else {
        match trimmed.find(char::is_whitespace) {
            Some(split) => {
                let (token, rest) = trimmed.split_at(split);
                (Some(token), rest.trim_start())
            }
            None => (Some(trimmed), ""),
        }
    };

    if rest.is_empty() {
        return Ok(InfoString {
            language,
            emphasis: None,
        });
    }

    let spec = extract_braced_spec(rest, line)?;
    Ok(InfoString {
        language,
        emphasis: Some(spec),
    })
}

fn extract_braced_spec(rest: &str, line: usize) -> Result<&str> {
    let Some(inner) = rest.strip_prefix('{') else {
        return Err(error(
            line,
            "unexpected text after the code language",
            format!("`{rest}` is not an emphasis spec; write line emphasis as `{{2-4}}`"),
        ));
    };

    let Some(spec) = inner.strip_suffix('}') else {
        return Err(error(
            line,
            "unclosed emphasis spec",
            "close the emphasis spec with `}`, e.g. `{2-4}`",
        ));
    };

    // Pandoc-flavored Markdown puts attributes where peitho puts emphasis:
    // ```{.rust} selects a language, ```{=html} an output format. Name the
    // case explicitly instead of letting it fall through to a confusing
    // "expects line numbers" error.
    let looks_like_pandoc = spec.trim_start().starts_with(['.', '=', '#']) || spec.contains('=');
    if looks_like_pandoc {
        return Err(error(
            line,
            "Pandoc-style attribute blocks are not supported",
            format!(
                "write the language bare (```` ```{} ````); braces are line emphasis, e.g. `{{2-4}}`",
                spec.trim_start_matches(['.', '=', '#']).trim(),
            ),
        ));
    }

    if spec.contains('{') || spec.contains('}') {
        return Err(error(
            line,
            "malformed emphasis spec",
            "write a single brace group, e.g. `{2-4}` or `{1|3}`",
        ));
    }

    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_lines(emphasis: &LineEmphasis, index: usize) -> Vec<usize> {
        emphasis.groups()[index].lines().collect()
    }

    #[test]
    fn parses_static_and_stepped_specs() {
        let e = parse_emphasis_spec("2-4", 3).unwrap();
        assert!(!e.stepped());
        assert_eq!(e.groups().len(), 1);
        assert_eq!(group_lines(&e, 0), vec![2, 3, 4]);

        let e = parse_emphasis_spec("2,5-7|9", 3).unwrap();
        assert!(e.stepped());
        assert_eq!(e.groups().len(), 2);
        assert_eq!(group_lines(&e, 0), vec![2, 5, 6, 7]);
        assert_eq!(group_lines(&e, 1), vec![9]);
    }

    #[test]
    fn parses_a_single_line_as_a_range() {
        let e = parse_emphasis_spec("3", 3).unwrap();
        assert!(!e.stepped());
        assert_eq!(group_lines(&e, 0), vec![3]);
    }

    #[test]
    fn tolerates_whitespace_around_items() {
        let e = parse_emphasis_spec(" 2 , 5 - 7 | 9 ", 3).unwrap();
        assert!(e.stepped());
        assert_eq!(group_lines(&e, 0), vec![2, 5, 6, 7]);
        assert_eq!(group_lines(&e, 1), vec![9]);
    }

    #[test]
    fn a_single_group_with_a_trailing_separator_is_still_stepped() {
        // `{2|}` is stepped notation with an empty second group: an error,
        // not a silent downgrade to static emphasis.
        let err = parse_emphasis_spec("2|", 3).unwrap_err();
        assert_eq!(err.message, "empty emphasis group");
    }

    #[test]
    fn emphasis_spec_errors_are_line_numbered() {
        for (spec, message) in [
            ("0", "code line numbers start at 1"),
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
            ("|2", "empty emphasis group"),
        ] {
            let err = parse_emphasis_spec(spec, 7).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Parse, "spec {spec:?}");
            assert_eq!(err.line, Some(7), "spec {spec:?}");
            assert_eq!(err.message, message, "spec {spec:?}");
            assert!(!err.help.is_empty(), "spec {spec:?}");
        }
    }

    #[test]
    fn an_absurd_line_number_errors_instead_of_panicking() {
        // Overflow must surface as a parse error, never a panic.
        let err = parse_emphasis_spec("99999999999999999999999999", 3).unwrap_err();
        assert_eq!(err.message, "emphasis spec expects line numbers");
    }

    #[test]
    fn max_line_reports_the_highest_referenced_line() {
        let e = parse_emphasis_spec("2,5-7|9", 3).unwrap();
        assert_eq!(e.max_line(), 9);

        let e = parse_emphasis_spec("12-14|3", 3).unwrap();
        assert_eq!(e.max_line(), 14);
    }

    #[test]
    fn splits_info_string_into_language_and_spec() {
        let split = |info| split_info_string(info, 3).unwrap();

        assert_eq!(
            split("rust"),
            InfoString {
                language: Some("rust"),
                emphasis: None
            }
        );
        assert_eq!(
            split("rust {2-4}"),
            InfoString {
                language: Some("rust"),
                emphasis: Some("2-4")
            }
        );
        assert_eq!(
            split("{2-4}"),
            InfoString {
                language: None,
                emphasis: Some("2-4")
            }
        );
        assert_eq!(
            split(""),
            InfoString {
                language: None,
                emphasis: None
            }
        );
        // Extra whitespace is tolerated; the language is the first token.
        assert_eq!(
            split("rust  {2-4}"),
            InfoString {
                language: Some("rust"),
                emphasis: Some("2-4")
            }
        );
        assert_eq!(
            split("  rust {1|3}  "),
            InfoString {
                language: Some("rust"),
                emphasis: Some("1|3")
            }
        );
    }

    #[test]
    fn pandoc_attribute_blocks_name_themselves_in_the_error() {
        for info in ["{.rust}", "{=html}", "{#id .rust}"] {
            let err = split_info_string(info, 5).unwrap_err();
            assert_eq!(err.line, Some(5), "info {info:?}");
            assert_eq!(
                err.message, "Pandoc-style attribute blocks are not supported",
                "info {info:?}"
            );
            assert!(!err.help.is_empty(), "info {info:?}");
        }
    }

    #[test]
    fn malformed_info_strings_are_line_numbered() {
        for (info, message) in [
            ("rust {2-4", "unclosed emphasis spec"),
            ("rust 2-4", "unexpected text after the code language"),
            ("rust {2}{4}", "malformed emphasis spec"),
        ] {
            let err = split_info_string(info, 9).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Parse, "info {info:?}");
            assert_eq!(err.line, Some(9), "info {info:?}");
            assert_eq!(err.message, message, "info {info:?}");
            assert!(!err.help.is_empty(), "info {info:?}");
        }
    }

    #[test]
    fn group_of_finds_the_first_group_containing_a_line() {
        let e = parse_emphasis_spec("1-3|5", 3).unwrap();
        assert_eq!(e.group_of(1), Some(0));
        assert_eq!(e.group_of(3), Some(0));
        assert_eq!(e.group_of(4), None);
        assert_eq!(e.group_of(5), Some(1));

        // Overlapping groups resolve to the first: "emphasized again" is
        // legal, and the first occurrence is what stepping starts from.
        let e = parse_emphasis_spec("1-5|3", 3).unwrap();
        assert_eq!(e.group_of(3), Some(0));
    }
}
