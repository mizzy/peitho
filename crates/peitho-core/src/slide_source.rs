//! Source extraction and rewriting for a single slide body.

use crate::{
    error::{BuildError, ErrorKind, Result},
    notes_edit::{normalized_note_text, remove_comment_spans, spans_match_source, strip_bom},
    phase::ParsedSlide,
};

/// Returns a slide's body Markdown without page settings or speaker notes.
///
/// Leading and trailing blank lines are removed, and line endings are
/// normalized to LF. A parse error is returned when the slide's recorded spans
/// do not match `source`.
pub fn slide_body(source: &str, slide: &ParsedSlide) -> Result<String> {
    let (source, _) = strip_bom(source);
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
    use super::slide_body;
    use crate::{
        error::ErrorKind,
        highlight::Highlighter,
        notes_edit::strip_bom,
        parser::{parse_frontmatter, parse_markdown},
    };

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

        for (name, source, slide_index, expected, expected_settings, note_count) in cases {
            let frontmatter = parse_frontmatter(source).unwrap();
            let deck = parse_markdown(source, frontmatter, &Highlighter::defaults()).unwrap();
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
        let frontmatter = parse_frontmatter(parsed_source).unwrap();
        let deck = parse_markdown(parsed_source, frontmatter, &Highlighter::defaults()).unwrap();
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
        let frontmatter = parse_frontmatter(&settings_source).unwrap();
        let settings_deck =
            parse_markdown(&settings_source, frontmatter, &Highlighter::defaults()).unwrap();
        let swallowed_slide = &settings_deck.parsed_slides()[0];
        let settings_lf_source = "<!-- {\"key\":\"a\"} -->\n# T\n";
        let frontmatter = parse_frontmatter(settings_lf_source).unwrap();
        let settings_lf_deck =
            parse_markdown(settings_lf_source, frontmatter, &Highlighter::defaults()).unwrap();
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
