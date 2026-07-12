use std::collections::{BTreeMap, BTreeSet};

use crate::error::{BuildError, ErrorKind, Result};

/// One CSS source file; `name` appears in validation errors.
#[derive(Debug, Clone)]
pub struct CssFile {
    pub name: String,
    pub content: String,
}

struct StrippedCssFile<'a> {
    name: &'a str,
    content: String,
}

const PEITHO_SLIDE_CLASS: &str = "peitho-slide";

/// Concatenate the theme CSS files (already in load order) after validating
/// each with one uniform rule set:
///
/// - a selector containing `[data-slide-key=...]` must reference an existing
///   slide key, and any `.slot-*` class in it must exist in that slide's own
///   layout (`slide_slots`, see `Deck::<Checked>::slide_slot_classes`)
/// - a bare `.slot-*` class must exist in some provided layout
///   (`layout_slots`), which catches typos without breaking themes shared
///   across decks that use only a subset of the layouts
/// - bare root layout classes must not set `width` or `height` differently
///   from `.peitho-slide`, which owns the slide root's canvas sizing
/// - everything else is unrestricted theme CSS
pub fn build_theme_css(
    files: &[CssFile],
    slide_slots: &BTreeMap<String, BTreeSet<String>>,
    layout_slots: &BTreeSet<String>,
    root_classes: &BTreeSet<String>,
) -> Result<String> {
    let stripped_files = files
        .iter()
        .map(|file| StrippedCssFile {
            name: file.name.as_str(),
            content: strip_css_comments(&file.content),
        })
        .collect::<Vec<_>>();
    let root_classes = root_classes
        .iter()
        .filter(|class| class.as_str() != PEITHO_SLIDE_CLASS)
        .cloned()
        .collect::<BTreeSet<_>>();
    let slide_root_size_references = if root_classes.is_empty() {
        BTreeMap::new()
    } else {
        collect_peitho_slide_size_references(&stripped_files)
    };

    for file in &stripped_files {
        validate_override_selectors(&file.content, slide_slots, layout_slots).map_err(
            |mut err| {
                err.message = format!("{}: {}", file.name, err.message);
                err
            },
        )?;
        if !root_classes.is_empty() {
            validate_root_class_size_declarations(
                &file.content,
                &root_classes,
                &slide_root_size_references,
            )
            .map_err(|mut err| {
                err.message = format!("{}: {}", file.name, err.message);
                err
            })?;
        }
    }
    Ok(files
        .iter()
        .map(|file| file.content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SizeProperty {
    Width,
    Height,
}

impl SizeProperty {
    fn as_str(self) -> &'static str {
        match self {
            Self::Width => "width",
            Self::Height => "height",
        }
    }

    fn canvas_value(self) -> &'static str {
        match self {
            Self::Width => "var(--peitho-canvas-width, 1280px)",
            Self::Height => "var(--peitho-canvas-height, 720px)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CssSizeValue {
    normalized: String,
    display: String,
}

impl CssSizeValue {
    fn new(value: &str) -> Self {
        Self {
            normalized: normalize_css_value(value),
            display: value.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }
}

/// Blank out `/* ... */` comments while keeping newlines, so selector
/// validation neither trips over example selectors inside comments nor
/// shifts the line numbers it reports.
fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    let mut in_comment = false;
    let mut string = CssStringState::default();
    while let Some(c) = chars.next() {
        if in_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                out.push_str("  ");
                in_comment = false;
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
        } else if string.consume(c) {
            out.push(c);
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

fn collect_peitho_slide_size_references(
    files: &[StrippedCssFile<'_>],
) -> BTreeMap<SizeProperty, CssSizeValue> {
    let mut references = BTreeMap::new();
    for file in files {
        scan_css_size_declarations(
            &file.content,
            |selector| peitho_slide_reference_selector(selector).then_some(()),
            |(), property, value, _line_no| {
                references.insert(property, CssSizeValue::new(&value));
            },
        );
    }
    references
}

fn validate_root_class_size_declarations(
    css: &str,
    root_classes: &BTreeSet<String>,
    references: &BTreeMap<SizeProperty, CssSizeValue>,
) -> Result<()> {
    if root_classes.is_empty() {
        return Ok(());
    }

    let mut error = None;
    scan_css_size_declarations(
        css,
        |selector| bare_root_class_selector_member(selector, root_classes),
        |class, property, value, line_no| {
            let reference = references.get(&property);
            let normalized = normalize_css_value(&value);
            if error.is_none()
                && reference.map(|reference| reference.normalized.as_str())
                    != Some(normalized.as_str())
            {
                error = Some(root_class_size_error(
                    class,
                    property,
                    reference.map(|reference| reference.display.as_str()),
                    line_no,
                ));
            }
        },
    );

    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

enum CssScanMode<T> {
    Top,
    Style {
        target: Option<T>,
        declaration: String,
        declaration_line: Option<usize>,
        string: CssStringState,
        paren_depth: usize,
    },
    Nested {
        target: Option<T>,
        depth: usize,
        string: CssStringState,
    },
    Ignored {
        depth: usize,
        string: CssStringState,
    },
}

#[derive(Debug, Clone, Copy, Default)]
struct CssStringState {
    quote: Option<char>,
    escaped: bool,
}

impl CssStringState {
    fn consume(&mut self, ch: char) -> bool {
        if ch == '\n' {
            self.quote = None;
            self.escaped = false;
            return false;
        }
        if let Some(quote) = self.quote {
            if self.escaped {
                self.escaped = false;
            } else if ch == '\\' {
                self.escaped = true;
            } else if ch == quote {
                self.quote = None;
            }
            true
        } else if matches!(ch, '"' | '\'') {
            self.quote = Some(ch);
            true
        } else {
            false
        }
    }
}

fn scan_css_size_declarations<T, S, D>(css: &str, mut select_rule: S, mut on_declaration: D)
where
    S: FnMut(&str) -> Option<T>,
    D: FnMut(&T, SizeProperty, String, usize),
{
    let mut selector = String::new();
    let mut selector_string = CssStringState::default();
    let mut mode = CssScanMode::Top;
    let mut line_no = 1;

    for ch in css.chars() {
        let current_line = line_no;
        mode = match std::mem::replace(&mut mode, CssScanMode::Top) {
            CssScanMode::Top => {
                if selector_string.consume(ch) {
                    selector.push(ch);
                    CssScanMode::Top
                } else {
                    match ch {
                        '{' => {
                            let selector_text = selector.trim();
                            let mode = if selector_text.starts_with('@') {
                                CssScanMode::Ignored {
                                    depth: 1,
                                    string: CssStringState::default(),
                                }
                            } else {
                                CssScanMode::Style {
                                    target: select_rule(selector_text),
                                    declaration: String::new(),
                                    declaration_line: None,
                                    string: CssStringState::default(),
                                    paren_depth: 0,
                                }
                            };
                            selector.clear();
                            selector_string = CssStringState::default();
                            mode
                        }
                        ';' | '}' => {
                            selector.clear();
                            selector_string = CssStringState::default();
                            CssScanMode::Top
                        }
                        _ => {
                            selector.push(ch);
                            CssScanMode::Top
                        }
                    }
                }
            }
            CssScanMode::Style {
                target,
                mut declaration,
                mut declaration_line,
                mut string,
                mut paren_depth,
            } => {
                if string.consume(ch) {
                    push_declaration_char(
                        &mut declaration,
                        &mut declaration_line,
                        ch,
                        current_line,
                    );
                    CssScanMode::Style {
                        target,
                        declaration,
                        declaration_line,
                        string,
                        paren_depth,
                    }
                } else if paren_depth > 0 {
                    match ch {
                        '(' => paren_depth += 1,
                        ')' => paren_depth -= 1,
                        _ => {}
                    }
                    push_declaration_char(
                        &mut declaration,
                        &mut declaration_line,
                        ch,
                        current_line,
                    );
                    CssScanMode::Style {
                        target,
                        declaration,
                        declaration_line,
                        string,
                        paren_depth,
                    }
                } else {
                    match ch {
                        '(' => {
                            push_declaration_char(
                                &mut declaration,
                                &mut declaration_line,
                                ch,
                                current_line,
                            );
                            CssScanMode::Style {
                                target,
                                declaration,
                                declaration_line,
                                string,
                                paren_depth: 1,
                            }
                        }
                        '{' => CssScanMode::Nested {
                            target,
                            depth: 1,
                            string: CssStringState::default(),
                        },
                        ';' => {
                            emit_declaration(
                                &target,
                                &declaration,
                                declaration_line,
                                &mut on_declaration,
                            );
                            CssScanMode::Style {
                                target,
                                declaration: String::new(),
                                declaration_line: None,
                                string: CssStringState::default(),
                                paren_depth: 0,
                            }
                        }
                        '}' => {
                            emit_declaration(
                                &target,
                                &declaration,
                                declaration_line,
                                &mut on_declaration,
                            );
                            CssScanMode::Top
                        }
                        _ => {
                            push_declaration_char(
                                &mut declaration,
                                &mut declaration_line,
                                ch,
                                current_line,
                            );
                            CssScanMode::Style {
                                target,
                                declaration,
                                declaration_line,
                                string,
                                paren_depth,
                            }
                        }
                    }
                }
            }
            CssScanMode::Nested {
                target,
                mut depth,
                mut string,
            } => {
                if string.consume(ch) {
                    CssScanMode::Nested {
                        target,
                        depth,
                        string,
                    }
                } else {
                    match ch {
                        '{' => {
                            depth += 1;
                            CssScanMode::Nested {
                                target,
                                depth,
                                string,
                            }
                        }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                CssScanMode::Style {
                                    target,
                                    declaration: String::new(),
                                    declaration_line: None,
                                    string: CssStringState::default(),
                                    paren_depth: 0,
                                }
                            } else {
                                CssScanMode::Nested {
                                    target,
                                    depth,
                                    string,
                                }
                            }
                        }
                        _ => CssScanMode::Nested {
                            target,
                            depth,
                            string,
                        },
                    }
                }
            }
            CssScanMode::Ignored {
                mut depth,
                mut string,
            } => {
                if string.consume(ch) {
                    CssScanMode::Ignored { depth, string }
                } else {
                    match ch {
                        '{' => {
                            depth += 1;
                            CssScanMode::Ignored { depth, string }
                        }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                selector.clear();
                                selector_string = CssStringState::default();
                                CssScanMode::Top
                            } else {
                                CssScanMode::Ignored { depth, string }
                            }
                        }
                        _ => CssScanMode::Ignored { depth, string },
                    }
                }
            }
        };

        if ch == '\n' {
            line_no += 1;
        }
    }

    if let CssScanMode::Style {
        target,
        declaration,
        declaration_line,
        ..
    } = mode
    {
        emit_declaration(&target, &declaration, declaration_line, &mut on_declaration);
    }
}

fn push_declaration_char(
    declaration: &mut String,
    declaration_line: &mut Option<usize>,
    ch: char,
    line_no: usize,
) {
    if declaration_line.is_none() && !ch.is_whitespace() {
        *declaration_line = Some(line_no);
    }
    declaration.push(ch);
}

fn emit_declaration<T, D>(
    target: &Option<T>,
    declaration: &str,
    declaration_line: Option<usize>,
    on_declaration: &mut D,
) where
    D: FnMut(&T, SizeProperty, String, usize),
{
    let Some(target) = target else {
        return;
    };
    let Some(line_no) = declaration_line else {
        return;
    };
    let Some((property, value)) = declaration.trim().split_once(':') else {
        return;
    };
    let Some(property) = parse_size_property(property.trim()) else {
        return;
    };
    let value = value.trim().to_owned();
    if !value.is_empty() {
        on_declaration(target, property, value, line_no);
    }
}

fn parse_size_property(property: &str) -> Option<SizeProperty> {
    if property.eq_ignore_ascii_case("width") {
        Some(SizeProperty::Width)
    } else if property.eq_ignore_ascii_case("height") {
        Some(SizeProperty::Height)
    } else {
        None
    }
}

fn normalize_css_value(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn bare_root_class_selector_member(
    selector: &str,
    root_classes: &BTreeSet<String>,
) -> Option<String> {
    selector_class_members(selector)
        .find_map(|class| root_classes.contains(class).then(|| class.to_owned()))
}

fn peitho_slide_reference_selector(selector: &str) -> bool {
    selector_members(selector).into_iter().any(|member| {
        let member = member.trim();
        !member.is_empty()
            && !has_reference_selector_combinator_or_pseudo(member)
            && selector_member_classes(member).contains(&PEITHO_SLIDE_CLASS)
    })
}

fn has_reference_selector_combinator_or_pseudo(member: &str) -> bool {
    let mut string = CssStringState::default();
    member.chars().any(|ch| {
        if string.consume(ch) {
            false
        } else {
            ch.is_whitespace() || matches!(ch, '>' | '+' | '~' | ':')
        }
    })
}

fn selector_class_members(selector: &str) -> impl Iterator<Item = &str> {
    selector_members(selector)
        .into_iter()
        .filter_map(bare_selector_member_class)
}

fn selector_members(selector: &str) -> Vec<&str> {
    let mut members = Vec::new();
    let mut start = 0;
    let mut string = CssStringState::default();
    for (index, ch) in selector.char_indices() {
        if string.consume(ch) {
            continue;
        }
        if ch == ',' {
            members.push(&selector[start..index]);
            start = index + 1;
        }
    }
    members.push(&selector[start..]);
    members
}

fn bare_selector_member_class(member: &str) -> Option<&str> {
    let member = member.trim();
    let class = selector_member_classes(member).into_iter().next()?;
    (member.strip_prefix('.') == Some(class)).then_some(class)
}

fn selector_member_classes(member: &str) -> Vec<&str> {
    let mut classes = Vec::new();
    let mut string = CssStringState::default();
    for (index, ch) in member.char_indices() {
        if string.consume(ch) {
            continue;
        }
        if ch != '.' {
            continue;
        }
        let class_start = index + 1;
        let class = &member[class_start..];
        let class_len = css_class_ident_len(class);
        if class_len > 0 {
            classes.push(&class[..class_len]);
        }
    }
    classes
}

fn css_class_ident_len(value: &str) -> usize {
    value
        .char_indices()
        .take_while(|(_, ch)| is_css_ident_char(*ch))
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or(0)
}

fn is_css_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '-' | '_') || !ch.is_ascii()
}

fn root_class_size_error(
    class: &str,
    property: SizeProperty,
    reference: Option<&str>,
    line_no: usize,
) -> BuildError {
    let property_name = property.as_str();
    let help = match reference {
        Some(reference) => format!(
            "remove {property_name} (.peitho-slide sizes the slide root) or match .peitho-slide's {property_name} ({reference})"
        ),
        None => format!(
            "size the slide root on .peitho-slide instead ({property_name}: {}) and remove {property_name} from '.{class}'",
            property.canvas_value()
        ),
    };

    BuildError::new(
        ErrorKind::Theme,
        Some(line_no),
        format!(
            "root class '.{class}' sets {} on the slide root <section>",
            property_name
        ),
        help,
    )
}

fn validate_override_selectors(
    css: &str,
    slide_slots: &BTreeMap<String, BTreeSet<String>>,
    layout_slots: &BTreeSet<String>,
) -> Result<()> {
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
                        "use one of: {}; if this key belongs to a slide marked {{\"draft\":true}}, remove the override or the draft flag",
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
        build_with_root_classes(base, overrides, slide_slots, &[])
    }

    fn build_with_root_classes(
        base: &str,
        overrides: &str,
        slide_slots: &BTreeMap<String, BTreeSet<String>>,
        root_classes: &[&str],
    ) -> crate::error::Result<String> {
        let layout_slots: BTreeSet<String> = slide_slots.values().flatten().cloned().collect();
        let root_classes: BTreeSet<String> = root_classes
            .iter()
            .map(|class| (*class).to_owned())
            .collect();
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
            &root_classes,
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
        assert_eq!(
            err.help,
            r#"use one of: arch-1; if this key belongs to a slide marked {"draft":true}, remove the override or the draft flag"#
        );
    }

    #[test]
    fn unknown_slide_key_help_mentions_drafted_slides() {
        let markdown = "# Live\n\n---\n\
                        <!-- {\"key\":\"drafted\",\"draft\":true} -->\n# Drafted";
        let frontmatter = crate::parser::parse_frontmatter(markdown).unwrap();
        let parsed = crate::parser::parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let layout = crate::layout::parse_layout(
            "title-only",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let mapped = crate::mapping::map_by_convention(parsed, &layout).unwrap();
        let checked = crate::check::check_deck(mapped).unwrap();
        let slide_slots = checked.slide_slot_classes();
        let layout_slots: BTreeSet<String> = slide_slots.values().flatten().cloned().collect();

        let err = build_theme_css(
            &[CssFile {
                name: "overrides.css".to_owned(),
                content: r#"[data-slide-key="drafted"] .slot-title { color: red; }"#.to_owned(),
            }],
            &slide_slots,
            &layout_slots,
            &BTreeSet::new(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert!(err.to_string().contains("unknown slide key 'drafted'"));
        assert!(err.help.contains("draft"));
        assert!(err.help.contains("remove the override or the draft flag"));
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
        assert_eq!(
            err.help,
            r#"use one of: arch-1; if this key belongs to a slide marked {"draft":true}, remove the override or the draft flag"#
        );
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
        assert_eq!(
            err.help,
            r#"use one of: arch-1; if this key belongs to a slide marked {"draft":true}, remove the override or the draft flag"#
        );
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
            &BTreeSet::new(),
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

    #[test]
    fn rejects_root_class_width_after_peitho_slide_reference() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("overrides.css:"));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width on the slide root <section>"));
        assert_eq!(
            err.help,
            "remove width (.peitho-slide sizes the slide root) or match .peitho-slide's width (var(--peitho-canvas-width, 1280px))"
        );
    }

    #[test]
    fn rejects_root_class_height_before_peitho_slide_reference() {
        let err = build_with_root_classes(
            ".code-images {\n  height: 100%;\n}\n.peitho-slide { height: var(--peitho-canvas-height, 720px); }",
            "",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("base.css:"));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets height"));
        assert_eq!(
            err.help,
            "remove height (.peitho-slide sizes the slide root) or match .peitho-slide's height (var(--peitho-canvas-height, 720px))"
        );
    }

    #[test]
    fn allows_root_class_size_matching_peitho_slide_reference() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: var(--peitho-canvas-width, 1280px); }"));
    }

    #[test]
    fn allows_root_class_canvas_var_pattern_when_it_matches_reference() {
        let css = build_with_root_classes(
            ".peitho-slide {\n  height: var(--peitho-canvas-height, 720px);\n}",
            ".code-images {\n  height: var(--peitho-canvas-height, 720px);\n}",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains("height: var(--peitho-canvas-height, 720px);"));
    }

    #[test]
    fn allows_size_on_non_root_class() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".not-root { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".not-root { width: 100%; }"));
    }

    #[test]
    fn never_flags_peitho_slide_as_root_class_violation() {
        let css = build_with_root_classes(
            "",
            ".peitho-slide { width: 100%; height: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["peitho-slide"],
        )
        .unwrap();

        assert!(css.contains(".peitho-slide { width: 100%; height: 100%; }"));
    }

    #[test]
    fn non_ascii_non_root_selector_does_not_panic() {
        let css = build_with_root_classes(
            "",
            ".タイトル { color: red; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".タイトル { color: red; }"));
    }

    #[test]
    fn rejects_non_ascii_root_class_size() {
        let err = build_with_root_classes(
            ".peitho-slide { width: 1280px; }",
            ".タイトル { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["タイトル"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("root class '.タイトル' sets width"));
    }

    #[test]
    fn rejects_root_class_size_when_reference_is_in_another_file() {
        let err = build_with_root_classes(
            ".code-images { width: 100%; }",
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("base.css:"));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn uses_last_peitho_slide_reference_value() {
        let css = build_with_root_classes(
            ".peitho-slide { width: 100%; }\n.peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: var(--peitho-canvas-width, 1280px); }"));
    }

    #[test]
    fn peitho_slide_pseudo_element_does_not_override_root_size_reference() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }\n.peitho-slide::after { content: \"\"; width: 100%; }",
            ".code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
        assert_eq!(
            err.help,
            "remove width (.peitho-slide sizes the slide root) or match .peitho-slide's width (var(--peitho-canvas-width, 1280px))"
        );
    }

    #[test]
    fn peitho_slide_pseudo_class_is_not_root_size_reference() {
        let err = build_with_root_classes(
            ".peitho-slide:fullscreen { width: 100%; }",
            ".code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert_eq!(
            err.help,
            "size the slide root on .peitho-slide instead (width: var(--peitho-canvas-width, 1280px)) and remove width from '.code-images'"
        );
    }

    #[test]
    fn rejects_root_class_size_without_peitho_slide_reference_with_help() {
        let err = build_with_root_classes(
            "",
            ".code-images { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
        assert_eq!(
            err.help,
            "size the slide root on .peitho-slide instead (width: var(--peitho-canvas-width, 1280px)) and remove width from '.code-images'"
        );
    }

    #[test]
    fn root_class_size_help_quotes_peitho_slide_reference_value() {
        let err = build_with_root_classes(
            ".peitho-slide { width: 1280px; }",
            ".code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(
            err.help,
            "remove width (.peitho-slide sizes the slide root) or match .peitho-slide's width (1280px)"
        );
    }

    #[test]
    fn rejects_root_class_size_in_selector_list_member() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".not-root,\n.code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(2));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn rejects_root_class_size_after_import_statement_with_correct_line() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            "@import \"reset.css\";\n.code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(2));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn peitho_slide_reference_after_import_allows_matching_root_class_size() {
        let css = build_with_root_classes(
            "@import \"reset.css\";\n.peitho-slide { width: 1280px; }",
            ".code-images { width: 1280px; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: 1280px; }"));
    }

    #[test]
    fn rejects_root_class_size_after_charset_statement_with_correct_line() {
        let err = build_with_root_classes(
            ".peitho-slide { height: var(--peitho-canvas-height, 720px); }",
            "@charset \"utf-8\";\n.code-images { height: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(2));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets height"));
    }

    #[test]
    fn stray_apostrophe_before_root_size_violation_resets_at_newline() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".typo { font-family: Baz's Font;\n}\n.code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn stray_apostrophe_before_peitho_slide_reference_does_not_drop_reference() {
        let css = build_with_root_classes(
            ".typo { font-family: Baz's Font;\n}\n.peitho-slide { width: 1280px; }",
            ".code-images { width: 1280px; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: 1280px; }"));
    }

    #[test]
    fn rejects_outer_root_size_after_nested_css_block() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images {\n  &:hover { color: red; }\n  width: 100%;\n}",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn allows_width_inside_nested_block_on_root_class() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images { &:hover { width: 100%; } }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains("&:hover { width: 100%; }"));
    }

    #[test]
    fn allows_multiline_root_size_value_matching_reference() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images {\n  width:\n    var(--peitho-canvas-width, 1280px);\n}",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains("width:\n    var(--peitho-canvas-width, 1280px);"));
    }

    #[test]
    fn records_multiline_peitho_slide_reference_value() {
        let css = build_with_root_classes(
            ".peitho-slide {\n  width:\n    var(--peitho-canvas-width, 1280px);\n}",
            ".code-images { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: var(--peitho-canvas-width, 1280px); }"));
    }

    #[test]
    fn selector_commas_inside_strings_do_not_create_root_class_members() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            "[data-note=\",.code-images,\"] { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains("[data-note=\",.code-images,\"] { width: 100%; }"));
    }

    #[test]
    fn selector_commas_inside_strings_do_not_create_peitho_slide_reference() {
        let err = build_with_root_classes(
            "[data-x=\",.peitho-slide,\"] { width: 999px; }",
            ".code-images { width: 999px; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(
            err.help,
            "size the slide root on .peitho-slide instead (width: var(--peitho-canvas-width, 1280px)) and remove width from '.code-images'"
        );
    }

    #[test]
    fn ignores_media_block_with_quoted_closing_brace() {
        let css = build_with_root_classes(
            ".peitho-slide { height: var(--peitho-canvas-height, 720px); }",
            "@media print { .x::before { content: \"}\"; } .code-images { height: 100%; } }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains("@media print"));
    }

    #[test]
    fn rejects_root_size_after_quoted_brace_in_declaration_value() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images { background: url(\"a}b.png\"); width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn url_function_open_brace_does_not_hide_later_root_size_violation() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".x { background: url(a{b.png); }\n.code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(2));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn url_function_close_brace_does_not_hide_later_root_size_violation() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".x { background: url(a}b.png); }\n.code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(2));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn paren_value_compares_after_nested_parentheses() {
        let css = build_with_root_classes(
            ".peitho-slide { width: calc((100% - 10px)); }",
            ".code-images { width: calc((100% - 10px)); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: calc((100% - 10px)); }"));
    }

    #[test]
    fn compound_peitho_slide_reference_without_combinator_is_collected() {
        let css = build_with_root_classes(
            "section.peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".cover { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["cover"],
        )
        .unwrap();

        assert!(css.contains(".cover { width: var(--peitho-canvas-width, 1280px); }"));
    }

    #[test]
    fn peitho_slide_reference_selector_ignores_whitespace_inside_attribute_string() {
        let css = build_with_root_classes(
            ".peitho-slide[data-x=\"a b\"] { width: var(--peitho-canvas-width, 1280px); }",
            ".cover { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["cover"],
        )
        .unwrap();

        assert!(css.contains(".cover { width: var(--peitho-canvas-width, 1280px); }"));
    }

    #[test]
    fn peitho_slide_reference_with_combinator_is_not_collected() {
        let err = build_with_root_classes(
            ".wrap .peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".cover { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["cover"],
        )
        .unwrap_err();

        assert_eq!(
            err.help,
            "size the slide root on .peitho-slide instead (width: var(--peitho-canvas-width, 1280px)) and remove width from '.cover'"
        );
    }

    #[test]
    fn root_size_value_comparison_removes_all_whitespace() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images { width: var(--peitho-canvas-width,1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: var(--peitho-canvas-width,1280px); }"));
    }

    #[test]
    fn ignores_custom_property_and_width_suffixed_property_on_root_class() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images { --width: 100%; border-width: 4px; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains("--width: 100%"));
        assert!(css.contains("border-width: 4px"));
    }

    #[test]
    fn cross_file_last_peitho_slide_reference_value_allows_last_value() {
        let css = build_with_root_classes(
            ".peitho-slide { width: 111px; }",
            ".peitho-slide { width: 222px; }\n.code-images { width: 222px; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: 222px; }"));
    }

    #[test]
    fn cross_file_last_peitho_slide_reference_value_rejects_earlier_value() {
        let err = build_with_root_classes(
            ".peitho-slide { width: 111px; }",
            ".peitho-slide { width: 222px; }\n.code-images { width: 111px; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(2));
        assert_eq!(
            err.help,
            "remove width (.peitho-slide sizes the slide root) or match .peitho-slide's width (222px)"
        );
    }

    #[test]
    fn rejects_important_root_size_even_when_value_matches_reference() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".code-images { width: var(--peitho-canvas-width, 1280px) !important; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert_eq!(
            err.help,
            "remove width (.peitho-slide sizes the slide root) or match .peitho-slide's width (var(--peitho-canvas-width, 1280px))"
        );
    }

    #[test]
    fn rejects_unclosed_root_class_size_at_eof() {
        let err = build_with_root_classes(
            ".peitho-slide { width: 1280px; }",
            ".code-images { width: 100%",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn root_class_comment_stripping_keeps_declaration_line_number() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            "/* .code-images { width: 100%; }\n   still comment */\n.code-images {\n  width: 100%;\n}",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Theme);
        assert_eq!(err.line, Some(4));
        assert!(err.to_string().contains("overrides.css:"));
        assert_eq!(
            err.help,
            "remove width (.peitho-slide sizes the slide root) or match .peitho-slide's width (var(--peitho-canvas-width, 1280px))"
        );
    }

    #[test]
    fn comment_opener_inside_string_does_not_disable_root_size_lint() {
        let err = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".x::before { content: \"/*\"; }\n.code-images { width: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap_err();

        assert_eq!(err.line, Some(2));
        assert!(err
            .to_string()
            .contains("root class '.code-images' sets width"));
    }

    #[test]
    fn comment_after_closed_string_still_strips_root_size_example() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); }",
            ".x::before { content: \"ok\"; }\n/* .code-images { width: 100%; } */\n.code-images { width: var(--peitho-canvas-width, 1280px); }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains(".code-images { width: var(--peitho-canvas-width, 1280px); }"));
    }

    #[test]
    fn compound_keyed_and_media_root_selectors_are_out_of_scope() {
        let css = build_with_root_classes(
            ".peitho-slide { width: var(--peitho-canvas-width, 1280px); height: var(--peitho-canvas-height, 720px); }",
            "section.code-images { width: 100%; }\n.code-images.active { width: 100%; }\n[data-slide-key=\"arch-1\"] .code-images { width: 100%; }\n@media print {\n  .code-images { height: 100%; }\n}\n.code-images { min-width: 100%; max-height: 100%; }",
            &slots(&[("arch-1", &["slot-title"])]),
            &["code-images"],
        )
        .unwrap();

        assert!(css.contains("@media print"));
        assert!(css.contains("min-width: 100%"));
    }
}
