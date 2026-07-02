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
    Ok(format!("{}\n\n{}", base_css.trim_end(), overrides_css.trim()))
}

fn validate_override_selectors(
    css: &str,
    known_keys: &BTreeSet<String>,
    template: &Template,
) -> Result<()> {
    let known_slots = template
        .slots
        .keys()
        .map(SlotName::class_name)
        .collect::<BTreeSet<_>>();

    for (line_index, line) in css.lines().enumerate() {
        let line_no = line_index + 1;
        let selector = line.split('{').next().unwrap_or(line);
        for key in extract_attr_values(selector, r#"[data-slide-key=""#, r#""]"#) {
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

fn extract_attr_values(selector: &str, prefix: &str, suffix: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = selector;
    while let Some(start) = rest.find(prefix) {
        let after_prefix = &rest[start + prefix.len()..];
        if let Some(end) = after_prefix.find(suffix) {
            values.push(after_prefix[..end].to_owned());
            rest = &after_prefix[end + suffix.len()..];
        } else {
            break;
        }
    }
    values
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
            r#"<slot name="title" accepts="inline" arity="1"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>"#,
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
            .slots
            .contains_key(&SlotName::new("code").unwrap()));
    }

    #[test]
    fn rejects_unknown_slide_key_in_override_selector() {
        let template = parse_template(
            "title",
            r#"<slot name="title" accepts="inline" arity="1"></slot>"#,
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
    fn rejects_unknown_slot_class_in_override_selector() {
        let template = parse_template(
            "title",
            r#"<slot name="title" accepts="inline" arity="1"></slot>"#,
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
