use std::path::Path;

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

pub struct Highlighter {
    syntax_set: SyntaxSet,
}

impl Highlighter {
    pub fn defaults() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
        }
    }

    pub fn with_user_dir(dir: &Path) -> Result<Self> {
        let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
        builder.add_from_folder(dir, true).map_err(|err| {
            BuildError::new(
                ErrorKind::Parse,
                None,
                format!("failed to load sublime-syntax file: {err}"),
                "check the sublime-syntax file",
            )
        })?;
        Ok(Self {
            syntax_set: builder.build(),
        })
    }

    /// A fenced code block's language tag must resolve to a known syntax: the
    /// author asked for highlighting, so failing to honor the tag silently
    /// would be a silent drop. Blocks without a tag stay unhighlighted on
    /// purpose.
    pub fn validate_language(&self, token: &str, line: usize) -> Result<()> {
        if self.syntax_set.find_syntax_by_token(token).is_some() {
            return Ok(());
        }
        Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("unknown code language '{token}'"),
            "use a language name syntect recognizes (e.g. rust, js, ts, py, sh, toml, json) or remove the tag",
        ))
    }

    pub(crate) fn highlight_html(&self, code: &str, token: &str, line: usize) -> Result<String> {
        let Some(syntax) = self.syntax_set.find_syntax_by_token(token) else {
            // Unreachable after parse-time validation, but stay a loud error.
            return self.validate_language(token, line).map(|()| String::new());
        };
        let mut generator =
            ClassedHTMLGenerator::new_with_class_style(syntax, &self.syntax_set, CLASS_STYLE);
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
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const CARINA_SUBLIME_SYNTAX: &str = r#"%YAML 1.2
---
name: Carina
file_extensions: [crn]
scope: source.carina
contexts:
  main:
    - match: '\b(resource|provider|module)\b'
      scope: keyword.control.carina
"#;

    #[test]
    fn known_language_tokens_validate() {
        let highlighter = Highlighter::defaults();

        assert!(highlighter.validate_language("rust", 1).is_ok());
        assert!(highlighter.validate_language("rs", 1).is_ok());
        assert!(highlighter.validate_language("js", 1).is_ok());
    }

    #[test]
    fn unknown_language_token_is_an_error_with_line() {
        let highlighter = Highlighter::defaults();
        let err = highlighter.validate_language("notalang", 7).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(7));
        assert!(err.to_string().contains("unknown code language 'notalang'"));
    }

    #[test]
    fn highlights_rust_with_prefixed_classes() {
        let highlighter = Highlighter::defaults();
        let html = highlighter
            .highlight_html("fn main() {}", "rust", 1)
            .unwrap();

        assert!(html.contains("hl-"));
        assert!(html.contains("fn"));
        assert!(!html.contains("style="));
    }

    #[test]
    fn user_dir_validates_carina_and_defaults_reject_it() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("carina.sublime-syntax"),
            CARINA_SUBLIME_SYNTAX,
        )
        .unwrap();

        let highlighter = Highlighter::with_user_dir(dir.path()).unwrap();

        assert!(highlighter.validate_language("carina", 1).is_ok());
        assert!(Highlighter::defaults()
            .validate_language("carina", 1)
            .is_err());
    }

    #[test]
    fn malformed_user_syntax_returns_parse_error_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let syntax_path = dir.path().join("broken.sublime-syntax");
        fs::write(&syntax_path, ":::: not a syntax ::::").unwrap();

        let err = match Highlighter::with_user_dir(dir.path()) {
            Ok(_) => panic!("malformed syntax unexpectedly loaded"),
            Err(err) => err,
        };

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, None);
        assert!(err.to_string().contains(&syntax_path.display().to_string()));
    }

    #[test]
    fn empty_user_syntax_dir_is_ok_and_keeps_defaults() {
        let dir = tempfile::tempdir().unwrap();

        let highlighter = Highlighter::with_user_dir(dir.path()).unwrap();

        assert!(highlighter.validate_language("rust", 1).is_ok());
    }

    #[test]
    fn nonexistent_user_syntax_dir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");

        assert!(Highlighter::with_user_dir(&missing).is_err());
    }

    #[test]
    fn highlights_carina_with_user_syntax_classes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("carina.sublime-syntax"),
            CARINA_SUBLIME_SYNTAX,
        )
        .unwrap();
        let highlighter = Highlighter::with_user_dir(dir.path()).unwrap();

        let html = highlighter
            .highlight_html(r#"resource "aws_s3_bucket" "site" {}"#, "carina", 1)
            .unwrap();

        assert!(html.contains("hl-"));
        assert!(html.contains("resource"));
    }
}
