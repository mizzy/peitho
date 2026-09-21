//! Source extraction and rewriting for a single slide body.

use crate::{
    domain::{SlideKey, SourceSpan},
    error::{BuildError, ErrorKind, Result},
    highlight::Highlighter,
    notes_edit::{
        canonical_comment, last_nonblank_line_end, normalized_note_text, remove_comment_spans,
        restore_bom, source_line_ending, spans_match_source, strip_bom,
    },
    parser::{parse_frontmatter, parse_markdown},
    phase::{Deck, Parsed, ParsedSlide},
    slide_compare::{compare_all, compare_except_key, target_key_change_allowed},
};
use std::ops::Range;

const SLIDE_BODY_EDIT_HELP: &str =
    "keep page settings, speaker notes, and the rest of the deck unchanged, then retry";

/// A validated slide-body rewrite and its post-save identity.
#[derive(Debug)]
pub struct SlideBodyRewrite {
    pub source: String,
    pub key: SlideKey,
    pub body: String,
}

/// Returns a slide's body Markdown without page settings or speaker notes.
///
/// Leading and trailing blank lines are removed, and line endings are
/// normalized to LF. A parse error is returned when the deck uses bare CR line
/// endings or the slide's recorded spans do not match `source`.
pub fn slide_body(source: &str, slide: &ParsedSlide) -> Result<String> {
    let (source, _) = strip_bom(source);
    refuse_bare_cr(source)?;
    if !spans_match_source(
        source,
        slide.source_span,
        slide.settings_span,
        &slide.note_spans,
    ) {
        return Err(BuildError::new(
            ErrorKind::Parse,
            None,
            "slide body spans do not match the deck source",
            "reload the preview and retry",
        ));
    }

    let mut spans = slide.note_spans.clone();
    spans.extend(slide.settings_span);

    let (body_source, removed_bytes) = remove_comment_spans(source, &spans);
    let body = &body_source[slide.source_span.start..slide.source_span.end - removed_bytes];
    Ok(normalize_body(body))
}

/// Returns the full source after a validated rewrite of one slide's body.
pub fn rewrite_slide_body(
    source: &str,
    target: &ParsedSlide,
    new_body: &str,
    highlighter: &Highlighter,
) -> Result<SlideBodyRewrite> {
    let (source, had_bom) = strip_bom(source);
    refuse_bare_cr(source)?;
    let before = parse_source(source, highlighter)?;
    let Some(target_index) = before
        .parsed_slides()
        .iter()
        .position(|slide| slide.source_span == target.source_span)
    else {
        return Err(target_mismatch_refusal());
    };
    let fresh_target = &before.parsed_slides()[target_index];
    if fresh_target.key != target.key
        || fresh_target.source_index != target.source_index
        || fresh_target.settings_span != target.settings_span
        || fresh_target.note_spans != target.note_spans
    {
        return Err(target_mismatch_refusal());
    }
    let normalized_body = normalize_body(new_body);
    let line_ending = source_line_ending(source, fresh_target.source_span);
    let (leading, trailing) = edge_blank_runs(source, fresh_target.source_span)?;
    let mut replacement = source[leading].to_owned();
    let mut interior = Vec::new();
    if let Some(settings_span) = fresh_target.settings_span {
        interior.push(
            strip_one_line_ending(&source[settings_span.start..settings_span.end]).to_owned(),
        );
    }
    if !normalized_body.is_empty() {
        interior.push(normalized_body.replace('\n', line_ending));
    }
    if let Some(notes) = fresh_target.notes.as_deref() {
        interior.push(canonical_comment(notes, line_ending));
    }
    replacement.push_str(&interior.join(&[line_ending, line_ending].concat()));
    let trailing = &source[trailing];
    if fresh_target.source_span.end < source.len() && matches!(trailing, "" | "\n" | "\r\n") {
        replacement.push_str(line_ending);
    }
    replacement.push_str(trailing);

    let mut candidate = source.to_owned();
    candidate.replace_range(
        fresh_target.source_span.start..fresh_target.source_span.end,
        &replacement,
    );
    // The parser strips leading BOMs, so span slicing and `restore_bom` require none here.
    let candidate = candidate.trim_start_matches('\u{feff}').to_owned();
    let after = parse_source(&candidate, highlighter)?;
    let (key, body) = validate_reparsed_slide_body(
        source,
        &before,
        &candidate,
        &after,
        target_index,
        &normalized_body,
    )?;

    Ok(SlideBodyRewrite {
        source: restore_bom(candidate, had_bom),
        key,
        body,
    })
}

fn parse_source(source: &str, highlighter: &Highlighter) -> Result<Deck<Parsed>> {
    let frontmatter = parse_frontmatter(source)?;
    parse_markdown(source, frontmatter, highlighter)
}

fn validate_reparsed_slide_body(
    before_source: &str,
    before: &Deck<Parsed>,
    after_source: &str,
    after: &Deck<Parsed>,
    target_index: usize,
    expected_body: &str,
) -> Result<(SlideKey, String)> {
    let before_slides = before.parsed_slides();
    let after_slides = after.parsed_slides();
    if before_slides.len() != after_slides.len() {
        return Err(slide_count_refusal());
    }
    if before.settings().sections() != after.settings().sections() {
        return Err(refusal("slide body edit would change the deck's sections"));
    }

    for (index, (before_slide, after_slide)) in before_slides.iter().zip(after_slides).enumerate() {
        if index == target_index {
            continue;
        }
        if let Err(difference) = compare_all(before_slide, after_slide) {
            return Err(refusal(format!(
                "slide body edit would change another slide's {}",
                difference.noun_phrase()
            )));
        }
    }

    let before_target = &before_slides[target_index];
    let after_target = &after_slides[target_index];
    if let Err(difference) = compare_except_key(before_target, after_target) {
        return Err(refusal(format!(
            "slide body edit would change the edited slide's {}",
            difference.noun_phrase()
        )));
    }
    if !target_key_change_allowed(before_target, after_target) {
        return Err(refusal(
            "slide body edit would change an explicit key on the edited slide",
        ));
    }
    let before_settings = before_target
        .settings_span
        .map(|span| strip_one_line_ending(&before_source[span.start..span.end]));
    let after_settings = after_target
        .settings_span
        .map(|span| strip_one_line_ending(&after_source[span.start..span.end]));
    if before_settings != after_settings {
        return Err(refusal(
            "slide body edit would change the edited slide's settings comment",
        ));
    }
    let body = slide_body(after_source, after_target)?;
    if body != expected_body {
        return Err(round_trip_refusal());
    }

    Ok((after_target.key.clone(), body))
}

fn edge_blank_runs(source: &str, slide: SourceSpan) -> Result<(Range<usize>, Range<usize>)> {
    let body = &source[slide.start..slide.end];
    let leading_bytes = body
        .split_inclusive('\n')
        .take_while(|line| line.trim().is_empty())
        .map(str::len)
        .sum::<usize>();
    let Some(trailing_start) = last_nonblank_line_end(source, slide) else {
        return Err(BuildError::new(
            ErrorKind::Parse,
            None,
            "slide has no non-blank line",
            "edit the slide in a Markdown editor, then reload the preview",
        ));
    };
    Ok((
        slide.start..slide.start + leading_bytes,
        trailing_start..slide.end,
    ))
}

fn strip_one_line_ending(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
}

fn refusal(message: impl Into<String>) -> BuildError {
    BuildError::new(ErrorKind::Parse, None, message, SLIDE_BODY_EDIT_HELP)
}

fn target_mismatch_refusal() -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        None,
        "slide body edit target does not match the current deck source",
        "reload the preview and retry",
    )
}

fn slide_count_refusal() -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        None,
        "slide body edit would change the deck's slide count",
        "a `---` line in the body splits the slide, and removing all content removes it; take the separator out or keep some content, then retry",
    )
}

fn round_trip_refusal() -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        None,
        "slide body edit did not round-trip through the Markdown parser",
        "the Markdown parser reads the saved text differently from what was typed, typically because of an unclosed code fence or HTML block; close it and retry",
    )
}

fn refuse_bare_cr(source: &str) -> Result<()> {
    let has_bare_cr =
        source.as_bytes().iter().enumerate().any(|(index, byte)| {
            *byte == b'\r' && source.as_bytes().get(index + 1) != Some(&b'\n')
        });
    if has_bare_cr {
        return Err(BuildError::new(
            ErrorKind::Parse,
            None,
            "bare CR line endings are not supported by preview editing",
            "convert the deck to LF or CRLF line endings, then reload the preview",
        ));
    }
    Ok(())
}

fn normalize_body(body: &str) -> String {
    let normalized = normalized_note_text(body);
    let lines = normalized.split('\n').collect::<Vec<_>>();
    let Some(start) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return String::new();
    };
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .unwrap();
    lines[start..=end].join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        edge_blank_runs, parse_source, rewrite_slide_body, slide_body, validate_reparsed_slide_body,
    };
    use crate::{
        domain::{SlideKey, SourceSpan},
        error::{BuildError, ErrorKind},
        highlight::Highlighter,
        notes_edit::strip_bom,
    };

    const TARGET_WITH_METADATA_SOURCE: &str = "# First\n\n---\n\n<!-- {\"key\":\"target\",\"layout\":\"cover\"} -->\n# Old\n\n<!-- speaker note -->\n\n---\n\n# Last\n";

    #[test]
    fn rewrite_slide_body_canonicalizes_settings_body_and_one_note() {
        let source = "\n<!-- {\"key\":\"fixed\"} -->\n\n<!-- note one -->\n\n# Old\n\nTail <!-- note two -->\n\n";
        let expected = "\n<!-- {\"key\":\"fixed\"} -->\n\n# New\n\n- one\n\n<!--\nnote one\n\nnote two\n-->\n\n";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();
        let rewritten = rewrite_slide_body(
            source,
            &deck.parsed_slides()[0],
            "# New\n\n- one",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten.source, expected);
        assert_eq!(rewritten.key.as_str(), "fixed");
        assert_eq!(rewritten.body, "# New\n\n- one");
    }

    #[test]
    fn rewrite_slide_body_accepts_structural_changes_and_preserves_source_conventions() {
        struct Case {
            name: &'static str,
            source: &'static str,
            slide_index: usize,
            new_body: &'static str,
            expected_source: &'static str,
            expected_key: &'static str,
        }

        let cases = [
            Case {
                name: "paragraph-to-list",
                source: "# Title\n\nOld paragraph\n",
                slide_index: 0,
                new_body: "# Title\n\n- one\n- two",
                expected_source: "# Title\n\n- one\n- two\n",
                expected_key: "title",
            },
            Case {
                name: "new-code-block",
                source: "# Title\n\nOld paragraph\n",
                slide_index: 0,
                new_body: "# Title\n\n```rust\nfn main() {}\n```",
                expected_source: "# Title\n\n```rust\nfn main() {}\n```\n",
                expected_key: "title",
            },
            Case {
                name: "changed-reveal-and-emphasis-shape",
                source: "# Title\n\nOld paragraph\n",
                slide_index: 0,
                new_body: "# Title\n\n::: {reveal}\n\n- one\n- two\n\n:::\n\n```rust {1|2}\nlet one = 1;\nlet two = 2;\n```",
                expected_source: "# Title\n\n::: {reveal}\n\n- one\n- two\n\n:::\n\n```rust {1|2}\nlet one = 1;\nlet two = 2;\n```\n",
                expected_key: "title",
            },
            Case {
                name: "derived-heading-key-change",
                source: "# Old heading\n\nBody\n",
                slide_index: 0,
                new_body: "# New heading\n\nBody",
                expected_source: "# New heading\n\nBody\n",
                expected_key: "new-heading",
            },
            Case {
                name: "fixed-key-and-section-marker",
                source: "---\ntime: 1m\n---\n<!-- {\"key\":\"fixed\",\"section\":\"Only\",\"time\":\"1m\"} -->\n# Old\n",
                slide_index: 0,
                new_body: "# New\n\n- item",
                expected_source: "---\ntime: 1m\n---\n<!-- {\"key\":\"fixed\",\"section\":\"Only\",\"time\":\"1m\"} -->\n\n# New\n\n- item\n",
                expected_key: "fixed",
            },
            Case {
                name: "inline-settings-paragraph",
                source: "text <!-- {\"key\":\"a\"} -->",
                slide_index: 0,
                new_body: "Changed",
                expected_source: "<!-- {\"key\":\"a\"} -->\n\nChanged",
                expected_key: "a",
            },
            Case {
                name: "inline-settings-heading",
                source: "# <!-- {\"key\":\"a\"} --> T",
                slide_index: 0,
                new_body: "# New",
                expected_source: "<!-- {\"key\":\"a\"} -->\n\n# New",
                expected_key: "a",
            },
            Case {
                name: "settings-indent-3",
                source: "   <!-- {\"key\":\"a\"} -->\n# Old\n",
                slide_index: 0,
                new_body: "# New",
                expected_source: "<!-- {\"key\":\"a\"} -->\n\n# New\n",
                expected_key: "a",
            },
            Case {
                name: "settings-trailing-spaces",
                source: "<!-- {\"key\":\"a\"} -->   \n# Old\n",
                slide_index: 0,
                new_body: "# New",
                expected_source: "<!-- {\"key\":\"a\"} -->   \n\n# New\n",
                expected_key: "a",
            },
            Case {
                name: "note-before-settings",
                source: "<!-- note -->\n<!-- {\"key\":\"a\"} -->\n# Old\n",
                slide_index: 0,
                new_body: "# New",
                expected_source: "<!-- {\"key\":\"a\"} -->\n\n# New\n\n<!-- note -->\n",
                expected_key: "a",
            },
            Case {
                name: "non-first-middle-slide",
                source: "# First\n\n---\n\n# Middle\n\n---\n\n# Last\n",
                slide_index: 1,
                new_body: "# Changed\n\nBody",
                expected_source: "# First\n\n---\n\n# Changed\n\nBody\n\n---\n\n# Last\n",
                expected_key: "changed",
            },
            Case {
                name: "last-slide-at-eof",
                source: "# First\n\n---\n\n# Last",
                slide_index: 1,
                new_body: "# Changed",
                expected_source: "# First\n\n---\n\n# Changed",
                expected_key: "changed",
            },
            Case {
                name: "pure-crlf",
                source: "<!-- {\"key\":\"fixed\"} -->\r\n\r\n# Old\r\n\r\n<!-- note -->\r\n",
                slide_index: 0,
                new_body: "# New\n\n- one",
                expected_source: "<!-- {\"key\":\"fixed\"} -->\r\n\r\n# New\r\n\r\n- one\r\n\r\n<!-- note -->\r\n",
                expected_key: "fixed",
            },
            Case {
                name: "leading-bom",
                source: "\u{feff}# Old\n",
                slide_index: 0,
                new_body: "# New",
                expected_source: "\u{feff}# New\n",
                expected_key: "new",
            },
            Case {
                name: "configured-code-image-renderer-does-not-run",
                source: "---\ncode_images:\n  dot: \"false\"\n---\n# Old\n\n```dot\ndigraph { old -> body }\n```\n",
                slide_index: 0,
                new_body: "# New\n\n```dot\ndigraph { new -> body }\n```",
                expected_source: "---\ncode_images:\n  dot: \"false\"\n---\n# New\n\n```dot\ndigraph { new -> body }\n```\n",
                expected_key: "new",
            },
        ];
        let highlighter = Highlighter::defaults();

        for case in cases {
            let deck = parse_source(case.source, &highlighter).unwrap();
            let target = &deck.parsed_slides()[case.slide_index];

            let rewritten =
                rewrite_slide_body(case.source, target, case.new_body, &highlighter).unwrap();

            assert_eq!(
                rewritten.source, case.expected_source,
                "{}: source",
                case.name
            );
            assert_eq!(
                rewritten.key.as_str(),
                case.expected_key,
                "{}: key",
                case.name
            );
            assert_eq!(rewritten.body, case.new_body, "{}: body", case.name);
        }
    }

    #[test]
    fn rewrite_slide_body_rejects_target_that_does_not_match_fresh_parse() {
        let source = "<!-- {\"key\":\"fixed\"} -->\n# Old\n<!-- note -->\n";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();
        let target = &deck.parsed_slides()[0];
        let mut candidates = Vec::new();

        let mut missing = target.clone();
        missing.source_span.start += 1;
        candidates.push(("source-start", missing));

        let mut key = target.clone();
        key.key = SlideKey::new("other").unwrap();
        candidates.push(("key", key));

        let mut source_index = target.clone();
        source_index.source_index += 1;
        candidates.push(("source-index", source_index));

        let mut source_span = target.clone();
        source_span.source_span.end -= 1;
        candidates.push(("source-span", source_span));

        let mut settings_span = target.clone();
        settings_span.settings_span = None;
        candidates.push(("settings-span", settings_span));

        let mut note_spans = target.clone();
        note_spans.note_spans.clear();
        candidates.push(("note-spans", note_spans));

        for (name, candidate) in candidates {
            let error = rewrite_slide_body(source, &candidate, "# New", &highlighter).unwrap_err();

            assert_eq!(error.kind, ErrorKind::Parse, "{name}: kind");
            assert_eq!(error.line, None, "{name}: line");
            assert_eq!(
                error.message, "slide body edit target does not match the current deck source",
                "{name}: message"
            );
            assert_eq!(error.help, "reload the preview and retry", "{name}: help");
        }
    }

    #[test]
    fn rewrite_slide_body_adds_blank_line_before_tight_separator() {
        let source = "# A\n---\n# B\n";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();
        assert_eq!(deck.parsed_slides().len(), 2);

        let rewritten = rewrite_slide_body(
            source,
            &deck.parsed_slides()[0],
            "# A\n\ntext",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten.source, "# A\n\ntext\n\n---\n# B\n");
        assert_eq!(rewritten.body, "# A\n\ntext");
        let deck = parse_source(&rewritten.source, &highlighter).unwrap();
        assert_eq!(deck.parsed_slides().len(), 2);
    }

    #[test]
    fn rewrite_slide_body_blank_line_rule_preserves_existing_spacing_and_final_newline() {
        let cases = [
            ("existing-blank-line", "# A\n\n---\n# B\n", 0, "# A"),
            ("last-slide-with-newline", "# A\n\n---\n\n# B\n", 1, "# B"),
        ];
        let highlighter = Highlighter::defaults();

        for (name, source, slide_index, body) in cases {
            let deck = parse_source(source, &highlighter).unwrap();
            let rewritten = rewrite_slide_body(
                source,
                &deck.parsed_slides()[slide_index],
                body,
                &highlighter,
            )
            .unwrap();

            assert_eq!(rewritten.source, source, "{name}: source");
            assert_eq!(rewritten.body, body, "{name}: body");
        }
    }

    #[test]
    fn rewrite_slide_body_adds_crlf_blank_line_before_tight_separator() {
        let source = "# A\r\n---\r\n# B\r\n";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();

        let rewritten = rewrite_slide_body(
            source,
            &deck.parsed_slides()[0],
            "# A\n\ntext",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten.source, "# A\r\n\r\ntext\r\n\r\n---\r\n# B\r\n");
        assert_eq!(rewritten.body, "# A\n\ntext");
    }

    #[test]
    fn rewrite_slide_body_adds_blank_line_after_canonical_note_before_separator() {
        let source = "# A\n<!-- speaker note -->\n---\n# B\n";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();

        let rewritten =
            rewrite_slide_body(source, &deck.parsed_slides()[0], "# Changed", &highlighter)
                .unwrap();

        assert_eq!(
            rewritten.source,
            "# Changed\n\n<!-- speaker note -->\n\n---\n# B\n"
        );
        assert_eq!(rewritten.body, "# Changed");
    }

    #[test]
    fn rewrite_slide_body_refuses_submitted_leading_bom() {
        let cases = [
            // Behavior-only case: pins the ordinary refusal without a settings comment.
            (
                "without-settings",
                "# T\n\n---\n\n# B\n",
                "\u{feff}\n# T",
                "slide body edit did not round-trip through the Markdown parser",
            ),
            // Before the candidate BOM strip this sliced inside the BOM and panicked.
            (
                "with-settings",
                "# T\n\n---\n\n# B\n",
                "\u{feff}\n<!-- {\"skip\":false} -->\n# T",
                "slide body edit would change the edited slide's settings comment",
            ),
            (
                "double-bom-lf-deck",
                "# T\n\n---\n\n# B\n",
                "\u{feff}\u{feff}\n<!-- {\"skip\":false} -->\n# T",
                "slide body edit would change the edited slide's settings comment",
            ),
            (
                "double-bom-bom-deck",
                "\u{feff}# T\n\n---\n\n# B\n",
                "\u{feff}\u{feff}\n<!-- {\"skip\":false} -->\n# T",
                "slide body edit would change the edited slide's settings comment",
            ),
        ];
        let highlighter = Highlighter::defaults();

        for (case, source, body, expected_message) in cases {
            let deck = parse_source(source, &highlighter).unwrap();
            let error = rewrite_slide_body(source, &deck.parsed_slides()[0], body, &highlighter)
                .unwrap_err();

            assert_eq!(error.kind, ErrorKind::Parse, "{case}: kind");
            assert_eq!(error.message, expected_message, "{case}: message");
        }
    }

    // Pins each observed refusal's exact message and source line.
    fn assert_refusal_cause(
        case: &str,
        error: &BuildError,
        expected_message: &str,
        expected_line: Option<usize>,
    ) {
        assert_eq!(error.kind, ErrorKind::Parse, "{case}: kind");
        assert_eq!(error.message, expected_message, "{case}: message");
        assert_eq!(error.line, expected_line, "{case}: line");
    }

    #[test]
    fn rewrite_slide_body_refusals_fall_out_of_reparse() {
        struct Case {
            name: &'static str,
            source: &'static str,
            slide_index: usize,
            body: &'static str,
            expected_message: &'static str,
            expected_line: Option<usize>,
            expected_help: Option<&'static str>,
        }

        let cases = [
            Case {
                name: "separator",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "# New\n\n---\n\n# Extra",
                expected_message: "slide body edit would change the deck's slide count",
                expected_line: None,
                expected_help: None,
            },
            Case {
                name: "plaintext-comment",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "# New\n\n<!-- stolen note -->",
                expected_message: "slide body edit would change the edited slide's notes",
                expected_line: None,
                expected_help: None,
            },
            Case {
                name: "json-comment",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "<!-- {\"layout\":\"cover\"} -->\n# New",
                expected_message: "duplicate page settings comment",
                expected_line: Some(7),
                expected_help: None,
            },
            Case {
                name: "unclosed-fence",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "# New\n\n```rust\nlet x = 1;",
                expected_message: "slide body edit would change the deck's slide count",
                expected_line: None,
                expected_help: None,
            },
            Case {
                name: "unknown-language",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "# New\n\n```not-installed\nx\n```",
                expected_message: "unknown code language 'not-installed'",
                expected_line: Some(9),
                expected_help: None,
            },
            Case {
                name: "bad-slot",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "::: {slot=}\ntext\n:::",
                expected_message: "explicit slot fence attribute needs a slot name",
                expected_line: Some(7),
                expected_help: None,
            },
            Case {
                name: "bad-reveal",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "::: {reveal=yes}\ntext\n:::",
                expected_message: "reveal fence values are reserved for future syntax",
                expected_line: Some(7),
                expected_help: None,
            },
            Case {
                name: "bad-emphasis",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "```rust {0}\nx\n```",
                expected_message: "code line numbers start at 1",
                expected_line: Some(7),
                expected_help: None,
            },
            // Focused decks reach outcomes that the shared metadata fixture cannot isolate.
            // The thematic break splits the slide, drops an empty half, preserves count, and changes text.
            Case {
                name: "body-round-trip",
                source: "# Tight A\n---\n# Tight B\n---\n# Tight C",
                slide_index: 0,
                body: "***\n- a\n",
                expected_message: "slide body edit did not round-trip through the Markdown parser",
                expected_line: None,
                expected_help: Some(
                    "the Markdown parser reads the saved text differently from what was typed, typically because of an unclosed code fence or HTML block; close it and retry",
                ),
            },
            // The split adds a slide; the comment swallows the next, preserving count before sections.
            Case {
                name: "sections",
                source: "<!-- {\"section\":\"One\",\"time\":\"1m\"} -->\n# A\n\n---\n\n<!-- {\"section\":\"Two\",\"time\":\"1m\"} -->\n# B\n\n---\n\n# C\n",
                slide_index: 0,
                body: "# A\n\n---\n\n<!--",
                expected_message: "slide body edit would change the deck's sections",
                expected_line: None,
                expected_help: None,
            },
            // The split adds a slide; the comment swallows the next, preserving count before notes.
            Case {
                name: "another-slide-notes",
                source: "# A\n\n---\n\n<!-- note -->\n# B\n\n---\n\n# C\n",
                slide_index: 0,
                body: "# A\n\n---\n\n<!--",
                expected_message: "slide body edit would change another slide's notes",
                expected_line: None,
                expected_help: None,
            },
            Case {
                name: "explicit-key",
                source: "# T\n\n---\n\n# B\n",
                slide_index: 0,
                body: "<!-- {\"key\":\"other\"} -->\n# T",
                expected_message: "slide body edit would change an explicit key on the edited slide",
                expected_line: None,
                expected_help: None,
            },
            Case {
                name: "settings-presence",
                source: "# T\n\n---\n\n# B\n",
                slide_index: 0,
                body: "<!-- {\"skip\":false} -->\n# T",
                expected_message: "slide body edit would change the edited slide's settings comment",
                expected_line: None,
                expected_help: None,
            },
            // Other slides remain, so the postcondition observes the emptied target disappearing.
            Case {
                name: "whitespace-removes-one-of-three-slides",
                source: "# A\n\n---\n\n# T\n\n---\n\n# B\n",
                slide_index: 1,
                body: " \n\t\n",
                expected_message: "slide body edit would change the deck's slide count",
                expected_line: None,
                expected_help: Some(
                    "a `---` line in the body splits the slide, and removing all content removes it; take the separator out or keep some content, then retry",
                ),
            },
            // Emptying the only body leaves no slide for the parser to return.
            Case {
                name: "whitespace-removes-only-slide",
                source: "# T\n",
                slide_index: 0,
                body: " \n\t\n",
                expected_message: "deck has no slides",
                expected_line: None,
                expected_help: None,
            },
        ];
        let highlighter = Highlighter::defaults();

        for case in cases {
            let deck = parse_source(case.source, &highlighter).unwrap();
            let error = rewrite_slide_body(
                case.source,
                &deck.parsed_slides()[case.slide_index],
                case.body,
                &highlighter,
            )
            .unwrap_err();

            assert_refusal_cause(case.name, &error, case.expected_message, case.expected_line);
            if let Some(expected_help) = case.expected_help {
                assert_eq!(error.help, expected_help, "{}: help", case.name);
            }
        }
    }

    #[test]
    fn rewrite_slide_body_accepts_bodies_without_naive_input_prechecks() {
        struct Case {
            name: &'static str,
            source: &'static str,
            slide_index: usize,
            body: &'static str,
            expected_body: &'static str,
            expected_source: Option<&'static str>,
        }

        let cases = [
            Case {
                name: "whitespace-with-settings-and-note",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: " \n\t\n",
                expected_body: "",
                expected_source: Some(
                    "# First\n\n---\n\n<!-- {\"key\":\"target\",\"layout\":\"cover\"} -->\n\n<!-- speaker note -->\n\n---\n\n# Last\n",
                ),
            },
            Case {
                name: "separator-inside-code-fence",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "# New\n\n```text\n---\n```",
                expected_body: "# New\n\n```text\n---\n```",
                expected_source: None,
            },
            Case {
                name: "comment-inside-code-fence",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "# New\n\n```text\n<!-- not a note -->\n```",
                expected_body: "# New\n\n```text\n<!-- not a note -->\n```",
                expected_source: None,
            },
            Case {
                name: "comment-inside-inline-code",
                source: TARGET_WITH_METADATA_SOURCE,
                slide_index: 1,
                body: "# New\n\n`<!-- x -->`",
                expected_body: "# New\n\n`<!-- x -->`",
                expected_source: None,
            },
            Case {
                name: "settings-only-empty-body",
                source: "<!-- {\"key\":\"a\"} -->\n# T\n\n---\n\n# B\n",
                slide_index: 0,
                body: " \n\t\n",
                expected_body: "",
                expected_source: Some("<!-- {\"key\":\"a\"} -->\n\n---\n\n# B\n"),
            },
            Case {
                name: "note-only-empty-body",
                source: "# T\n<!-- note -->\n\n---\n\n# B\n",
                slide_index: 0,
                body: " \n\t\n",
                expected_body: "",
                expected_source: Some("<!-- note -->\n\n---\n\n# B\n"),
            },
        ];
        let highlighter = Highlighter::defaults();

        for case in cases {
            let deck = parse_source(case.source, &highlighter).unwrap();
            let rewritten = rewrite_slide_body(
                case.source,
                &deck.parsed_slides()[case.slide_index],
                case.body,
                &highlighter,
            )
            .unwrap();

            assert_eq!(rewritten.body, case.expected_body, "{}: body", case.name);
            if let Some(expected_source) = case.expected_source {
                assert_eq!(rewritten.source, expected_source, "{}: source", case.name);
            }
        }
    }

    #[test]
    fn rewrite_slide_body_is_parse_identity_preserving_and_idempotent() {
        let corpus = [
            (
                "settings-sections-notes-and-flags",
                concat!(
                    "---\n",
                    "time: 2m\n",
                    "page_numbers: current\n",
                    "---\n",
                    "<!-- {\"key\":\"intro\",\"layout\":\"cover\",\"section\":\"Opening\",\"time\":\"1m\"} -->\n",
                    "<!-- before -->\n",
                    "# Intro\n\n",
                    "Text <!-- inline --> tail\n\n",
                    "<!-- after -->\n\n",
                    "---\n\n",
                    "<!-- {\"section\":\"Closing\",\"time\":\"1m\",\"page_number\":false} -->\n",
                    "# Closing\n\n",
                    "::: {reveal}\n\n",
                    "- one\n",
                    "- two\n\n",
                    ":::\n\n",
                    "---\n\n",
                    "<!-- {\"key\":\"appendix\",\"skip\":true} -->\n",
                    "# Appendix\n",
                ),
            ),
            (
                "bom-and-crlf",
                "\u{feff}<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\nBody\r\n<!-- crlf note -->\r\n",
            ),
            (
                "tight-separators",
                "# Tight A\n---\n# Tight B\n---\n# Tight C",
            ),
            ("tight-crlf-separators", "# A\r\n---\r\n# B\r\n"),
            (
                "last-slide-with-newline",
                "# First\n\n---\n\n# Last with newline\n",
            ),
            (
                "last-slide-without-newline",
                "# First\n\n---\n\n# Last without newline",
            ),
        ];
        let highlighter = Highlighter::defaults();

        // The corpus contains none of the three documented limitations: an
        // unclosed fence on a slide with notes, a note as the sole content of
        // a list item or blockquote line, and two comments on one line.
        for (case, source) in corpus {
            let before = parse_source(source, &highlighter).unwrap();

            for target_index in 0..before.parsed_slides().len() {
                let body = slide_body(source, &before.parsed_slides()[target_index]).unwrap();
                let first = rewrite_slide_body(
                    source,
                    &before.parsed_slides()[target_index],
                    &body,
                    &highlighter,
                )
                .unwrap();
                let after = parse_source(&first.source, &highlighter).unwrap();

                assert_eq!(
                    after.settings().sections(),
                    before.settings().sections(),
                    "{case}/{target_index}: sections"
                );
                assert_eq!(
                    after.parsed_slides().len(),
                    before.parsed_slides().len(),
                    "{case}/{target_index}: slide count"
                );
                for (slide_index, (before_slide, after_slide)) in before
                    .parsed_slides()
                    .iter()
                    .zip(after.parsed_slides())
                    .enumerate()
                {
                    assert_eq!(
                        after_slide.index, before_slide.index,
                        "{case}/{target_index}/{slide_index}: index"
                    );
                    assert_eq!(
                        after_slide.source_index, before_slide.source_index,
                        "{case}/{target_index}/{slide_index}: source index"
                    );
                    assert_eq!(
                        after_slide.key, before_slide.key,
                        "{case}/{target_index}/{slide_index}: key"
                    );
                    assert!(
                        after_slide
                            .key_source
                            .same_kind_as(&before_slide.key_source),
                        "{case}/{target_index}/{slide_index}: key source kind"
                    );
                    assert_eq!(
                        after_slide.layout_request_name(),
                        before_slide.layout_request_name(),
                        "{case}/{target_index}/{slide_index}: layout"
                    );
                    assert_eq!(
                        after_slide.skip, before_slide.skip,
                        "{case}/{target_index}/{slide_index}: skip"
                    );
                    assert_eq!(
                        after_slide.page_number_hidden, before_slide.page_number_hidden,
                        "{case}/{target_index}/{slide_index}: page number"
                    );
                    assert_eq!(
                        after_slide.notes, before_slide.notes,
                        "{case}/{target_index}/{slide_index}: notes"
                    );
                    assert_eq!(
                        after_slide.step_count, before_slide.step_count,
                        "{case}/{target_index}/{slide_index}: step count"
                    );
                    assert_eq!(
                        slide_body(&first.source, after_slide).unwrap(),
                        slide_body(source, before_slide).unwrap(),
                        "{case}/{target_index}/{slide_index}: body"
                    );
                }

                let target_after = &after.parsed_slides()[target_index];
                assert_eq!(
                    first.key, target_after.key,
                    "{case}/{target_index}: result key"
                );
                assert_eq!(
                    first.body,
                    slide_body(&first.source, target_after).unwrap(),
                    "{case}/{target_index}: result body"
                );
                let second =
                    rewrite_slide_body(&first.source, target_after, &first.body, &highlighter)
                        .unwrap();

                assert_eq!(second.source, first.source, "{case}/{target_index}: source");
                assert_eq!(second.key, first.key, "{case}/{target_index}: key");
                assert_eq!(second.body, first.body, "{case}/{target_index}: body");
            }
        }
    }

    #[test]
    fn rewrite_slide_body_adds_blank_line_for_mixed_trailing_line_endings() {
        let cases = [
            (
                "crlf-then-lf",
                "# A\r\n- x\n---\n# B\n",
                "para\r\n\n---\n# B\n",
            ),
            (
                "lf-then-crlf",
                "# A\n- x\r\n---\r\n# B\r\n",
                "para\r\n\r\n---\r\n# B\r\n",
            ),
        ];
        let highlighter = Highlighter::defaults();

        for (name, source, expected) in cases {
            let deck = parse_source(source, &highlighter).unwrap();
            let rewritten =
                rewrite_slide_body(source, &deck.parsed_slides()[0], "para", &highlighter).unwrap();

            assert_eq!(rewritten.source, expected, "{name}: source");
            assert_eq!(rewritten.body, "para", "{name}: body");
            let deck = parse_source(&rewritten.source, &highlighter).unwrap();
            assert_eq!(deck.parsed_slides().len(), 2, "{name}: slide count");
        }
    }

    #[test]
    fn slide_body_and_rewrite_slide_body_refuse_bare_cr_anywhere_in_the_deck() {
        let cases = [
            ("target", "# T\r\rtext\r", 0, "# T\n\ntext"),
            ("inside-replaced-part", "# bare\rcr\n", 0, "\\"),
            (
                "separator-before-target",
                "# A\n\n---\r<!-- n -->\n# B\n",
                1,
                "x",
            ),
            ("swallowed-tail", "# T\n***\r", 0, "~~~"),
            ("another-slide", "# A\n\n---\n\n# B\r", 0, "# Changed"),
        ];
        let highlighter = Highlighter::defaults();

        for (case, source, slide_index, new_body) in cases {
            let deck = parse_source(source, &highlighter).unwrap();
            let target = &deck.parsed_slides()[slide_index];

            for (operation, error) in [
                ("slide-body", slide_body(source, target).unwrap_err()),
                (
                    "rewrite",
                    rewrite_slide_body(source, target, new_body, &highlighter).unwrap_err(),
                ),
            ] {
                assert_eq!(error.kind, ErrorKind::Parse, "{case}/{operation}: kind");
                assert_eq!(
                    error.message, "bare CR line endings are not supported by preview editing",
                    "{case}/{operation}: message"
                );
                assert_eq!(
                    error.help,
                    "convert the deck to LF or CRLF line endings, then reload the preview",
                    "{case}/{operation}: help"
                );
            }
        }
    }

    #[test]
    fn slide_body_normalizes_crlf() {
        let source = "# T\r\n\r\ntext\r\n";
        let body = "# T\n\ntext";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();
        let target = &deck.parsed_slides()[0];

        assert_eq!(slide_body(source, target).unwrap(), body);
    }

    #[test]
    fn rewrite_slide_body_normalizes_submitted_bare_cr_to_lf() {
        let source = "# T\n";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();

        let rewritten = rewrite_slide_body(
            source,
            &deck.parsed_slides()[0],
            "# Changed\r\rtext\r",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten.source, "# Changed\n\ntext\n");
        assert_eq!(rewritten.body, "# Changed\n\ntext");
    }

    #[test]
    fn validate_reparsed_slide_body_rejects_changed_target_settings_comment_bytes() {
        let highlighter = Highlighter::defaults();
        let before_source = "<!-- {\"key\":\"fixed\"} -->\n# Title\n";
        let after_source = "<!-- { \"key\": \"fixed\" } -->\n# Title\n";
        let before = parse_source(before_source, &highlighter).unwrap();
        let after = parse_source(after_source, &highlighter).unwrap();

        let error = validate_reparsed_slide_body(
            before_source,
            &before,
            after_source,
            &after,
            0,
            "# Title",
        )
        .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(
            error.message,
            "slide body edit would change the edited slide's settings comment"
        );
    }

    #[test]
    fn edge_blank_runs_refuses_a_slide_without_a_nonblank_line() {
        let source = "\u{3000}\n";
        let error = edge_blank_runs(
            source,
            SourceSpan {
                start: 0,
                end: source.len(),
            },
        )
        .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(error.message, "slide has no non-blank line");
        assert_eq!(
            error.help,
            "edit the slide in a Markdown editor, then reload the preview"
        );
    }

    #[test]
    fn slide_body_removes_only_settings_and_note_comments() {
        let cases = [
            (
                "no-comments",
                "\n# Title\n\nBody\n\n",
                0,
                "# Title\n\nBody",
                None,
                0,
            ),
            (
                "settings-only",
                "<!-- {\"key\":\"fixed\"} -->\n\n# Title",
                0,
                "# Title",
                Some("<!-- {\"key\":\"fixed\"} -->\n"),
                0,
            ),
            (
                "note-before",
                "<!-- note -->\n# Title",
                0,
                "# Title",
                None,
                1,
            ),
            (
                "note-between",
                "Before\n<!-- note -->\nAfter",
                0,
                "Before\n\nAfter",
                None,
                1,
            ),
            (
                "note-inline",
                "Text <!-- note --> more",
                0,
                "Text  more",
                None,
                1,
            ),
            (
                "note-after",
                "# Title\n<!-- note -->",
                0,
                "# Title",
                None,
                1,
            ),
            (
                "after-frontmatter",
                "---\ntime: 1m\n---\n\n# Title\n",
                0,
                "# Title",
                None,
                0,
            ),
            (
                "crlf",
                "<!-- {\"key\":\"fixed\"} -->\r\n\r\n# Title\r\n",
                0,
                "# Title",
                Some("<!-- {\"key\":\"fixed\"} -->\r\n"),
                0,
            ),
            (
                "bom",
                "\u{feff}<!-- note -->\n# Title",
                0,
                "# Title",
                None,
                1,
            ),
            (
                "multiple-notes-retains-interior-whitespace",
                "<!-- first -->\nBefore\n<!-- second -->\n \t \nAfter\n<!-- third -->",
                0,
                "Before\n \t \nAfter",
                None,
                3,
            ),
            (
                "inline-settings-paragraph",
                "text <!-- {\"key\":\"a\"} -->",
                0,
                "text ",
                Some("<!-- {\"key\":\"a\"} -->"),
                0,
            ),
            (
                "inline-settings-heading",
                "# <!-- {\"key\":\"a\"} --> T",
                0,
                "#  T",
                Some("<!-- {\"key\":\"a\"} -->"),
                0,
            ),
            (
                "settings-indent-1",
                " <!-- {\"key\":\"a\"} -->\n# Title",
                0,
                "# Title",
                Some("<!-- {\"key\":\"a\"} -->\n"),
                0,
            ),
            (
                "settings-indent-2",
                "  <!-- {\"key\":\"a\"} -->\n# Title",
                0,
                "# Title",
                Some("<!-- {\"key\":\"a\"} -->\n"),
                0,
            ),
            (
                "settings-indent-3",
                "   <!-- {\"key\":\"a\"} -->\n# Title",
                0,
                "# Title",
                Some("<!-- {\"key\":\"a\"} -->\n"),
                0,
            ),
            (
                "settings-trailing-spaces",
                "<!-- {\"key\":\"a\"} -->   \n# Title",
                0,
                "# Title",
                Some("<!-- {\"key\":\"a\"} -->   \n"),
                0,
            ),
            (
                "settings-immediately-before-title",
                "<!-- {\"key\":\"a\"} -->\n# Title",
                0,
                "# Title",
                Some("<!-- {\"key\":\"a\"} -->\n"),
                0,
            ),
            (
                "repeated-settings-delimiters",
                "<!--<!-- {\"key\":\"a\"} -->\n# T\n",
                0,
                "# T",
                Some("<!--<!-- {\"key\":\"a\"} -->\n"),
                0,
            ),
            (
                "inline-repeated-settings-delimiters",
                "text <!--<!-- {\"key\":\"z\"} -->",
                0,
                "text ",
                Some("<!--<!-- {\"key\":\"z\"} -->"),
                0,
            ),
            (
                "non-first-slide-global-offsets",
                "# First\n\n---\n\n<!-- {\"key\":\"second\"} -->\n# Second\n\n<!-- note -->\nBody\n",
                1,
                "# Second\n\nBody",
                Some("<!-- {\"key\":\"second\"} -->\n"),
                1,
            ),
        ];

        let highlighter = Highlighter::defaults();
        for (name, source, slide_index, expected, expected_settings, note_count) in cases {
            let deck = parse_source(source, &highlighter).unwrap();
            let slide = &deck.parsed_slides()[slide_index];
            let (stripped, _) = strip_bom(source);
            let settings = slide
                .settings_span
                .map(|span| &stripped[span.start..span.end]);

            assert_eq!(settings, expected_settings, "{name}: settings span");
            assert_eq!(slide.note_spans.len(), note_count, "{name}: note spans");
            assert_eq!(slide_body(source, slide).unwrap(), expected, "{name}: body");
        }
    }

    #[test]
    fn slide_body_rejects_spans_that_do_not_match_the_source() {
        let parsed_source = "<!-- {\"key\":\"fixed\"} -->\n# Title\n<!-- note -->\n";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(parsed_source, &highlighter).unwrap();
        let slide = &deck.parsed_slides()[0];
        let settings = slide.settings_span.unwrap();
        let note = slide.note_spans[0];
        let in_range_non_comment = "x".repeat(parsed_source.len());
        let out_of_range = "# T";
        let mut note_as_settings = slide.clone();
        note_as_settings.settings_span = Some(note);
        note_as_settings.note_spans.clear();
        let mut settings_overlapping_note = slide.clone();
        settings_overlapping_note.note_spans = vec![settings];
        let swallowed_source = "<!-- {\"key\":\"a\"} -->\nBody\n<!-- n -->";
        let settings_source = format!("<!--\n{{\"key\":\"a\"}}{}\n-->", " ".repeat(16));
        assert_eq!(settings_source.len(), swallowed_source.len());
        let settings_deck = parse_source(&settings_source, &highlighter).unwrap();
        let swallowed_slide = &settings_deck.parsed_slides()[0];
        let settings_lf_source = "<!-- {\"key\":\"a\"} -->\n# T\n";
        let settings_lf_deck = parse_source(settings_lf_source, &highlighter).unwrap();
        let settings_lf_slide = &settings_lf_deck.parsed_slides()[0];
        assert_eq!(
            swallowed_slide.settings_span,
            Some(swallowed_slide.source_span)
        );

        assert!(settings.end <= in_range_non_comment.len());
        assert!(slide.source_span.end > out_of_range.len());

        for (name, source, candidate) in [
            ("in-range-non-comment", in_range_non_comment.as_str(), slide),
            ("out-of-range", out_of_range, slide),
            ("settings-points-at-note", parsed_source, &note_as_settings),
            (
                "settings-overlaps-note",
                parsed_source,
                &settings_overlapping_note,
            ),
            (
                "settings-span-swallows-following-text",
                swallowed_source,
                swallowed_slide,
            ),
            (
                "settings-span-ends-inside-crlf",
                "<!-- {\"key\":\"a\"} -->\r\n# T\n",
                settings_lf_slide,
            ),
        ] {
            let error = slide_body(source, candidate).unwrap_err();

            assert_eq!(error.kind, ErrorKind::Parse, "{name}: kind");
            assert_eq!(error.line, None, "{name}: line");
            assert_eq!(
                error.message, "slide body spans do not match the deck source",
                "{name}: message"
            );
            assert_eq!(error.help, "reload the preview and retry", "{name}: help");
        }
    }
}
