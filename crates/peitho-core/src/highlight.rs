use std::{
    fs,
    path::{Path, PathBuf},
};

use syntect::{
    html::{line_tokens_to_classed_spans, ClassStyle, ClassedHTMLGenerator},
    parsing::{ParseState, ScopeStack, SyntaxDefinition, SyntaxSet},
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

    pub fn with_user_files(files: &[PathBuf]) -> Result<Self> {
        let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
        for file in files {
            builder.add(load_user_syntax_file(file)?);
        }
        Ok(Self {
            syntax_set: builder.build(),
        })
    }

    /// A fenced code block's language tag must resolve to a known syntax: the
    /// author asked for highlighting, so failing to honor the tag silently
    /// would be a silent drop. Blocks without a tag stay unhighlighted on
    /// purpose.
    pub(crate) fn validate_language(&self, token: &str, line: usize) -> Result<()> {
        if self.syntax_set.find_syntax_by_token(token).is_some() {
            return Ok(());
        }
        Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("unknown code language '{token}'"),
            format!(
                "use a language name syntect recognizes (e.g. {}) or remove the tag",
                self.example_tokens().join(", "),
            ),
        ))
    }

    fn example_tokens(&self) -> Vec<&'static str> {
        const PREFERRED: &[&str] = &[
            "rust", "js", "py", "sh", "toml", "json", "yaml", "html", "css", "md", "go", "c",
            "cpp", "java", "rb",
        ];
        PREFERRED
            .iter()
            .copied()
            .filter(|t| self.syntax_set.find_syntax_by_token(t).is_some())
            .collect()
    }

    /// Highlight `code` into one HTML string per source line.
    ///
    /// [`Self::highlight_html`] appends every line into a single
    /// `ClassedHTMLGenerator` buffer, so line boundaries vanish and syntect's
    /// outer scope span wraps the whole block — usable for a plain `<pre>`,
    /// but not for wrapping individual lines. Line emphasis needs each line to
    /// stand alone, so this drives `ParseState`/`ScopeStack` directly and
    /// closes the open scope stack at the end of every line, reopening it at
    /// the start of the next.
    ///
    /// Scopes routinely stay open across lines (multi-line strings, block
    /// comments), which is exactly why the close/reopen is required: without
    /// it a line span would inherit unbalanced `<span>` tags from its
    /// predecessor.
    pub(crate) fn highlight_lines(
        &self,
        code: &str,
        token: &str,
        line: usize,
    ) -> Result<Vec<String>> {
        let Some(syntax) = self.syntax_set.find_syntax_by_token(token) else {
            // Unreachable after parse-time validation, but stay a loud error.
            return self.validate_language(token, line).map(|()| Vec::new());
        };

        let fail = |err: &dyn std::fmt::Display| {
            BuildError::new(
                ErrorKind::Parse,
                Some(line),
                format!("failed to highlight {token} code: {err}"),
                "simplify the code block or remove the language tag",
            )
        };

        let mut parse_state = ParseState::new(syntax);
        let mut scope_stack = ScopeStack::new();
        let mut lines = Vec::new();

        for source_line in LinesWithEndings::from(code) {
            let ops = parse_state
                .parse_line(source_line, &self.syntax_set)
                .map_err(|err| fail(&err))?;

            // Reopen the scopes inherited from previous lines:
            // `line_tokens_to_classed_spans` emits tags only for scopes it
            // pushes on this line, so an inherited multi-line string or block
            // comment would otherwise lose its classes here.
            let inherited = scope_stack.as_slice().to_vec();
            let mut html = String::new();
            for scope in &inherited {
                html.push_str("<span class=\"");
                push_scope_classes(&mut html, *scope);
                html.push_str("\">");
            }

            let (line_html, _delta) = line_tokens_to_classed_spans(
                source_line,
                ops.as_slice(),
                CLASS_STYLE,
                &mut scope_stack,
            )
            .map_err(|err| fail(&err))?;
            html.push_str(line_html.trim_end_matches('\n'));

            // Close every span still open at end of line, which is exactly the
            // depth of the scope stack now: `line_html` leaves
            // `stack_after - stack_before` spans unclosed, and `stack_before`
            // more were reopened above. A line that *closes* an inherited scope
            // is already accounted for — syntect emitted that `</span>` inside
            // `line_html`, which is why the reopening has to come first.
            for _ in 0..scope_stack.len() {
                html.push_str("</span>");
            }

            lines.push(html);
        }

        Ok(lines)
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

/// Write a scope's atoms as `hl-`-prefixed classes.
///
/// Mirrors syntect's own (private) `scope_to_classes` for [`CLASS_STYLE`]:
/// a scope's `Display` is its dot-separated atoms, which is exactly the atom
/// sequence the class list needs. Reopening an inherited scope has to produce
/// the same classes syntect emitted when it first opened, or a multi-line
/// string would change color partway down the block.
fn push_scope_classes(out: &mut String, scope: syntect::parsing::Scope) {
    let ClassStyle::SpacedPrefixed { prefix } = CLASS_STYLE else {
        unreachable!("CLASS_STYLE is SpacedPrefixed");
    };
    for (index, atom) in scope.build_string().split('.').enumerate() {
        if atom.is_empty() {
            continue;
        }
        if index != 0 {
            out.push(' ');
        }
        out.push_str(prefix);
        out.push_str(atom);
    }
}

fn load_user_syntax_file(file: &Path) -> Result<SyntaxDefinition> {
    let source = fs::read_to_string(file).map_err(|err| {
        BuildError::new(
            ErrorKind::Parse,
            None,
            format!(
                "failed to load sublime-syntax file {}: {err}",
                file.display()
            ),
            "check the sublime-syntax file",
        )
    })?;
    SyntaxDefinition::load_from_str(
        &source,
        true,
        file.file_stem().and_then(|name| name.to_str()),
    )
    .map_err(|err| {
        BuildError::new(
            ErrorKind::Parse,
            None,
            format!(
                "failed to load sublime-syntax file {}: {err}",
                file.display()
            ),
            "check the sublime-syntax file",
        )
    })
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

    const DRIFT_SUBLIME_SYNTAX: &str = r#"%YAML 1.2
---
name: Drift
file_extensions: [drift]
scope: source.drift
contexts:
  main:
    - match: '\b(move|wait)\b'
      scope: keyword.control.drift
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
    fn help_text_only_suggests_tokens_the_default_set_recognizes() {
        let highlighter = Highlighter::defaults();
        let err = highlighter.validate_language("notalang", 1).unwrap_err();
        let msg = err.to_string();

        let list_start = msg.find("e.g. ").expect("help preamble present") + 5;
        let list_end = msg[list_start..].find(')').expect("help suffix present") + list_start;
        for raw in msg[list_start..list_end].split(',') {
            let token = raw.trim();
            assert!(
                highlighter.validate_language(token, 1).is_ok(),
                "help suggests '{token}' but the default set rejects it"
            );
        }
    }

    #[test]
    fn ts_stays_unrecognized_by_default_set_reminder() {
        // If this test fires, syntect's default set now includes TypeScript.
        // Add "ts" to PREFERRED in `example_tokens` so the help suggests it.
        let highlighter = Highlighter::defaults();
        assert!(
            highlighter.validate_language("ts", 1).is_err(),
            "syntect defaults now recognize 'ts' - add \"ts\" to PREFERRED in example_tokens"
        );
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
    fn user_files_validates_single_syntax_file() {
        let dir = tempfile::tempdir().unwrap();
        let syntax = dir.path().join("carina.sublime-syntax");
        fs::write(&syntax, CARINA_SUBLIME_SYNTAX).unwrap();

        let highlighter = Highlighter::with_user_files(&[syntax]).unwrap();

        assert!(highlighter.validate_language("carina", 1).is_ok());
        assert!(highlighter.validate_language("crn", 1).is_ok());
    }

    #[test]
    fn user_files_validates_multiple_syntax_files() {
        let dir = tempfile::tempdir().unwrap();
        let carina = dir.path().join("carina.sublime-syntax");
        let drift = dir.path().join("drift.sublime-syntax");
        fs::write(&carina, CARINA_SUBLIME_SYNTAX).unwrap();
        fs::write(&drift, DRIFT_SUBLIME_SYNTAX).unwrap();

        let highlighter = Highlighter::with_user_files(&[carina, drift]).unwrap();

        assert!(highlighter.validate_language("carina", 1).is_ok());
        assert!(highlighter.validate_language("drift", 1).is_ok());
    }

    #[test]
    fn malformed_user_syntax_file_returns_parse_error_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let syntax_path = dir.path().join("broken.sublime-syntax");
        fs::write(&syntax_path, ":::: not a syntax ::::").unwrap();

        let err = match Highlighter::with_user_files(std::slice::from_ref(&syntax_path)) {
            Ok(_) => panic!("malformed syntax unexpectedly loaded"),
            Err(err) => err,
        };

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, None);
        assert!(err.to_string().contains(&syntax_path.display().to_string()));
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
