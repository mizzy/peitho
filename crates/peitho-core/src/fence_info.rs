//! Typed parsing for fenced-code info strings.
//!
//! The positional language, optional line-emphasis group, code title, and
//! renderer-owned options are recognized in one pass. Consumers receive
//! typed attributes and never reinterpret a raw tail.

use crate::error::{BuildError, ErrorKind, Result};

/// A validated, display-ready title for one authored code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeTitle(String);

impl CodeTitle {
    fn new(value: &str) -> Option<Self> {
        let value = value.trim();
        (!value.is_empty()).then(|| Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FenceInfo<'a> {
    pub(crate) language: Option<&'a str>,
    pub(crate) attributes: FenceAttributes<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FenceAttributes<'a> {
    pub(crate) emphasis: Option<&'a str>,
    pub(crate) title: Option<CodeTitle>,
    pub(crate) options: Vec<FenceOption<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FenceOption<'a> {
    pub(crate) key: &'a str,
    pub(crate) value: Option<&'a str>,
    pub(crate) raw: &'a str,
}

/// Split the infallible positional language token from the attribute tail.
pub(crate) fn split_language(info: &str) -> (Option<&str>, &str) {
    let trimmed = info.trim();
    if trimmed.is_empty() {
        return (None, "");
    }

    if trimmed.starts_with('{') {
        (None, trimmed)
    } else {
        match trimmed.find(char::is_whitespace) {
            Some(split) => {
                let (token, rest) = trimmed.split_at(split);
                (Some(token), rest)
            }
            None => (Some(trimmed), ""),
        }
    }
}

/// Parse the complete post-language tail into typed fence attributes.
pub(crate) fn parse_attributes(mut rest: &str, line: usize) -> Result<FenceAttributes<'_>> {
    let mut emphasis = None;
    let mut title = None;
    let mut options = Vec::new();

    while !rest.trim_start().is_empty() {
        rest = rest.trim_start();
        if rest.starts_with('{') {
            if emphasis.is_some() {
                return Err(malformed_emphasis_error(line));
            }
            let (spec, remaining) = extract_braced_spec(rest, line)?;
            emphasis = Some(spec);
            rest = remaining;
            continue;
        }

        let (option, remaining) = extract_option(rest, line)?;
        rest = remaining;
        if option.key == "title" {
            if let Some(value) = option.value {
                if title.is_some() {
                    return Err(error(
                        line,
                        "duplicate code block title",
                        "keep only one title= option on this code fence",
                    ));
                }
                let Some(parsed_title) = CodeTitle::new(value) else {
                    return Err(error(
                        line,
                        "code block title cannot be empty",
                        "write a non-empty title, e.g. `title=\"main.rs\"`, or remove the option",
                    ));
                };
                title = Some(parsed_title);
                continue;
            }
        }
        options.push(option);
    }

    Ok(FenceAttributes {
        emphasis,
        title,
        options,
    })
}

fn extract_braced_spec(rest: &str, line: usize) -> Result<(&str, &str)> {
    let inner = rest
        .strip_prefix('{')
        .expect("extract_braced_spec is called only for a brace group");
    let Some(close) = inner.find('}') else {
        return Err(error(
            line,
            "unclosed emphasis spec",
            "close the emphasis spec with `}`, e.g. `{2-4}`",
        ));
    };
    let spec = &inner[..close];

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

    if spec.contains('{') {
        return Err(malformed_emphasis_error(line));
    }

    let remaining = &inner[close + 1..];
    if remaining.starts_with('{') {
        return Err(malformed_emphasis_error(line));
    }
    if remaining
        .chars()
        .next()
        .is_some_and(|next| !next.is_whitespace())
    {
        return Err(unexpected_info_tail_error(
            glued_attribute_token(rest, remaining),
            line,
        ));
    }

    Ok((spec, remaining))
}

fn extract_option(rest: &str, line: usize) -> Result<(FenceOption<'_>, &str)> {
    let first_whitespace = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let Some(equals) = rest.find('=').filter(|equals| *equals <= first_whitespace) else {
        let raw = &rest[..first_whitespace];
        return Ok((
            FenceOption {
                key: raw,
                value: None,
                raw,
            },
            &rest[first_whitespace..],
        ));
    };

    let key = &rest[..equals];
    let after_equals = &rest[equals + 1..];
    let (value, remaining) = if let Some(quoted) = after_equals.strip_prefix('"') {
        let Some(close) = quoted.find('"') else {
            return Err(error(
                line,
                "unclosed quoted fence option",
                "close the value with `\"`, e.g. `title=\"main.rs\"`",
            ));
        };
        let value = &quoted[..close];
        let remaining = &quoted[close + 1..];
        if !remaining.is_empty() && !remaining.chars().next().is_some_and(char::is_whitespace) {
            return Err(unexpected_info_tail_error(
                glued_attribute_token(rest, remaining),
                line,
            ));
        }
        (value, remaining)
    } else {
        let end = after_equals
            .find(char::is_whitespace)
            .unwrap_or(after_equals.len());
        let value = &after_equals[..end];
        let remaining = &after_equals[end..];
        if value.contains(['{', '}', '"']) {
            let raw = &rest[..rest.len() - remaining.len()];
            return Err(error(
                line,
                "unexpected text after the code language",
                format!(
                    "`{raw}` contains fence syntax in an unquoted value; quote the value or separate the next attribute with a space"
                ),
            ));
        }
        (value, remaining)
    };

    let raw = &rest[..rest.len() - remaining.len()];
    Ok((
        FenceOption {
            key,
            value: Some(value),
            raw,
        },
        remaining,
    ))
}

fn glued_attribute_token<'a>(rest: &'a str, remaining: &str) -> &'a str {
    let consumed = rest.len() - remaining.len();
    let mut quoted = false;
    let suffix = remaining
        .char_indices()
        .find_map(|(index, character)| match character {
            '"' => {
                quoted = !quoted;
                None
            }
            whitespace if whitespace.is_whitespace() && !quoted => Some(index),
            _ => None,
        })
        .unwrap_or(remaining.len());
    &rest[..consumed + suffix]
}

pub(crate) fn unexpected_info_tail_error(rest: &str, line: usize) -> BuildError {
    error(
        line,
        "unexpected text after the code language",
        format!(
            "`{rest}` is not a supported fence attribute; write line emphasis as `{{2-4}}` or a title as `title=\"...\"`"
        ),
    )
}

fn malformed_emphasis_error(line: usize) -> BuildError {
    error(
        line,
        "malformed emphasis spec",
        "write a single brace group, e.g. `{2-4}` or `{1|3}`",
    )
}

fn error(line: usize, message: impl Into<String>, help: impl Into<String>) -> BuildError {
    BuildError::new(ErrorKind::Parse, Some(line), message, help)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    fn parse(info: &str) -> FenceInfo<'_> {
        let (language, rest) = split_language(info);
        FenceInfo {
            language,
            attributes: parse_attributes(rest, 3).unwrap(),
        }
    }

    fn parse_error(info: &str, line: usize) -> BuildError {
        let (_, rest) = split_language(info);
        parse_attributes(rest, line).unwrap_err()
    }

    #[test]
    fn language_splitting_is_infallible_and_options_keep_exact_raw_tokens() {
        let (language, rest) = split_language(r#"rust bare foo="bar" label="two words""#);
        let attributes = parse_attributes(rest, 3).unwrap();

        assert_eq!(language, Some("rust"));
        assert_eq!(
            attributes.options,
            vec![
                FenceOption {
                    key: "bare",
                    value: None,
                    raw: "bare",
                },
                FenceOption {
                    key: "foo",
                    value: Some("bar"),
                    raw: r#"foo="bar""#,
                },
                FenceOption {
                    key: "label",
                    value: Some("two words"),
                    raw: r#"label="two words""#,
                },
            ]
        );
    }

    #[test]
    fn parses_existing_language_and_emphasis_shapes() {
        let plain = parse("rust");
        assert_eq!(plain.language, Some("rust"));
        assert_eq!(plain.attributes.emphasis, None);
        assert_eq!(plain.attributes.title, None);
        assert!(plain.attributes.options.is_empty());

        let tagged = parse("rust {2-4}");
        assert_eq!(tagged.language, Some("rust"));
        assert_eq!(tagged.attributes.emphasis, Some("2-4"));

        let extra_whitespace = parse("  rust  {1|3}  ");
        assert_eq!(extra_whitespace.language, Some("rust"));
        assert_eq!(extra_whitespace.attributes.emphasis, Some("1|3"));

        let untagged = parse("{2-4}");
        assert_eq!(untagged.language, None);
        assert_eq!(untagged.attributes.emphasis, Some("2-4"));

        let empty = parse("");
        assert_eq!(empty.language, None);
        assert_eq!(empty.attributes.emphasis, None);
        assert_eq!(empty.attributes.title, None);
        assert!(empty.attributes.options.is_empty());
    }

    #[test]
    fn title_and_emphasis_are_order_independent() {
        let emphasis_first = parse(r#"ts {2} title="app.ts""#);
        let title_first = parse(r#"ts title="app.ts" {2}"#);

        assert_eq!(emphasis_first.language, Some("ts"));
        assert_eq!(emphasis_first.attributes.emphasis, Some("2"));
        assert_eq!(
            emphasis_first
                .attributes
                .title
                .as_ref()
                .map(|title| title.as_str()),
            Some("app.ts")
        );
        assert_eq!(emphasis_first, title_first);
    }

    #[test]
    fn quoted_title_may_contain_spaces_and_is_trimmed() {
        let info = parse(r#"hcl title="  network main.tf  ""#);

        assert_eq!(
            info.attributes.title.as_ref().map(|title| title.as_str()),
            Some("network main.tf")
        );
    }

    #[test]
    fn bare_title_value_is_accepted() {
        let info = parse("hcl title=main.tf");

        assert_eq!(
            info.attributes.title.as_ref().map(|title| title.as_str()),
            Some("main.tf")
        );
    }

    #[test]
    fn attributes_must_be_separated_by_whitespace() {
        for info in [
            "rust title=main.rs{2}",
            r#"rust title="a"{2}"#,
            "rust {2}title=a",
            r#"rust {2}title="two words""#,
        ] {
            let err = parse_error(info, 13);
            assert_eq!(err.kind, ErrorKind::Parse, "info {info:?}");
            assert_eq!(err.line, Some(13), "info {info:?}");
            assert_eq!(
                err.message, "unexpected text after the code language",
                "info {info:?}"
            );
            assert!(
                err.help.contains(info.strip_prefix("rust ").unwrap()),
                "info {info:?}, help {:?}",
                err.help
            );
        }

        let err = parse_error("rust title=main.rs{2}", 13);
        assert!(err.help.contains("quote"), "{}", err.help);
        assert!(err.help.contains("space"), "{}", err.help);
    }

    #[test]
    fn quoted_values_may_contain_braces() {
        let info = parse(r#"rust title="main{2}.rs""#);

        assert_eq!(
            info.attributes.title.as_ref().map(|title| title.as_str()),
            Some("main{2}.rs")
        );
    }

    #[test]
    fn unquoted_values_reject_every_fence_syntax_character() {
        for info in [
            "rust title=main{2}",
            "rust title=main}2",
            "rust title=main\"rs",
        ] {
            let err = parse_error(info, 14);
            assert_eq!(err.kind, ErrorKind::Parse, "info {info:?}");
            assert_eq!(err.line, Some(14), "info {info:?}");
            assert_eq!(
                err.message, "unexpected text after the code language",
                "info {info:?}"
            );
            assert!(err.help.contains("quote"), "info {info:?}: {err}");
            assert!(err.help.contains("space"), "info {info:?}: {err}");
        }
    }

    #[test]
    fn non_title_options_pass_through_in_source_order() {
        let info = parse(r#"embed provider="x dot com" mode=card theme=dark"#);

        assert_eq!(
            info.attributes.options,
            vec![
                FenceOption {
                    key: "provider",
                    value: Some("x dot com"),
                    raw: r#"provider="x dot com""#,
                },
                FenceOption {
                    key: "mode",
                    value: Some("card"),
                    raw: "mode=card",
                },
                FenceOption {
                    key: "theme",
                    value: Some("dark"),
                    raw: "theme=dark",
                },
            ]
        );
    }

    #[test]
    fn empty_and_duplicate_titles_are_line_numbered_errors() {
        for info in [r#"rust title="""#, r#"rust title="   ""#, "rust title="] {
            let err = parse_error(info, 7);
            assert_eq!(err.kind, ErrorKind::Parse, "info {info:?}");
            assert_eq!(err.line, Some(7), "info {info:?}");
            assert_eq!(
                err.message, "code block title cannot be empty",
                "info {info:?}"
            );
            assert!(!err.help.is_empty(), "info {info:?}");
        }

        let err = parse_error(r#"rust title=one title="two""#, 8);
        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(8));
        assert_eq!(err.message, "duplicate code block title");
        assert!(!err.help.is_empty());
    }

    #[test]
    fn unclosed_quoted_value_is_a_line_numbered_error() {
        let err = parse_error(r#"rust title="main file.rs"#, 11);

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(11));
        assert_eq!(err.message, "unclosed quoted fence option");
        assert!(!err.help.is_empty());
    }

    #[test]
    fn a_second_brace_group_is_malformed_emphasis() {
        for info in ["rust {2}{4}", "rust {2} {4}"] {
            let err = parse_error(info, 9);
            assert_eq!(err.kind, ErrorKind::Parse, "info {info:?}");
            assert_eq!(err.line, Some(9), "info {info:?}");
            assert_eq!(err.message, "malformed emphasis spec", "info {info:?}");
            assert!(!err.help.is_empty(), "info {info:?}");
        }
    }

    #[test]
    fn pandoc_attribute_blocks_name_themselves_in_the_error() {
        for info in ["{.rust}", "{=html}", "{#id .rust}"] {
            let err = parse_error(info, 5);
            assert_eq!(err.line, Some(5), "info {info:?}");
            assert_eq!(
                err.message, "Pandoc-style attribute blocks are not supported",
                "info {info:?}"
            );
            assert!(!err.help.is_empty(), "info {info:?}");
        }
    }

    #[test]
    fn malformed_existing_info_strings_keep_their_errors() {
        let unclosed = parse_error("rust {2-4", 9);
        assert_eq!(unclosed.kind, ErrorKind::Parse);
        assert_eq!(unclosed.line, Some(9));
        assert_eq!(unclosed.message, "unclosed emphasis spec");
        assert!(!unclosed.help.is_empty());
    }
}
