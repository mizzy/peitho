//! Terminal rendering for diagnostics.
//!
//! Every terminal-facing site that prints a `miette::Report` must go through
//! [`render_diagnostic`], so propagated (`main`-returned) and swallowed (watch
//! loop, preview) error paths stay byte-identical. HTML sinks such as the
//! preview error page must use [`plain_diagnostic_text`] instead, because the
//! terminal renderer can emit ANSI escapes.
//!
//! The output is cargo/rustc-shaped, in the same family as lint's
//! `warning:` / `help:` output: a red `error:` prefix, the message wrapped to
//! the terminal width with a hanging indent, and a right-aligned yellow
//! `help:` block whose body shares the message's start column. There is
//! deliberately no wrap gutter — miette's graphical handler hardcodes the
//! severity color onto both the `×` marker and the gutter, which is the
//! red-vertical-bar noise this renderer replaces (issue #414).

use std::fmt;
use std::io::IsTerminal;

// The `help:` label is right-aligned under `error:` so the colons line up and
// both bodies start in the same column (author feedback 2026-08-06); the
// shared 7-column indent also keeps continuation lines of both blocks aligned.
const ERROR_PREFIX: &str = "error: ";
const HELP_PREFIX: &str = " help: ";
const ERROR_PREFIX_STYLED: &str = "\x1b[1;31merror:\x1b[0m ";
const HELP_PREFIX_STYLED: &str = " \x1b[1;33mhelp:\x1b[0m ";
const FALLBACK_WIDTH: usize = 80;
const MIN_WIDTH: usize = 40;

/// A `BuildError` crossing the CLI boundary with its structure intact:
/// `Display` is the location-prefixed headline, and the typed help rides
/// `miette::Diagnostic::help` so renderers can style it separately instead of
/// receiving one flattened multi-line string.
#[derive(Debug)]
pub(crate) struct DeckDiagnostic(peitho_core::BuildError);

impl DeckDiagnostic {
    pub(crate) fn new(err: peitho_core::BuildError) -> Self {
        Self(err)
    }
}

impl fmt::Display for DeckDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.headline())
    }
}

impl std::error::Error for DeckDiagnostic {}

impl miette::Diagnostic for DeckDiagnostic {
    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        if self.0.help.is_empty() {
            None
        } else {
            Some(Box::new(&self.0.help))
        }
    }
}

/// Render a diagnostic for the terminal. Color and width are derived from
/// stderr, where every caller writes diagnostics.
pub(crate) fn render_diagnostic(err: &miette::Report) -> String {
    render_diagnostic_parts(&err.to_string(), report_help(err).as_deref(), {
        let stderr = std::io::stderr();
        let is_tty = stderr.is_terminal();
        TerminalStyle {
            colors: is_tty && colors_enabled_by_env(),
            // Wrap only for humans at a real terminal; piped output (CI logs,
            // grep) keeps each logical line whole, like cargo and rustc.
            width: if is_tty {
                terminal_size::terminal_size_of(&stderr)
                    .map(|(width, _)| usize::from(width.0).max(MIN_WIDTH))
                    .unwrap_or(FALLBACK_WIDTH)
            } else {
                usize::MAX
            },
        }
    })
}

/// The diagnostic as plain text for non-terminal sinks (the preview error
/// page): headline plus the `  = help:` tail, matching `BuildError`'s own
/// `Display`, with no ANSI escapes.
pub(crate) fn plain_diagnostic_text(err: &miette::Report) -> String {
    match report_help(err) {
        Some(help) => format!("{err}\n  = help: {help}"),
        None => err.to_string(),
    }
}

/// The structured help of a report, if any. Sites that wrap one report inside
/// another must carry the inner help forward through this accessor — `{err}`
/// interpolation alone would silently drop it.
pub(crate) fn report_help(err: &miette::Report) -> Option<String> {
    err.help().map(|help| help.to_string())
}

fn colors_enabled_by_env() -> bool {
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    let dumb_term = std::env::var_os("TERM").is_some_and(|term| term == "dumb");
    !no_color && !dumb_term
}

pub(crate) struct TerminalStyle {
    colors: bool,
    width: usize,
}

impl TerminalStyle {
    #[cfg(test)]
    pub(crate) fn plain(width: usize) -> Self {
        Self {
            colors: false,
            width,
        }
    }

    #[cfg(test)]
    pub(crate) fn colored(width: usize) -> Self {
        Self {
            colors: true,
            width,
        }
    }
}

fn render_diagnostic_parts(message: &str, help: Option<&str>, style: TerminalStyle) -> String {
    let mut out = String::new();
    push_block(&mut out, message, ERROR_PREFIX, ERROR_PREFIX_STYLED, &style);
    if let Some(help) = help {
        push_block(&mut out, help, HELP_PREFIX, HELP_PREFIX_STYLED, &style);
    }
    out.truncate(out.trim_end_matches('\n').len());
    out
}

/// Wrap one logical block (message or help) to the target width with a
/// hanging indent under the prefix. Embedded newlines are preserved, each
/// logical line wrapping independently, aligned under the text start.
fn push_block(
    out: &mut String,
    text: &str,
    prefix: &str,
    styled_prefix: &str,
    style: &TerminalStyle,
) {
    let block_start = out.len();
    let indent = " ".repeat(prefix.len());
    for (index, line) in text.split('\n').enumerate() {
        // Greedy wrapping on plain spaces, never inside a token — not even at
        // hyphens: paths, URLs, CSS class names, and cache hashes must stay
        // contiguous so they can be copied and grepped, even when that
        // overflows the target width (cargo/rustc behave the same way).
        let options = textwrap::Options::new(style.width)
            .initial_indent(if index == 0 { prefix } else { &indent })
            .subsequent_indent(&indent)
            .word_separator(textwrap::WordSeparator::AsciiSpace)
            .word_splitter(textwrap::WordSplitter::NoHyphenation)
            .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit)
            .break_words(false);
        for wrapped in textwrap::wrap(line, options) {
            out.push_str(&wrapped);
            out.push('\n');
        }
    }
    if style.colors {
        // The block's first line starts with the plain prefix (wrapping was done
        // against its real width); swap in the styled variant afterwards so SGR
        // bytes never skew the width math.
        out.replace_range(block_start..block_start + prefix.len(), styled_prefix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck_report(err: peitho_core::BuildError) -> miette::Report {
        miette::Report::new(DeckDiagnostic::new(err))
    }

    fn frontmatter_error() -> peitho_core::BuildError {
        peitho_core::BuildError::new(
            peitho_core::error::ErrorKind::Parse,
            Some(3),
            "invalid deck frontmatter: unknown field `fontss`",
            "use only the supported deck frontmatter keys",
        )
        .with_origin_file("deck.md")
    }

    #[test]
    fn deck_diagnostic_displays_headline_and_exposes_structured_help() {
        let report = deck_report(frontmatter_error());

        assert_eq!(
            report.to_string(),
            "deck.md:3: invalid deck frontmatter: unknown field `fontss`"
        );
        assert_eq!(
            report.help().expect("help must survive").to_string(),
            "use only the supported deck frontmatter keys"
        );
    }

    #[test]
    fn deck_diagnostic_with_empty_help_has_none() {
        let err =
            peitho_core::BuildError::new(peitho_core::error::ErrorKind::Parse, None, "broken", "");

        assert!(deck_report(err).help().is_none());
    }

    // Rendering output is pinned with insta snapshots (`cargo insta review` to
    // accept intentional format changes); non-visual behavior stays in plain
    // asserts below.

    #[test]
    fn renders_error_prefix_and_help_block() {
        insta::assert_snapshot!(render_diagnostic_parts(
            "deck.md:3: broken deck",
            Some("fix the frontmatter"),
            TerminalStyle::plain(80),
        ));
    }

    #[test]
    fn help_label_aligns_with_error_label() {
        let rendered = render_diagnostic_parts(
            "deck.md:3: broken deck",
            Some("fix the frontmatter"),
            TerminalStyle::plain(80),
        );

        let text_columns: Vec<usize> = rendered
            .lines()
            .map(|line| line.find(": ").expect("label line") + 2)
            .collect();
        assert_eq!(text_columns, vec![7, 7], "actual: {rendered}");
    }

    #[test]
    fn wraps_with_hanging_indent_under_each_prefix() {
        insta::assert_snapshot!(render_diagnostic_parts(
            "deck.md:3: invalid deck frontmatter: unknown field `fontss`, expected one of `time`, `aspect_ratio`",
            Some("use only the supported deck frontmatter keys: time, aspect_ratio, resolution"),
            TerminalStyle::plain(60),
        ));
    }

    #[test]
    fn preserves_embedded_newlines_aligned_under_the_message() {
        insta::assert_snapshot!(render_diagnostic_parts(
            "slide 2 ('whoami'), line 16: no layout matches this slide\nbooks: unassigned content remains for missing 'body' slot\ncode: no slot accepts image in layout 'code'\nprofile: slot 'photo' got 2 item(s), but layout 'profile' allows 1",
            Some("adjust the slide content or pick a layout explicitly with <!-- {\"layout\":\"…\"} -->"),
            TerminalStyle::plain(80),
        ));
    }

    #[test]
    fn renders_without_help_when_absent() {
        insta::assert_snapshot!(render_diagnostic_parts(
            "broken",
            None,
            TerminalStyle::plain(80)
        ));
    }

    #[test]
    fn colors_only_the_prefix_labels() {
        insta::assert_snapshot!(render_diagnostic_parts(
            "deck.md:3: broken deck",
            Some("fix the frontmatter"),
            TerminalStyle::colored(80),
        ));
    }

    #[test]
    fn non_tty_width_keeps_logical_lines_whole() {
        insta::assert_snapshot!(render_diagnostic_parts(
            "deck.md:3: invalid deck frontmatter: unknown field `fontss`, expected one of `time`, `aspect_ratio`, `resolution`, `breaks`, `page_numbers`, `pointer_color`, `lang`, `layouts`, `css`, `syntaxes`, `fonts`, `code_images`",
            Some("use only the supported deck frontmatter keys: time, aspect_ratio, resolution, breaks, page_numbers, pointer_color, lang, layouts, css, syntaxes, fonts, code_images"),
            TerminalStyle::plain(usize::MAX),
        ));
    }

    #[test]
    fn long_tokens_overflow_instead_of_breaking() {
        insta::assert_snapshot!(render_diagnostic_parts(
            "line 3: code_images 'embed' failed: failed to render https://x.com/gosukenator/status/2083825695709597710: Chrome not found",
            Some("cache file: .peitho/embeds-cache/972af547d2f8499d017734fd84d4b6ba9f65228db5313b996ff1b7f07a167f38.png; delete the cache file to refresh"),
            TerminalStyle::plain(60),
        ));
    }

    #[test]
    fn plain_text_matches_build_error_display_shape() {
        let report = deck_report(frontmatter_error());

        assert_eq!(
            plain_diagnostic_text(&report),
            frontmatter_error().to_string()
        );
    }

    #[test]
    fn plain_text_omits_help_tail_when_absent() {
        let report = miette::miette!("watcher exploded");

        assert_eq!(plain_diagnostic_text(&report), "watcher exploded");
    }

    #[test]
    fn structured_help_from_adhoc_reports_is_rendered() {
        let report = miette::miette!(
            help = "workspace kept at /tmp/x",
            "Chrome PDF export failed"
        );

        let rendered = render_diagnostic_parts(
            &report.to_string(),
            report.help().map(|help| help.to_string()).as_deref(),
            TerminalStyle::plain(80),
        );

        assert_eq!(
            rendered,
            "error: Chrome PDF export failed\n help: workspace kept at /tmp/x"
        );
    }
}
