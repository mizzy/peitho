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
    let after = parse_source(&candidate, highlighter)?;
    validate_rewrite(source, &before, &candidate, &after, target_index)?;
    let target_after = &after.parsed_slides()[target_index];
    let body = slide_body(&candidate, target_after)?;
    if body != normalized_body {
        return Err(round_trip_refusal());
    }

    Ok(SlideBodyRewrite {
        source: restore_bom(candidate, had_bom),
        key: target_after.key.clone(),
        body,
    })
}

fn parse_source(source: &str, highlighter: &Highlighter) -> Result<Deck<Parsed>> {
    let frontmatter = parse_frontmatter(source)?;
    parse_markdown(source, frontmatter, highlighter)
}

fn validate_rewrite(
    before_source: &str,
    before: &Deck<Parsed>,
    after_source: &str,
    after: &Deck<Parsed>,
    target_index: usize,
) -> Result<()> {
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

    Ok(())
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
        edge_blank_runs, parse_source, rewrite_slide_body, round_trip_refusal, slide_body,
        validate_rewrite,
    };
    use crate::{
        domain::{SlideKey, SourceSpan},
        error::ErrorKind,
        highlight::Highlighter,
        notes_edit::strip_bom,
        slide_compare::{compare_except_key, SlideDifference},
    };

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
    fn rewrite_slide_body_adds_blank_line_before_tight_separator_and_is_idempotent() {
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

        let repeated = rewrite_slide_body(
            &rewritten.source,
            &deck.parsed_slides()[0],
            "# A\n\ntext",
            &highlighter,
        )
        .unwrap();

        assert_eq!(repeated.source, rewritten.source);
        assert_eq!(repeated.body, rewritten.body);
    }

    #[test]
    fn rewrite_slide_body_blank_line_rule_preserves_existing_spacing_and_last_slide_eof() {
        let cases = [
            ("existing-blank-line", "# A\n\n---\n# B\n", 0, "# A"),
            ("last-slide-no-newline", "# A\n\n---\n\n# B", 1, "# B"),
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
    fn rewrite_slide_body_accepts_empty_body_with_metadata_and_is_idempotent() {
        let cases = [
            (
                "settings",
                "<!-- {\"key\":\"a\"} -->\n# T\n\n---\n\n# B\n",
                "<!-- {\"key\":\"a\"} -->\n\n---\n\n# B\n",
            ),
            (
                "note",
                "# T\n<!-- note -->\n\n---\n\n# B\n",
                "<!-- note -->\n\n---\n\n# B\n",
            ),
        ];
        let highlighter = Highlighter::defaults();

        for (name, source, expected) in cases {
            let deck = parse_source(source, &highlighter).unwrap();
            let rewritten =
                rewrite_slide_body(source, &deck.parsed_slides()[0], " \n\t\n", &highlighter)
                    .unwrap();

            assert_eq!(rewritten.source, expected, "{name}: first source");
            assert_eq!(rewritten.body, "", "{name}: first body");
            let deck = parse_source(&rewritten.source, &highlighter).unwrap();
            let repeated = rewrite_slide_body(
                &rewritten.source,
                &deck.parsed_slides()[0],
                " \n\t\n",
                &highlighter,
            )
            .unwrap();

            assert_eq!(repeated.source, rewritten.source, "{name}: repeated source");
            assert_eq!(repeated.body, "", "{name}: repeated body");
        }
    }

    #[test]
    fn rewrite_slide_body_refuses_empty_body_when_slide_disappears() {
        let source = "# A\n\n---\n\n# T\n\n---\n\n# B\n";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();

        let error = rewrite_slide_body(source, &deck.parsed_slides()[1], " \n\t\n", &highlighter)
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(
            error.message,
            "slide body edit would change the deck's slide count"
        );
        assert_eq!(
            error.help,
            "a `---` line in the body splits the slide, and removing all content removes it; take the separator out or keep some content, then retry"
        );
    }

    #[test]
    fn round_trip_refusal_has_actionable_help() {
        let error = round_trip_refusal();

        assert_eq!(
            error.message,
            "slide body edit did not round-trip through the Markdown parser"
        );
        assert_eq!(
            error.help,
            "the Markdown parser reads the saved text differently from what was typed, typically because of an unclosed code fence or HTML block; close it and retry"
        );
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
    fn slide_body_rewrite_preserves_crlf() {
        let source = "# T\r\n\r\ntext\r\n";
        let body = "# T\n\ntext";
        let highlighter = Highlighter::defaults();
        let deck = parse_source(source, &highlighter).unwrap();
        let target = &deck.parsed_slides()[0];

        assert_eq!(slide_body(source, target).unwrap(), body);
        let rewritten = rewrite_slide_body(source, target, body, &highlighter).unwrap();

        assert_eq!(rewritten.source, source);
        assert_eq!(rewritten.body, body);
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
    fn shared_comparison_reports_notes_before_layout_request() {
        let highlighter = Highlighter::defaults();
        let before = parse_source("# T\n<!-- before -->\n", &highlighter).unwrap();
        let after = parse_source(
            "<!-- {\"layout\":\"cover\"} -->\n# T\n<!-- after -->\n",
            &highlighter,
        )
        .unwrap();

        assert_eq!(
            compare_except_key(&before.parsed_slides()[0], &after.parsed_slides()[0]),
            Err(SlideDifference::Notes)
        );
    }

    #[test]
    fn validate_rewrite_rejects_changed_target_settings_comment_bytes_or_presence() {
        let highlighter = Highlighter::defaults();
        let cases = [
            (
                "bytes",
                "<!-- {\"key\":\"fixed\"} -->\n# Title\n",
                "<!-- { \"key\": \"fixed\" } -->\n# Title\n",
            ),
            (
                "presence",
                "# Title\n",
                "<!-- {\"key\":\"title\"} -->\n# Title\n",
            ),
        ];

        for (name, before_source, after_source) in cases {
            let before = parse_source(before_source, &highlighter).unwrap();
            let after = parse_source(after_source, &highlighter).unwrap();

            let error =
                validate_rewrite(before_source, &before, after_source, &after, 0).unwrap_err();

            assert_eq!(error.kind, ErrorKind::Parse, "{name}: kind");
            assert_eq!(
                error.message, "slide body edit would change the edited slide's settings comment",
                "{name}: message"
            );
        }
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
