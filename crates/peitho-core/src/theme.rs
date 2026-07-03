use std::collections::{BTreeMap, BTreeSet};

use crate::error::{BuildError, ErrorKind, Result};

/// One CSS source file; `name` appears in validation errors.
#[derive(Debug, Clone)]
pub struct CssFile {
    pub name: String,
    pub content: String,
}

/// Concatenate the theme CSS files (already in load order) after validating
/// each with one uniform rule set:
///
/// - a selector containing `[data-slide-key=...]` must reference an existing
///   slide key, and any `.slot-*` class in it must exist in that slide's own
///   layout (`slide_slots`, see `Deck::<Checked>::slide_slot_classes`)
/// - a bare `.slot-*` class must exist in some provided layout
///   (`layout_slots`), which catches typos without breaking themes shared
///   across decks that use only a subset of the layouts
/// - everything else is unrestricted theme CSS
pub fn build_theme_css(
    files: &[CssFile],
    slide_slots: &BTreeMap<String, BTreeSet<String>>,
    layout_slots: &BTreeSet<String>,
) -> Result<String> {
    for file in files {
        validate_override_selectors(&file.content, slide_slots, layout_slots).map_err(
            |mut err| {
                err.message = format!("{}: {}", file.name, err.message);
                err
            },
        )?;
    }
    Ok(files
        .iter()
        .map(|file| file.content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// Blank out `/* ... */` comments while keeping newlines, so selector
/// validation neither trips over example selectors inside comments nor
/// shifts the line numbers it reports.
fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    let mut in_comment = false;
    while let Some(c) = chars.next() {
        if in_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                out.push_str("  ");
                in_comment = false;
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            out.push_str("  ");
            in_comment = true;
        } else {
            out.push(c);
        }
    }
    out
}

fn validate_override_selectors(
    css: &str,
    slide_slots: &BTreeMap<String, BTreeSet<String>>,
    layout_slots: &BTreeSet<String>,
) -> Result<()> {
    let css = &strip_css_comments(css);

    for (line_index, line) in css.lines().enumerate() {
        let line_no = line_index + 1;
        let selector = line.split('{').next().unwrap_or(line);
        let keys = extract_slide_key_values(selector, line_no)?;
        for key in &keys {
            if !slide_slots.contains_key(key) {
                return Err(BuildError::new(
                    ErrorKind::Theme,
                    Some(line_no),
                    format!("unknown slide key '{key}' in override selector"),
                    format!(
                        "use one of: {}",
                        slide_slots.keys().cloned().collect::<Vec<_>>().join(", ")
                    ),
                ));
            }
        }
        let scope = if keys.is_empty() {
            layout_slots.clone()
        } else {
            keys.iter()
                .filter_map(|key| slide_slots.get(key))
                .flatten()
                .cloned()
                .collect()
        };
        for class in extract_slot_classes(selector) {
            if !scope.contains(&class) {
                let context = if keys.is_empty() {
                    "in override selector".to_owned()
                } else {
                    format!("for slide '{}'", keys.join("', '"))
                };
                return Err(BuildError::new(
                    ErrorKind::Theme,
                    Some(line_no),
                    format!("unknown slot class '.{class}' {context}"),
                    format!(
                        "use one of: {}",
                        scope
                            .iter()
                            .map(|slot| format!(".{slot}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn extract_slot_classes(selector: &str) -> Vec<String> {
    selector
        .split('.')
        .skip(1)
        .filter_map(|tail| {
            tail.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .next()
        })
        .filter(|class| class.starts_with("slot-"))
        .map(str::to_owned)
        .collect()
}

fn extract_slide_key_values(selector: &str, line_no: usize) -> Result<Vec<String>> {
    const ATTR: &str = "[data-slide-key";
    let mut values = Vec::new();
    let mut offset = 0;

    while let Some(relative_start) = selector[offset..].find(ATTR) {
        let attr_start = offset + relative_start;
        let after_name_start = attr_start + ATTR.len();
        let after_name = &selector[after_name_start..];
        let Some(close_relative) = after_name.find(']') else {
            return Err(malformed_selector(line_no));
        };
        let close_index = after_name_start + close_relative;
        let body = &selector[after_name_start..close_index];
        let body = body.trim_start();

        if body.starts_with('-')
            || body
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric())
        {
            offset = after_name_start;
            continue;
        }

        let Some(value_part) = strip_attr_operator(body) else {
            if body.trim() == "]" || body.trim().is_empty() {
                offset = close_index + 1;
                continue;
            }
            return Err(malformed_selector(line_no));
        };

        let value = parse_attr_value(value_part, line_no)?;
        values.push(value);
        offset = close_index + 1;
    }

    Ok(values)
}

fn strip_attr_operator(body: &str) -> Option<&str> {
    let body = body.trim_start();
    for operator in ["~=", "|=", "^=", "$=", "*=", "="] {
        if let Some(rest) = body.strip_prefix(operator) {
            return Some(rest);
        }
    }
    None
}

fn parse_attr_value(value_part: &str, line_no: usize) -> Result<String> {
    let value_part = value_part.trim_start();
    if value_part.is_empty() {
        return Err(malformed_selector(line_no));
    }

    let mut chars = value_part.chars();
    if let Some(quote @ ('"' | '\'')) = chars.next() {
        let rest = chars.as_str();
        if let Some(end) = rest.find(quote) {
            return Ok(rest[..end].to_owned());
        }
        return Err(malformed_selector(line_no));
    }

    let value = value_part
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim();
    if value.is_empty() {
        Err(malformed_selector(line_no))
    } else {
        Ok(value.to_owned())
    }
}

fn malformed_selector(line_no: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Theme,
        Some(line_no),
        "malformed selector for data-slide-key",
        r#"write selectors like [data-slide-key="arch-1"] .slot-code"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    fn slots(entries: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        entries
            .iter()
            .map(|(key, classes)| {
                (
                    (*key).to_owned(),
                    classes.iter().map(|class| (*class).to_owned()).collect(),
                )
            })
            .collect()
    }

    /// base.css + overrides.css の2ファイル構成で、提供レイアウトの
    /// スロット和集合はスライドのものと同一という旧来相当のセットアップ。
    fn build(
        base: &str,
        overrides: &str,
        slide_slots: &BTreeMap<String, BTreeSet<String>>,
    ) -> crate::error::Result<String> {
        let layout_slots: BTreeSet<String> = slide_slots.values().flatten().cloned().collect();
        build_theme_css(
            &[
                CssFile {
                    name: "base.css".to_owned(),
                    content: base.to_owned(),
                },
                CssFile {
                    name: "overrides.css".to_owned(),
                    content: overrides.to_owned(),
                },
            ],
            slide_slots,
            &layout_slots,
        )
    }

    #[test]
    fn appends_valid_override_after_base_css() {
        let css = build(
            ".slot-code { color: black; }",
            r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
            &slots(&[("arch-1", &["slot-title", "slot-code"])]),
        )
        .unwrap();

        assert!(css.contains(".slot-code { color: black; }"));
        assert!(
            css.ends_with(r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#)
        );
    }

    #[test]
    fn ignores_selectors_inside_css_comments() {
        let css = build(
            "",
            "/* example: [data-slide-key=\"...\"] .slot-nope { } */\n/* spans\n[data-slide-key=\"also-ignored\"] .slot-ghost { }\nlines */\n[data-slide-key=\"arch-1\"] .slot-title { color: red; }",
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap();

        assert!(css.contains(r#"[data-slide-key="arch-1"] .slot-title { color: red; }"#));
    }

    #[test]
    fn reports_correct_line_number_after_multiline_comment() {
        let err = build(
            "",
            "/* comment\nstill comment */\n[data-slide-key=\"missing\"] .slot-title { color: red; }",
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap_err();

        assert_eq!(err.line, Some(3));
    }

    #[test]
    fn rejects_unknown_slide_key_in_override_selector() {
        let err = build(
            "",
            r#"[data-slide-key="missing"] .slot-title { color: red; }"#,
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown slide key 'missing'"));
        assert_eq!(err.help, "use one of: arch-1");
    }

    #[test]
    fn rejects_unknown_single_quoted_slide_key_in_override_selector() {
        let err = build(
            "",
            r#"[data-slide-key='missing'] .slot-title { color: red; }"#,
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown slide key 'missing'"));
        assert_eq!(err.help, "use one of: arch-1");
    }

    #[test]
    fn rejects_unknown_unquoted_slide_key_in_override_selector() {
        let err = build(
            "",
            r#"[data-slide-key=missing] .slot-title { color: red; }"#,
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown slide key 'missing'"));
        assert_eq!(err.help, "use one of: arch-1");
    }

    #[test]
    fn accepts_known_single_quoted_slide_key_in_override_selector() {
        let css = build(
            ".slot-title { color: black; }",
            r#"[data-slide-key='arch-1'] .slot-title { color: red; }"#,
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap();

        assert!(css.contains(r#"[data-slide-key='arch-1'] .slot-title"#));
    }

    #[test]
    fn rejects_malformed_slide_key_selector() {
        let err = build(
            "",
            r#"[data-slide-key='arch-1' .slot-title { color: red; }"#,
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("malformed selector"));
    }

    #[test]
    fn rejects_unknown_slot_class_in_override_selector() {
        let err = build(
            "",
            r#"[data-slide-key="arch-1"] .slot-code { color: red; }"#,
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown slot class '.slot-code'"));
        assert_eq!(err.help, "use one of: .slot-title");
    }

    #[test]
    fn keyed_selectors_are_validated_in_every_css_file() {
        let err = build(
            r#"[data-slide-key="missing"] .slot-title { color: red; }"#,
            "",
            &slots(&[("arch-1", &["slot-title"])]),
        )
        .unwrap_err();

        assert!(err.to_string().contains("base.css:"));
        assert!(err.to_string().contains("unknown slide key 'missing'"));
    }

    #[test]
    fn bare_slot_class_from_unused_provided_layout_is_allowed() {
        let slide_slots = slots(&[("cover", &["slot-title"])]);
        let layout_slots: BTreeSet<String> = ["slot-title", "slot-code"]
            .iter()
            .map(|class| (*class).to_owned())
            .collect();

        let css = build_theme_css(
            &[CssFile {
                name: "base.css".to_owned(),
                content: ".slot-code { color: red; }".to_owned(),
            }],
            &slide_slots,
            &layout_slots,
        )
        .unwrap();

        assert!(css.contains(".slot-code { color: red; }"));
    }

    #[test]
    fn keyed_selector_validates_against_that_slides_layout_only() {
        let deck_slots = slots(&[
            ("walkthrough", &["slot-title", "slot-code"]),
            ("cover", &["slot-title"]),
        ]);

        let err = build(
            "",
            r#"[data-slide-key="cover"] .slot-code { color: red; }"#,
            &deck_slots,
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("unknown slot class '.slot-code' for slide 'cover'"));

        let css = build("", ".slot-code { color: red; }", &deck_slots).unwrap();
        assert!(css.contains(".slot-code { color: red; }"));
    }
}
