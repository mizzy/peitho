use std::sync::OnceLock;

use syntect::{
    html::{ClassStyle, ClassedHTMLGenerator},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

use crate::error::{BuildError, ErrorKind, Result};

/// Highlight classes are scope atoms prefixed with `hl-` (e.g. `hl-keyword`,
/// `hl-string`, `hl-comment`), so themes color code from CSS and the prefix
/// cannot collide with layout or slot classes.
const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// A fenced code block's language tag must resolve to a known syntax: the
/// author asked for highlighting, so failing to honor the tag silently would
/// be a silent drop. Blocks without a tag stay unhighlighted on purpose.
pub fn validate_language(token: &str, line: usize) -> Result<()> {
    if syntax_set().find_syntax_by_token(token).is_some() {
        return Ok(());
    }
    Err(BuildError::new(
        ErrorKind::Parse,
        Some(line),
        format!("unknown code language '{token}'"),
        "use a language name syntect recognizes (e.g. rust, js, ts, py, sh, toml, json) or remove the tag",
    ))
}

pub(crate) fn highlight_html(code: &str, token: &str, line: usize) -> Result<String> {
    let set = syntax_set();
    let Some(syntax) = set.find_syntax_by_token(token) else {
        // Unreachable after parse-time validation, but stay a loud error.
        return validate_language(token, line).map(|()| String::new());
    };
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, set, CLASS_STYLE);
    for source_line in LinesWithEndings::from(code) {
        generator
            .parse_html_for_line_which_includes_newline(source_line)
            .map_err(|err| {
                BuildError::new(
                    ErrorKind::Parse,
                    Some(line),
                    format!("failed to highlight {token} code: {err}"),
                    "simplify the code block or remove the language tag",
                )
            })?;
    }
    Ok(generator.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_language_tokens_validate() {
        assert!(validate_language("rust", 1).is_ok());
        assert!(validate_language("rs", 1).is_ok());
        assert!(validate_language("js", 1).is_ok());
    }

    #[test]
    fn unknown_language_token_is_an_error_with_line() {
        let err = validate_language("notalang", 7).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(7));
        assert!(err.to_string().contains("unknown code language 'notalang'"));
    }

    #[test]
    fn highlights_rust_with_prefixed_classes() {
        let html = highlight_html("fn main() {}", "rust", 1).unwrap();

        assert!(html.contains("hl-"));
        assert!(html.contains("fn"));
        assert!(!html.contains("style="));
    }
}
