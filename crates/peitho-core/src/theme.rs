use std::collections::BTreeSet;

use crate::{
    domain::{SlideKey, SlotName},
    error::{BuildError, ErrorKind, Result},
    template::Template,
};

pub fn build_theme_css<'a>(
    base_css: &str,
    overrides_css: &str,
    slide_keys: impl Iterator<Item = &'a SlideKey>,
    template: &Template,
) -> Result<String> {
    let known_keys = slide_keys
        .map(|key| key.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    validate_override_selectors(overrides_css, &known_keys, template)?;
    Ok(format!(
        "{}\n\n{}",
        base_css.trim_end(),
        overrides_css.trim()
    ))
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
    known_keys: &BTreeSet<String>,
    template: &Template,
) -> Result<()> {
    let css = &strip_css_comments(css);
    let known_slots = template
        .slots()
        .keys()
        .map(SlotName::class_name)
        .collect::<BTreeSet<_>>();

    for (line_index, line) in css.lines().enumerate() {
        let line_no = line_index + 1;
        let selector = line.split('{').next().unwrap_or(line);
        for key in extract_slide_key_values(selector, line_no)? {
            if !known_keys.contains(&key) {
                return Err(BuildError::new(
                    ErrorKind::Theme,
                    Some(line_no),
                    format!("unknown slide key '{key}' in override selector"),
                    format!(
                        "use one of: {}",
                        known_keys.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                ));
            }
        }
        for class in extract_slot_classes(selector) {
            if !known_slots.contains(&class) {
                return Err(BuildError::new(
                    ErrorKind::Theme,
                    Some(line_no),
                    format!("unknown slot class '.{class}' in override selector"),
                    format!(
                        "use one of: {}",
                        known_slots
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
    use crate::{
        domain::{SlideKey, SlotName},
        error::ErrorKind,
        template::parse_template,
    };

    #[test]
    fn appends_valid_override_after_base_css() {
        let template = parse_template(
            "title-body-code",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];
        let css = build_theme_css(
            ".slot-code { color: black; }",
            r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
            keys.iter(),
            &template,
        )
        .unwrap();

        assert!(css.contains(".slot-code { color: black; }"));
        assert!(
            css.ends_with(r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#)
        );
        assert!(template
            .slots()
            .contains_key(&SlotName::new("code").unwrap()));
    }

    #[test]
    fn ignores_selectors_inside_css_comments() {
        let template = parse_template(
            "title",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];

        let css = build_theme_css(
            "",
            "/* example: [data-slide-key=\"...\"] .slot-nope { } */\n/* spans\n[data-slide-key=\"also-ignored\"] .slot-ghost { }\nlines */\n[data-slide-key=\"arch-1\"] .slot-title { color: red; }",
            keys.iter(),
            &template,
        )
        .unwrap();

        assert!(css.contains(r#"[data-slide-key="arch-1"] .slot-title { color: red; }"#));
    }

    #[test]
    fn reports_correct_line_number_after_multiline_comment() {
        let template = parse_template(
            "title",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];

        let err = build_theme_css(
            "",
            "/* comment\nstill comment */\n[data-slide-key=\"missing\"] .slot-title { color: red; }",
            keys.iter(),
            &template,
        )
        .unwrap_err();

        assert_eq!(err.line, Some(3));
    }

    #[test]
    fn rejects_unknown_slide_key_in_override_selector() {
        let template = parse_template(
            "title",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];

        let err = build_theme_css(
            "",
            r#"[data-slide-key="missing"] .slot-title { color: red; }"#,
            keys.iter(),
            &template,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown slide key 'missing'"));
        assert_eq!(err.help, "use one of: arch-1");
    }

    #[test]
    fn rejects_unknown_single_quoted_slide_key_in_override_selector() {
        let template = parse_template(
            "title",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];

        let err = build_theme_css(
            "",
            r#"[data-slide-key='missing'] .slot-title { color: red; }"#,
            keys.iter(),
            &template,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown slide key 'missing'"));
        assert_eq!(err.help, "use one of: arch-1");
    }

    #[test]
    fn rejects_unknown_unquoted_slide_key_in_override_selector() {
        let template = parse_template(
            "title",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];

        let err = build_theme_css(
            "",
            r#"[data-slide-key=missing] .slot-title { color: red; }"#,
            keys.iter(),
            &template,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown slide key 'missing'"));
        assert_eq!(err.help, "use one of: arch-1");
    }

    #[test]
    fn accepts_known_single_quoted_slide_key_in_override_selector() {
        let template = parse_template(
            "title",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];

        let css = build_theme_css(
            ".slot-title { color: black; }",
            r#"[data-slide-key='arch-1'] .slot-title { color: red; }"#,
            keys.iter(),
            &template,
        )
        .unwrap();

        assert!(css.contains(r#"[data-slide-key='arch-1'] .slot-title"#));
    }

    #[test]
    fn rejects_malformed_slide_key_selector() {
        let template = parse_template(
            "title",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];

        let err = build_theme_css(
            "",
            r#"[data-slide-key='arch-1' .slot-title { color: red; }"#,
            keys.iter(),
            &template,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("malformed selector"));
    }

    #[test]
    fn rejects_unknown_slot_class_in_override_selector() {
        let template = parse_template(
            "title",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];

        let err = build_theme_css(
            "",
            r#"[data-slide-key="arch-1"] .slot-code { color: red; }"#,
            keys.iter(),
            &template,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown slot class '.slot-code'"));
        assert_eq!(err.help, "use one of: .slot-title");
    }
}
