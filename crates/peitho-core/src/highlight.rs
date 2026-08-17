use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use sha2::{Digest, Sha256};
use syntect::{
    html::{line_tokens_to_classed_spans, ClassStyle, ClassedHTMLGenerator},
    parsing::{ParseState, ScopeStack, SyntaxDefinition, SyntaxSet},
    util::LinesWithEndings,
};
use walkdir::WalkDir;

use crate::error::{BuildError, ErrorKind, Result};

/// Highlight classes are scope atoms prefixed with `hl-` (e.g. `hl-keyword`,
/// `hl-string`, `hl-comment`), so themes color code from CSS and the prefix
/// cannot collide with layout or slot classes.
const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };

const PLAIN_TEXT_ALIAS_SUBLIME_SYNTAX: &str = r#"name: Plain Text
file_extensions:
  - text
  - plaintext
scope: text.plain
contexts:
  main: []
"#;

static BASE_SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();

type UserSyntaxCacheKey = [u8; 32];

static USER_SYNTAX_SET_CACHE: Mutex<Option<(UserSyntaxCacheKey, SyntaxSet)>> = Mutex::new(None);

#[cfg(test)]
static USER_SYNTAX_SET_BUILD_COUNT: AtomicUsize = AtomicUsize::new(0);

fn base_syntax_set() -> &'static SyntaxSet {
    BASE_SYNTAX_SET.get_or_init(|| {
        let mut builder = two_face::syntax::extra_newlines().into_builder();
        builder.add(
            SyntaxDefinition::load_from_str(PLAIN_TEXT_ALIAS_SUBLIME_SYNTAX, true, None)
                .expect("embedded plain-text alias syntax should be valid"),
        );
        builder.build()
    })
}

pub struct Highlighter {
    syntax_set: SyntaxSet,
}

impl Highlighter {
    pub fn defaults() -> Self {
        Self {
            syntax_set: base_syntax_set().clone(),
        }
    }

    pub fn with_user_dir(dir: &Path) -> Result<Self> {
        let files = user_syntax_files_in_dir(dir)?;
        Self::with_user_files_and_error_style(&files, UserSyntaxErrorStyle::Directory)
    }

    pub fn with_user_files(files: &[PathBuf]) -> Result<Self> {
        Self::with_user_files_and_error_style(files, UserSyntaxErrorStyle::ExplicitFile)
    }

    fn with_user_files_and_error_style(
        files: &[PathBuf],
        error_style: UserSyntaxErrorStyle,
    ) -> Result<Self> {
        let (key, syntaxes) = load_user_syntax_files(files, error_style)?;
        Ok(Self {
            syntax_set: cached_user_syntax_set(key, syntaxes),
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
            "rust", "js", "ts", "py", "sh", "toml", "json", "yaml", "html", "css", "md", "go", "c",
            "cpp", "java", "rb",
        ];
        PREFERRED
            .iter()
            .copied()
            .filter(|token| self.syntax_set.find_syntax_by_token(token).is_some())
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

#[derive(Clone, Copy)]
enum UserSyntaxErrorStyle {
    Directory,
    ExplicitFile,
}

fn user_syntax_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(dir)
        .follow_links(true)
        .sort_by(|a, b| a.file_name().cmp(b.file_name()))
    {
        let entry = entry.map_err(|err| {
            BuildError::new(
                ErrorKind::Parse,
                None,
                format!(
                    "failed to load sublime-syntax file: error finding all the files in a directory: {err}"
                ),
                "check the sublime-syntax file",
            )
        })?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "sublime-syntax")
        {
            files.push(entry.into_path());
        }
    }
    Ok(files)
}

fn load_user_syntax_files(
    files: &[PathBuf],
    error_style: UserSyntaxErrorStyle,
) -> Result<(UserSyntaxCacheKey, Vec<SyntaxDefinition>)> {
    let mut hasher = Sha256::new();
    let mut syntaxes = Vec::with_capacity(files.len());
    for file in files {
        let (source, syntax) = load_user_syntax_file(file, error_style)?;
        hash_user_syntax_component(&mut hasher, file.as_os_str().as_encoded_bytes());
        hash_user_syntax_component(&mut hasher, source.as_bytes());
        syntaxes.push(syntax);
    }
    Ok((hasher.finalize().into(), syntaxes))
}

fn hash_user_syntax_component(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn load_user_syntax_file(
    file: &Path,
    error_style: UserSyntaxErrorStyle,
) -> Result<(String, SyntaxDefinition)> {
    let source = fs::read_to_string(file).map_err(|err| {
        let message = match error_style {
            UserSyntaxErrorStyle::Directory => {
                format!("failed to load sublime-syntax file: error reading a file: {err}")
            }
            UserSyntaxErrorStyle::ExplicitFile => format!(
                "failed to load sublime-syntax file {}: {err}",
                file.display()
            ),
        };
        BuildError::new(
            ErrorKind::Parse,
            None,
            message,
            "check the sublime-syntax file",
        )
    })?;

    let syntax = SyntaxDefinition::load_from_str(
        &source,
        true,
        file.file_stem().and_then(|name| name.to_str()),
    )
    .map_err(|err| {
        let message = match error_style {
            UserSyntaxErrorStyle::Directory => format!(
                "failed to load sublime-syntax file: {}: {err}",
                file.display()
            ),
            UserSyntaxErrorStyle::ExplicitFile => format!(
                "failed to load sublime-syntax file {}: {err}",
                file.display()
            ),
        };
        BuildError::new(
            ErrorKind::Parse,
            None,
            message,
            "check the sublime-syntax file",
        )
    })?;

    Ok((source, syntax))
}

fn cached_user_syntax_set(key: UserSyntaxCacheKey, syntaxes: Vec<SyntaxDefinition>) -> SyntaxSet {
    let mut cache = USER_SYNTAX_SET_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((cached_key, syntax_set)) = cache.as_ref() {
        if cached_key == &key {
            return syntax_set.clone();
        }
    }

    let mut builder = base_syntax_set().clone().into_builder();
    for syntax in syntaxes {
        builder.add(syntax);
    }
    let syntax_set = builder.build();
    #[cfg(test)]
    USER_SYNTAX_SET_BUILD_COUNT.fetch_add(1, Ordering::Relaxed);
    *cache = Some((key, syntax_set));
    cache
        .as_ref()
        .expect("user syntax cache was just populated")
        .1
        .clone()
}

#[cfg(test)]
fn clear_user_syntax_set_cache() {
    *USER_SYNTAX_SET_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[cfg(test)]
fn user_syntax_set_build_count() -> usize {
    USER_SYNTAX_SET_BUILD_COUNT.load(Ordering::Relaxed)
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::MutexGuard};

    static USER_SYNTAX_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_user_syntax_tests() -> MutexGuard<'static, ()> {
        USER_SYNTAX_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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

    const UPDATED_CARINA_SUBLIME_SYNTAX: &str = r#"%YAML 1.2
---
name: Carina
file_extensions: [crn]
scope: source.carina
contexts:
  main:
    - match: '\b(resource|provider|module)\b'
      scope: string.quoted.carina
"#;

    const TEXT_OVERRIDE_SUBLIME_SYNTAX: &str = r#"%YAML 1.2
---
name: User Text
file_extensions: [text]
scope: source.usertext
contexts:
  main:
    - match: '\boverride\b'
      scope: keyword.control.usertext
"#;

    const TEXT_NAME_OVERRIDE_SUBLIME_SYNTAX: &str = r#"%YAML 1.2
---
name: Plain Text
file_extensions: [plaintext]
scope: source.usertextname
contexts:
  main:
    - match: '\boverride\b'
      scope: keyword.control.usertextname
"#;

    #[test]
    fn known_language_tokens_validate() {
        let highlighter = Highlighter::defaults();

        assert!(highlighter.validate_language("rust", 1).is_ok());
        assert!(highlighter.validate_language("rs", 1).is_ok());
        assert!(highlighter.validate_language("js", 1).is_ok());
    }

    #[test]
    fn bundled_language_tokens_validate_and_highlight() {
        let highlighter = Highlighter::defaults();

        for (token, code) in [
            ("typescript", "const answer: number = 42;"),
            ("ts", "const answer: number = 42;"),
            ("toml", "answer = 42"),
            ("dockerfile", "FROM rust:latest"),
        ] {
            highlighter
                .validate_language(token, 1)
                .unwrap_or_else(|err| panic!("{token} should validate: {err}"));

            let html = highlighter
                .highlight_html(code, token, 1)
                .unwrap_or_else(|err| panic!("{token} should highlight: {err}"));
            assert!(html.contains("<span class=\"hl-"), "{token}: {html}");

            let lines = highlighter
                .highlight_lines(code, token, 1)
                .unwrap_or_else(|err| panic!("{token} lines should highlight: {err}"));
            assert!(
                lines.iter().any(|line| line.contains("<span class=\"hl-")),
                "{token}: {lines:?}"
            );
        }
    }

    #[test]
    fn plain_text_aliases_validate_and_render_like_txt() {
        let highlighter = Highlighter::defaults();
        let code = "plain <text>\nsecond & line\n";
        let txt_html = highlighter.highlight_html(code, "txt", 1).unwrap();
        let txt_lines = highlighter.highlight_lines(code, "txt", 1).unwrap();

        for token in ["text", "plaintext"] {
            assert!(highlighter.validate_language(token, 1).is_ok(), "{token}");
            assert_eq!(
                highlighter.highlight_html(code, token, 1).unwrap(),
                txt_html,
                "{token} whole-block rendering differs from txt"
            );
            assert_eq!(
                highlighter.highlight_lines(code, token, 1).unwrap(),
                txt_lines,
                "{token} line rendering differs from txt"
            );
        }
    }

    #[test]
    fn plain_text_aliases_are_case_insensitive() {
        let syntax_set = base_syntax_set();
        let highlighter = Highlighter::defaults();

        for token in ["text", "Text", "TEXT", "plaintext", "PlainText"] {
            assert!(
                syntax_set.find_syntax_by_token(token).is_some(),
                "base syntax set should resolve {token}"
            );
            assert!(highlighter.validate_language(token, 1).is_ok(), "{token}");
        }
    }

    #[test]
    fn unknown_language_token_is_an_error_with_line() {
        let highlighter = Highlighter::defaults();
        let err = highlighter.validate_language("crn", 23).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(23));
        assert!(err.message.contains("unknown code language 'crn'"));
        assert!(err.help.contains("ts"), "{}", err.help);
        assert!(err.help.contains("toml"), "{}", err.help);
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
        let _guard = lock_user_syntax_tests();
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("carina.sublime-syntax"),
            CARINA_SUBLIME_SYNTAX,
        )
        .unwrap();

        let highlighter = Highlighter::with_user_dir(dir.path()).unwrap();

        assert!(highlighter.validate_language("carina", 1).is_ok());
        assert!(highlighter.validate_language("ts", 1).is_ok());
        assert!(Highlighter::defaults()
            .validate_language("carina", 1)
            .is_err());
    }

    #[test]
    fn user_files_validates_single_syntax_file() {
        let _guard = lock_user_syntax_tests();
        let dir = tempfile::tempdir().unwrap();
        let syntax = dir.path().join("carina.sublime-syntax");
        fs::write(&syntax, CARINA_SUBLIME_SYNTAX).unwrap();

        let highlighter = Highlighter::with_user_files(&[syntax]).unwrap();

        assert!(highlighter.validate_language("carina", 1).is_ok());
        assert!(highlighter.validate_language("crn", 1).is_ok());
        assert!(highlighter.validate_language("ts", 1).is_ok());
    }

    #[test]
    fn user_file_text_extension_shadows_bundled_alias() {
        let _guard = lock_user_syntax_tests();
        let dir = tempfile::tempdir().unwrap();
        let syntax = dir.path().join("user-text.sublime-syntax");
        fs::write(&syntax, TEXT_OVERRIDE_SUBLIME_SYNTAX).unwrap();
        let highlighter = Highlighter::with_user_files(&[syntax]).unwrap();

        let html = highlighter.highlight_html("override", "text", 1).unwrap();

        assert!(html.contains("hl-usertext"), "{html}");
    }

    #[test]
    fn user_syntax_name_collision_shadows_bundled_definition() {
        let _guard = lock_user_syntax_tests();
        let dir = tempfile::tempdir().unwrap();
        let syntax = dir.path().join("user-text-name.sublime-syntax");
        fs::write(&syntax, TEXT_NAME_OVERRIDE_SUBLIME_SYNTAX).unwrap();
        let highlighter = Highlighter::with_user_files(&[syntax]).unwrap();

        let html = highlighter
            .highlight_html("override", "plaintext", 1)
            .unwrap();

        assert!(html.contains("hl-usertextname"), "{html}");
    }

    #[test]
    fn user_files_validates_multiple_syntax_files() {
        let _guard = lock_user_syntax_tests();
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
    fn identical_user_syntax_content_reuses_cached_set() {
        let _guard = lock_user_syntax_tests();
        clear_user_syntax_set_cache();
        let dir = tempfile::tempdir().unwrap();
        let syntax = dir.path().join("carina.sublime-syntax");
        fs::write(&syntax, CARINA_SUBLIME_SYNTAX).unwrap();

        let builds_before = user_syntax_set_build_count();
        let first = Highlighter::with_user_files(std::slice::from_ref(&syntax)).unwrap();
        let builds_after_first = user_syntax_set_build_count();
        let second = Highlighter::with_user_files(std::slice::from_ref(&syntax)).unwrap();

        assert_eq!(builds_after_first, builds_before + 1);
        assert_eq!(user_syntax_set_build_count(), builds_after_first);
        assert!(first.validate_language("carina", 1).is_ok());
        assert!(second.validate_language("carina", 1).is_ok());
    }

    #[test]
    #[ignore = "manual release-mode cache measurement"]
    fn measure_user_syntax_cache_construction() {
        let _guard = lock_user_syntax_tests();
        let _ = Highlighter::defaults();
        clear_user_syntax_set_cache();
        let dir = tempfile::tempdir().unwrap();
        let syntax = dir.path().join("carina.sublime-syntax");
        fs::write(&syntax, CARINA_SUBLIME_SYNTAX).unwrap();

        let started = std::time::Instant::now();
        let first = Highlighter::with_user_files(std::slice::from_ref(&syntax)).unwrap();
        let first_elapsed = started.elapsed();
        let builds_after_first = user_syntax_set_build_count();

        let started = std::time::Instant::now();
        let second = Highlighter::with_user_files(std::slice::from_ref(&syntax)).unwrap();
        let second_elapsed = started.elapsed();

        assert!(first.validate_language("carina", 1).is_ok());
        assert!(second.validate_language("carina", 1).is_ok());
        assert_eq!(user_syntax_set_build_count(), builds_after_first);
        eprintln!("first construction: {first_elapsed:?}; second construction: {second_elapsed:?}");
    }

    #[test]
    fn changed_user_syntax_content_rebuilds_cached_set() {
        let _guard = lock_user_syntax_tests();
        clear_user_syntax_set_cache();
        let dir = tempfile::tempdir().unwrap();
        let syntax = dir.path().join("carina.sublime-syntax");
        fs::write(&syntax, CARINA_SUBLIME_SYNTAX).unwrap();

        let first = Highlighter::with_user_dir(dir.path()).unwrap();
        let builds_after_first = user_syntax_set_build_count();
        let first_html = first.highlight_html("resource", "carina", 1).unwrap();
        assert!(first_html.contains("hl-keyword"), "{first_html}");

        fs::write(&syntax, UPDATED_CARINA_SUBLIME_SYNTAX).unwrap();
        let second = Highlighter::with_user_dir(dir.path()).unwrap();
        let second_html = second.highlight_html("resource", "carina", 1).unwrap();

        assert_eq!(user_syntax_set_build_count(), builds_after_first + 1);
        assert!(second_html.contains("hl-string"), "{second_html}");
        assert!(!second_html.contains("hl-keyword"), "{second_html}");
    }

    #[test]
    fn user_syntax_errors_are_stable_and_do_not_replace_cached_set() {
        let _guard = lock_user_syntax_tests();
        clear_user_syntax_set_cache();
        let dir = tempfile::tempdir().unwrap();
        let valid = dir.path().join("carina.sublime-syntax");
        let malformed = dir.path().join("broken.sublime-syntax");
        let missing = dir.path().join("missing.sublime-syntax");
        fs::write(&valid, CARINA_SUBLIME_SYNTAX).unwrap();
        fs::write(&malformed, ":::: not a syntax ::::").unwrap();

        let missing_before = Highlighter::with_user_files(std::slice::from_ref(&missing))
            .err()
            .expect("missing syntax unexpectedly loaded")
            .to_string();
        let malformed_before = Highlighter::with_user_files(std::slice::from_ref(&malformed))
            .err()
            .expect("malformed syntax unexpectedly loaded")
            .to_string();

        let cached = Highlighter::with_user_files(std::slice::from_ref(&valid)).unwrap();
        let builds_after_success = user_syntax_set_build_count();
        assert!(cached.validate_language("carina", 1).is_ok());

        let missing_after = Highlighter::with_user_files(std::slice::from_ref(&missing))
            .err()
            .expect("missing syntax unexpectedly loaded")
            .to_string();
        let malformed_after = Highlighter::with_user_files(std::slice::from_ref(&malformed))
            .err()
            .expect("malformed syntax unexpectedly loaded")
            .to_string();
        assert_eq!(missing_after.as_bytes(), missing_before.as_bytes());
        assert_eq!(malformed_after.as_bytes(), malformed_before.as_bytes());

        let cached_again = Highlighter::with_user_files(std::slice::from_ref(&valid)).unwrap();
        assert_eq!(user_syntax_set_build_count(), builds_after_success);
        assert!(cached_again.validate_language("carina", 1).is_ok());
    }

    #[test]
    fn malformed_user_syntax_file_returns_parse_error_with_path() {
        let _guard = lock_user_syntax_tests();
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
        let _guard = lock_user_syntax_tests();
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
        let _guard = lock_user_syntax_tests();
        let dir = tempfile::tempdir().unwrap();

        let highlighter = Highlighter::with_user_dir(dir.path()).unwrap();

        assert!(highlighter.validate_language("rust", 1).is_ok());
    }

    #[test]
    fn nonexistent_user_syntax_dir_returns_error() {
        let _guard = lock_user_syntax_tests();
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");

        assert!(Highlighter::with_user_dir(&missing).is_err());
    }

    #[test]
    fn highlights_carina_with_user_syntax_classes() {
        let _guard = lock_user_syntax_tests();
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
