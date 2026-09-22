//! Pure source rewriting for speaker notes on a single slide.

use crate::{
    domain::SourceSpan,
    error::{BuildError, ErrorKind, Result},
    highlight::Highlighter,
    parser::{
        is_page_settings_body, line_for_offset, page_settings_comment_body, parse_frontmatter,
        parse_markdown,
    },
    phase::{Deck, Parsed, ParsedSlide},
    slide_compare::{
        compare_all, compare_except_notes, compare_fragment_shape, HtmlBlockComparison,
    },
};
use std::ops::Range;

/// Returns whether a complete or partial Markdown line contains only ASCII
/// blank-line bytes.
pub(crate) fn is_ascii_blank_line(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

struct LineContext {
    start: usize,
    content_end: usize,
    end: usize,
    terminator: Option<Range<usize>>,
    whole_line: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct RemovalShapeContext<'a> {
    before: &'a ParsedSlide,
    highlighter: &'a Highlighter,
}

impl<'a> RemovalShapeContext<'a> {
    pub(crate) fn new(before: &'a ParsedSlide, highlighter: &'a Highlighter) -> Self {
        Self {
            before,
            highlighter,
        }
    }
}

enum PreservationFailure {
    FragmentShape,
    Other,
}

/// Returns the full source after a validated speaker-note rewrite for one slide.
///
/// A column-0 whole-line first span is replaced in place. For any other first
/// span, every span is removed and the note is appended after the slide's last
/// non-blank line.
/// The result is re-parsed and rejected unless every other slide is unchanged
/// and the target note equals the submitted text.
/// A leading UTF-8 BOM is stripped before rewriting and restored on return.
/// Text containing `-->` or whose trimmed form starts with `{` is rejected.
pub fn rewrite_note(
    source: &str,
    slide: SourceSpan,
    notes: &[SourceSpan],
    text: &str,
    highlighter: &Highlighter,
) -> Result<String> {
    let (source, had_bom) = strip_bom(source);
    if !spans_match_source(source, slide, None, notes) {
        return Err(BuildError::new(
            ErrorKind::Parse,
            None,
            "speaker note spans do not match the deck source",
            "reload the preview and retry",
        ));
    }

    let text = text.trim();
    let error_offset = notes.first().map(|span| span.start).unwrap_or_else(|| {
        let body = &source[slide.start..slide.end];
        slide.end - body.trim_start().len()
    });
    let error_line = Some(line_for_offset(source, error_offset));

    if text.contains("-->") {
        return Err(BuildError::new(
            ErrorKind::Parse,
            error_line,
            "speaker note cannot contain '-->'",
            "remove or rewrite '-->' because it closes the HTML comment",
        ));
    }
    if is_page_settings_body(text) {
        return Err(BuildError::new(
            ErrorKind::Parse,
            error_line,
            "speaker note cannot start with '{'",
            "start the note with another character so it is not parsed as page settings",
        ));
    }
    if notes.is_empty() && text.is_empty() {
        return Ok(restore_bom(source.to_owned(), had_bom));
    }

    let Some(before) = parse_source(source, highlighter) else {
        return Err(rewrite_refusal(error_line));
    };
    let Some(before_target) = before
        .parsed_slides()
        .iter()
        .find(|candidate| candidate.source_span.start == slide.start)
    else {
        return Err(rewrite_refusal(error_line));
    };
    let removal_context = RemovalShapeContext::new(before_target, highlighter);
    let candidate = splice_note(source, slide, notes, text, removal_context);
    let expected_text = normalized_note_text(text);
    match preserves_deck(&before, &candidate, slide, &expected_text, highlighter) {
        Ok(()) => {}
        Err(PreservationFailure::FragmentShape) => {
            return Err(BuildError::new(
                ErrorKind::Parse,
                error_line,
                "speaker note rewrite would change the edited slide's fragment shape",
                "move the note comment in the deck file, then reload the preview and retry",
            ));
        }
        Err(PreservationFailure::Other) => return Err(rewrite_refusal(error_line)),
    }

    Ok(restore_bom(candidate, had_bom))
}

fn rewrite_refusal(line: Option<usize>) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        line,
        "speaker note cannot be written at this position",
        "the rewritten deck would not parse back to the same slides; edit the note in the deck file",
    )
}

pub(crate) fn spans_match_source(
    source: &str,
    slide: SourceSpan,
    settings: Option<SourceSpan>,
    notes: &[SourceSpan],
) -> bool {
    fn comment_span(source: &str, slide: SourceSpan, span: SourceSpan) -> Option<&str> {
        if slide.start > span.start
            || span.start >= span.end
            || span.end > slide.end
            || !source.is_char_boundary(span.start)
            || !source.is_char_boundary(span.end)
            || source
                .as_bytes()
                .get(span.end.saturating_sub(1)..=span.end)
                .is_some_and(|ending| ending == b"\r\n")
        {
            return None;
        }

        let raw = &source[span.start..span.end];
        let body = raw.trim_end();
        if !raw.starts_with("<!--") || !body.ends_with("-->") || body.len() < 7 {
            return None;
        }
        let inner = &body[4..body.len() - 3];
        if inner
            .find("-->")
            .is_some_and(|close| inner[close..].contains('\n'))
        {
            return None;
        }

        let line = line_context(source, span);
        (slide.start <= line.start && line.end <= slide.end).then_some(raw)
    }

    if slide.end > source.len()
        || slide.start > slide.end
        || !source.is_char_boundary(slide.start)
        || !source.is_char_boundary(slide.end)
    {
        return false;
    }

    if !notes.windows(2).all(|spans| spans[0].end <= spans[1].start)
        || settings.is_some_and(|settings| {
            notes
                .iter()
                .any(|note| settings.start < note.end && note.start < settings.end)
        })
    {
        return false;
    }

    let notes_match = notes.iter().all(|span| {
        comment_span(source, slide, *span)
            .is_some_and(|raw| page_settings_comment_body(raw).is_none())
    });
    let settings_match = settings.is_none_or(|span| {
        comment_span(source, slide, span)
            .is_some_and(|raw| page_settings_comment_body(raw).is_some())
    });
    notes_match && settings_match
}

pub(crate) fn strip_bom(source: &str) -> (&str, bool) {
    if let Some(source) = source.strip_prefix('\u{feff}') {
        (source, true)
    } else {
        (source, false)
    }
}

pub(crate) fn restore_bom(source: String, had_bom: bool) -> String {
    if had_bom {
        ["\u{feff}", source.as_str()].concat()
    } else {
        source
    }
}

/// Span order does not matter; spans must be disjoint.
pub(crate) fn remove_comment_spans(
    source: &str,
    spans: &[SourceSpan],
    context: RemovalShapeContext<'_>,
) -> (String, usize) {
    let mut spans = spans.to_vec();
    spans.sort_by_key(|span| span.start);
    let mut rewritten = source.to_owned();
    let mut removed_bytes = 0;
    for span in spans.iter().rev() {
        let line = line_context(source, *span);
        let edit = removal_edit(&rewritten, *span, &line);
        let (range, replacement) = shape_preserving_removal_edit(&rewritten, &line, edit, context);
        removed_bytes += range.end - range.start - replacement.len();
        rewritten.replace_range(range, &replacement);
    }
    (rewritten, removed_bytes)
}

fn splice_note(
    source: &str,
    slide: SourceSpan,
    notes: &[SourceSpan],
    text: &str,
    removal_context: RemovalShapeContext<'_>,
) -> String {
    if notes.is_empty() {
        let line_ending = append_line_ending(source, slide);
        let comment = canonical_comment(text, line_ending);
        return append_comment(source, slide, &comment, line_ending);
    }

    let first_line = line_context(source, notes[0]);
    let in_place_comment = (!text.is_empty()
        && first_line.whole_line
        && notes[0].start == first_line.start)
        .then(|| {
            let line_ending = first_line
                .terminator
                .as_ref()
                .map(|terminator| terminator_line_ending(source, terminator))
                .unwrap_or_else(|| source_line_ending(source, slide));
            canonical_comment(text, line_ending)
        });

    if let Some(mut replacement) = in_place_comment {
        let (mut rewritten, _) = remove_comment_spans(source, &notes[1..], removal_context);
        if let Some(terminator) = &first_line.terminator {
            replacement.push_str(&source[terminator.clone()]);
        }
        rewritten.replace_range(notes[0].start..first_line.end, &replacement);
        return rewritten;
    }

    let (rewritten, removed_bytes) = remove_comment_spans(source, notes, removal_context);
    if !text.is_empty() {
        let adjusted_slide = SourceSpan {
            start: slide.start,
            end: slide.end - removed_bytes,
        };
        let line_ending = append_line_ending(&rewritten, adjusted_slide);
        let comment = canonical_comment(text, line_ending);
        return append_comment(&rewritten, adjusted_slide, &comment, line_ending);
    }

    rewritten
}

pub(crate) fn source_line_ending(source: &str, slide: SourceSpan) -> &'static str {
    if source[slide.start..slide.end].contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn append_line_ending(source: &str, slide: SourceSpan) -> &'static str {
    last_nonblank_line_end(source, slide)
        .and_then(|end| {
            let tail = &source[end..slide.end];
            if tail.starts_with("\r\n") {
                Some("\r\n")
            } else if tail.starts_with('\n') {
                Some("\n")
            } else {
                None
            }
        })
        .unwrap_or_else(|| source_line_ending(source, slide))
}

fn terminator_line_ending(source: &str, terminator: &Range<usize>) -> &'static str {
    if source[terminator.clone()].starts_with('\r') {
        "\r\n"
    } else {
        "\n"
    }
}

pub(crate) fn normalized_note_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub(crate) fn canonical_comment(text: &str, line_ending: &str) -> String {
    let normalized = normalized_note_text(text);
    if normalized.contains('\n') {
        let body = normalized.replace('\n', line_ending);
        format!("<!--{line_ending}{body}{line_ending}-->")
    } else {
        format!("<!-- {normalized} -->")
    }
}

fn preserves_deck(
    before: &Deck<Parsed>,
    candidate: &str,
    slide: SourceSpan,
    expected_text: &str,
    highlighter: &Highlighter,
) -> std::result::Result<(), PreservationFailure> {
    let Some(after) = parse_source(candidate, highlighter) else {
        return Err(PreservationFailure::Other);
    };
    let before_slides = before.parsed_slides();
    let after_slides = after.parsed_slides();
    if before_slides.len() != after_slides.len()
        || before.settings().sections() != after.settings().sections()
    {
        return Err(PreservationFailure::Other);
    }

    let Some(before_target) = before_slides
        .iter()
        .position(|candidate| candidate.source_span.start == slide.start)
    else {
        return Err(PreservationFailure::Other);
    };
    let Some(after_target) = after_slides
        .iter()
        .position(|candidate| candidate.source_span.start == slide.start)
    else {
        return Err(PreservationFailure::Other);
    };
    if before_target != after_target {
        return Err(PreservationFailure::Other);
    }

    for (index, (before, after)) in before_slides.iter().zip(after_slides).enumerate() {
        if index == before_target {
            if compare_except_notes(before, after).is_err() {
                return Err(PreservationFailure::Other);
            }
            if !fragment_shape_matches(before, after) {
                return Err(PreservationFailure::FragmentShape);
            }
        } else if compare_all(before, after).is_err() {
            return Err(PreservationFailure::Other);
        }
    }

    if after_slides[after_target].notes.as_deref()
        != (!expected_text.is_empty()).then_some(expected_text)
    {
        return Err(PreservationFailure::Other);
    }

    Ok(())
}

fn parse_source(source: &str, highlighter: &Highlighter) -> Option<Deck<Parsed>> {
    let frontmatter = parse_frontmatter(source).ok()?;
    parse_markdown(source, frontmatter, highlighter).ok()
}

fn fragment_shape_matches(before: &ParsedSlide, after: &ParsedSlide) -> bool {
    compare_fragment_shape(
        &before.fragments,
        &after.fragments,
        HtmlBlockComparison::Ignore,
    )
    .is_equal()
}

fn line_context(source: &str, span: SourceSpan) -> LineContext {
    let bytes = source.as_bytes();
    let start = bytes[..span.start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |newline| newline + 1);

    let contained_terminator = if span.end > span.start && bytes[span.end - 1] == b'\n' {
        Some(line_terminator(source, span.end - 1))
    } else {
        None
    };

    let content_end = contained_terminator
        .as_ref()
        .map_or(span.end, |terminator| terminator.start);
    let terminator = contained_terminator.or_else(|| {
        bytes[span.end..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|relative_newline| line_terminator(source, span.end + relative_newline))
    });
    let line_content_end = terminator
        .as_ref()
        .map_or(source.len(), |terminator| terminator.start);
    let end = terminator
        .as_ref()
        .map_or(line_content_end, |terminator| terminator.end);
    let whole_line = source[start..span.start]
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'>'))
        && source
            .get(content_end..line_content_end)
            .is_some_and(is_ascii_blank_line);

    LineContext {
        start,
        content_end,
        end,
        terminator,
        whole_line,
    }
}

fn line_terminator(source: &str, newline: usize) -> Range<usize> {
    let start = if newline > 0 && source.as_bytes()[newline - 1] == b'\r' {
        newline - 1
    } else {
        newline
    };
    start..newline + 1
}

// Removing a whole line between non-blank neighbours could join `text` to `---`
// into a setext heading that merges slides; blockquotes retain `>` for the same reason.
// See docs/specs/2026-09-12-preview-notes-edit-design.md.
fn removal_edit(source: &str, span: SourceSpan, line: &LineContext) -> (Range<usize>, String) {
    if !line.whole_line {
        return (span.start..line.content_end, String::new());
    }

    let prefix = source[line.start..span.start].trim_end();
    match &line.terminator {
        Some(terminator) if line_has_nonblank_neighbors(source, line) => (
            line.start..line.end,
            [prefix, &source[terminator.clone()]].concat(),
        ),
        _ => (line.start..line.end, String::new()),
    }
}

fn shape_preserving_removal_edit(
    source: &str,
    line: &LineContext,
    current: (Range<usize>, String),
    context: RemovalShapeContext<'_>,
) -> (Range<usize>, String) {
    if !line.whole_line || current.1.is_empty() {
        return current;
    }

    if removal_preserves_fragment_shape(source, &current, context) {
        return current;
    }

    let deletion = (line.start..line.end, String::new());
    if removal_preserves_fragment_shape(source, &deletion, context) {
        deletion
    } else {
        current
    }
}

fn removal_preserves_fragment_shape(
    source: &str,
    edit: &(Range<usize>, String),
    context: RemovalShapeContext<'_>,
) -> bool {
    let mut candidate = source.to_owned();
    candidate.replace_range(edit.0.clone(), &edit.1);
    let Some(after) = parse_source(&candidate, context.highlighter) else {
        return false;
    };
    after
        .parsed_slides()
        .iter()
        .find(|after| after.source_span.start == context.before.source_span.start)
        .is_some_and(|after| fragment_shape_matches(context.before, after))
}

fn line_has_nonblank_neighbors(source: &str, line: &LineContext) -> bool {
    let bytes = source.as_bytes();
    let Some(previous_newline) = line
        .start
        .checked_sub(1)
        .filter(|index| bytes[*index] == b'\n')
    else {
        return false;
    };
    let previous_end = line_terminator(source, previous_newline).start;
    let previous_start = bytes[..previous_end]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |newline| newline + 1);
    if is_ascii_blank_line(&source[previous_start..previous_end]) || line.end >= source.len() {
        return false;
    }

    let next_end = bytes[line.end..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(source.len(), |relative_newline| {
            line_terminator(source, line.end + relative_newline).start
        });
    !is_ascii_blank_line(&source[line.end..next_end])
}

fn append_comment(source: &str, slide: SourceSpan, comment: &str, line_ending: &str) -> String {
    let two_terminators = [line_ending, line_ending].concat();
    let (cut, separator) = last_nonblank_line_end(source, slide)
        .map_or((slide.start, ""), |end| (end, two_terminators.as_str()));
    [
        &source[..cut],
        separator,
        comment,
        line_ending,
        &source[slide.end..],
    ]
    .concat()
}

pub(crate) fn last_nonblank_line_end(source: &str, slide: SourceSpan) -> Option<usize> {
    let body = &source[slide.start..slide.end];
    let kept = body.trim_end_matches([' ', '\t', '\r', '\n']).len();
    (kept > 0).then(|| {
        slide.start
            + body[kept..]
                .find(['\r', '\n'])
                .map_or(body.len(), |index| kept + index)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::ErrorKind,
        highlight::Highlighter,
        parser::{parse_frontmatter, parse_markdown},
        phase::{Deck, Parsed},
    };

    fn parse(source: &str, highlighter: &Highlighter) -> Deck<Parsed> {
        let frontmatter = parse_frontmatter(source).unwrap();
        parse_markdown(source, frontmatter, highlighter).unwrap()
    }

    fn assert_idempotent(once: &str, text: &str, highlighter: &Highlighter, slide_index: usize) {
        let reparsed = parse(once, highlighter);
        let slide = &reparsed.parsed_slides()[slide_index];
        assert_eq!(
            rewrite_note(
                once,
                slide.source_span,
                &slide.note_spans,
                text,
                highlighter,
            )
            .unwrap(),
            once
        );
    }

    #[test]
    fn rewrite_note_formats_single_and_multiline_text() {
        let source = "# Title\n\n<!-- old note -->\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];

        assert_eq!(
            rewrite_note(
                source,
                slide.source_span,
                &slide.note_spans,
                "  one line  ",
                &highlighter,
            )
            .unwrap(),
            "# Title\n\n<!-- one line -->\n"
        );
        assert_eq!(
            rewrite_note(
                source,
                slide.source_span,
                &slide.note_spans,
                " \r\nline one\rline two\n ",
                &highlighter,
            )
            .unwrap(),
            "# Title\n\n<!--\nline one\nline two\n-->\n"
        );
    }

    #[test]
    fn rewrite_note_accepts_two_comments_on_one_line() {
        let source = "# T\n\n<!-- a --> <!-- b -->\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.note_spans.len(), 1);

        let once = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "one",
            &highlighter,
        )
        .unwrap();

        assert_eq!(once, "# T\n\n<!-- one -->\n");
        assert_idempotent(&once, "one", &highlighter, 0);
    }

    #[test]
    fn rewrite_note_replaces_first_span_and_removes_later_spans() {
        let source = concat!(
            "<!-- {\"key\":\"intro\"} -->\n",
            "# Title\n",
            "<!-- first note -->\n",
            "- item\n",
            "  <!-- second note -->\n",
            "\n",
            "Text <!-- inline note --> more\n",
        );
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];

        assert_eq!(slide.note_spans.len(), 3);
        assert_eq!(
            rewrite_note(
                source,
                slide.source_span,
                &slide.note_spans,
                "edited",
                &highlighter,
            )
            .unwrap(),
            concat!(
                "<!-- {\"key\":\"intro\"} -->\n",
                "# Title\n",
                "<!-- edited -->\n",
                "- item\n",
                "\n",
                "Text  more\n",
            )
        );
    }

    #[test]
    fn remove_comment_spans_accepts_disjoint_spans_in_any_order() {
        let source = "# T\n<!-- first -->\nBody\n<!-- second -->\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let mut spans = deck.parsed_slides()[0].note_spans.clone();
        spans.reverse();

        let (rewritten, removed_bytes) = remove_comment_spans(
            source,
            &spans,
            RemovalShapeContext::new(&deck.parsed_slides()[0], &highlighter),
        );

        assert_eq!(rewritten, "# T\n\nBody\n");
        assert_eq!(removed_bytes, source.len() - rewritten.len());
    }

    #[test]
    fn rewrite_note_removal_between_nonblank_lines_keeps_a_blank_line() {
        let source = "# A\n\ntext\n<!-- note -->\n---\n# B\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let rewritten = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten, "# A\n\ntext\n\n---\n# B\n");
        assert_eq!(parse(&rewritten, &highlighter).parsed_slides().len(), 2);

        let source = "> para\n> <!-- x -->\n> more\n";
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let rewritten = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten, "> para\n>\n> more\n");
    }

    #[test]
    fn rewrite_note_keeps_list_tight_when_relocating_indented_note() {
        let source = "# T\n\n- item\n  <!-- note -->\n- next\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];

        let rewritten = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "edited",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten, "# T\n\n- item\n- next\n\n<!-- edited -->\n");
    }

    #[test]
    fn rewrite_note_keeps_paragraphs_separate_when_relocating_note() {
        let source = "# T\n\npara1\n <!-- note -->\npara2\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];

        let rewritten = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "edited",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten, "# T\n\npara1\n\npara2\n\n<!-- edited -->\n");
    }

    #[test]
    fn rewrite_note_refuses_when_no_removal_preserves_fragment_shape() {
        let source = "# T\n\ntext\n> <!-- note -->\nmore\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];

        let error = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "edited",
            &highlighter,
        )
        .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(error.line, Some(4));
        assert_eq!(
            error.message,
            "speaker note rewrite would change the edited slide's fragment shape"
        );
        assert!(!error.help.is_empty());
    }

    #[test]
    fn rewrite_note_removal_treats_unicode_whitespace_lines_as_nonblank_neighbors() {
        let highlighter = Highlighter::defaults();
        let cases = [
            (
                "ideographic-space-before",
                "\u{3000}\n<!-- note -->\nafter\n",
                "\u{3000}\n\nafter\n",
            ),
            (
                "nbsp-after",
                "before\n<!-- note -->\n\u{a0}\n",
                "before\n\n\u{a0}\n",
            ),
        ];

        for (name, source, expected) in cases {
            let deck = parse(source, &highlighter);
            let slide = &deck.parsed_slides()[0];
            let rewritten = rewrite_note(
                source,
                slide.source_span,
                &slide.note_spans,
                "",
                &highlighter,
            )
            .unwrap();

            assert_eq!(rewritten, expected, "{name}");
        }
    }

    #[test]
    fn rewrite_note_relocates_container_and_inline_comments_to_slide_end() {
        let highlighter = Highlighter::defaults();
        let cases = [
            (
                "> <!-- old -->\n\n- item\n",
                "new",
                "\n- item\n\n<!-- new -->\n",
            ),
            (
                "Text <!-- old --> more\n",
                "a\nb",
                "Text  more\n\n<!--\na\nb\n-->\n",
            ),
            (
                "- item\n  <!-- old -->\n",
                "a\nb",
                "- item\n\n<!--\na\nb\n-->\n",
            ),
            (" <!-- old -->\n# T\n", "new", "# T\n\n<!-- new -->\n"),
        ];

        for (source, text, expected) in cases {
            let deck = parse(source, &highlighter);
            let slide = &deck.parsed_slides()[0];
            let once = rewrite_note(
                source,
                slide.source_span,
                &slide.note_spans,
                text,
                &highlighter,
            )
            .unwrap();

            assert_eq!(once, expected);

            assert_idempotent(&once, text, &highlighter, 0);
        }
    }

    #[test]
    fn rewrite_note_empty_removes_every_note_span() {
        let source = concat!(
            "# Title\n",
            "<!-- first note -->\n",
            "Text <!-- inline note --> more\n",
            "  <!-- last note -->\n",
        );
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];

        assert_eq!(slide.note_spans.len(), 3);
        assert_eq!(
            rewrite_note(
                source,
                slide.source_span,
                &slide.note_spans,
                " \r\n\t ",
                &highlighter,
            )
            .unwrap(),
            "# Title\n\nText  more\n"
        );

        let source = "> <!-- old -->\n\n- item\n";
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let once = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "",
            &highlighter,
        )
        .unwrap();

        assert_eq!(once, "\n- item\n");
        assert_idempotent(&once, "", &highlighter, 0);
    }

    #[test]
    fn rewrite_note_appends_after_last_nonblank_line() {
        let source = "# Title\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let rewritten = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "new note",
            &highlighter,
        )
        .unwrap();

        assert_eq!(rewritten, "# Title\n\n<!-- new note -->\n");
    }

    #[test]
    fn rewrite_note_append_preserves_unicode_whitespace_trailing_paragraphs() {
        let highlighter = Highlighter::defaults();

        for (name, whitespace) in [("nbsp", "\u{a0}"), ("ideographic-space", "\u{3000}")] {
            let source = format!("# I1\n\ntext\n\n{whitespace}\n");
            let expected = format!("# I1\n\ntext\n\n{whitespace}\n\n<!-- new -->\n");
            let deck = parse(&source, &highlighter);
            let slide = &deck.parsed_slides()[0];
            let rewritten = rewrite_note(
                &source,
                slide.source_span,
                &slide.note_spans,
                "new",
                &highlighter,
            )
            .unwrap();

            assert_eq!(rewritten, expected, "{name}");
        }
    }

    #[test]
    fn rewrite_note_in_place_preserves_unicode_whitespace_trailing_paragraphs() {
        let highlighter = Highlighter::defaults();

        for (name, whitespace) in [("nbsp", "\u{a0}"), ("ideographic-space", "\u{3000}")] {
            let source = format!("# I1\n\ntext\n\n{whitespace}\n\n<!-- old -->\n");
            let expected = format!("# I1\n\ntext\n\n{whitespace}\n\n<!-- new -->\n");
            let deck = parse(&source, &highlighter);
            let slide = &deck.parsed_slides()[0];
            assert_eq!(slide.note_spans.len(), 1, "{name}: note span");

            let rewritten = rewrite_note(
                &source,
                slide.source_span,
                &slide.note_spans,
                "new",
                &highlighter,
            )
            .unwrap();

            assert_eq!(rewritten, expected, "{name}");
        }
    }

    #[test]
    fn rewrite_note_rejects_unparseable_source() {
        let highlighter = Highlighter::defaults();
        let error = rewrite_note(
            " \n\n",
            SourceSpan { start: 0, end: 3 },
            &[],
            "new note",
            &highlighter,
        )
        .unwrap_err();
        assert_eq!(error.line, Some(3));
        assert_eq!(
            error.message,
            "speaker note cannot be written at this position"
        );
    }

    #[test]
    fn rewrite_note_accepts_line_count_changes_on_middle_slides() {
        const PREFIX: &str = concat!(
            "---\n",
            "time: 2m\n",
            "---\n",
            "<!-- {\"section\":\"First\",\"time\":\"1m\"} -->\n",
            "# A\n\n",
            "---\n",
            "# B",
        );
        const SUFFIX: &str = concat!(
            "---\n",
            "<!-- {\"key\":\"later\",\"layout\":\"cover\",\"section\":\"Later\",\"time\":\"1m\"} -->\n",
            "# C\n",
        );
        let cases = [
            (
                [PREFIX, "\n\n", SUFFIX].concat(),
                "new",
                [PREFIX, "\n\n<!-- new -->\n", SUFFIX].concat(),
            ),
            (
                [PREFIX, "\n\n<!-- old -->\n\n", SUFFIX].concat(),
                "a\nb",
                [PREFIX, "\n\n<!--\na\nb\n-->\n\n", SUFFIX].concat(),
            ),
            (
                [PREFIX, "\n\nText <!-- old --> more\n\n", SUFFIX].concat(),
                "new",
                [PREFIX, "\n\nText  more\n\n<!-- new -->\n", SUFFIX].concat(),
            ),
            (
                [PREFIX, "\n\n<!--\na\nb\n-->\n\n", SUFFIX].concat(),
                "new",
                [PREFIX, "\n\n<!-- new -->\n\n", SUFFIX].concat(),
            ),
        ];
        let highlighter = Highlighter::defaults();

        for (source, text, expected) in cases {
            let before = parse(&source, &highlighter);
            let target = &before.parsed_slides()[1];
            let later = &before.parsed_slides()[2];
            assert!(matches!(
                later.key_source,
                crate::phase::KeySource::Explicit { .. }
            ));
            assert_eq!(
                later
                    .layout_request
                    .as_ref()
                    .map(|request| request.name.as_str()),
                Some("cover")
            );
            assert_eq!(before.settings().sections()[1].start(), 2);

            let once = rewrite_note(
                &source,
                target.source_span,
                &target.note_spans,
                text,
                &highlighter,
            )
            .unwrap();
            assert_eq!(once, expected);

            assert_idempotent(&once, text, &highlighter, 1);
        }
    }

    #[test]
    fn rewrite_note_preserves_crlf_and_is_idempotent() {
        let source = "# Title\r\n\r\n<!-- old\r\nnote -->\r\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let text = "line one\r\nline two";
        let once = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            text,
            &highlighter,
        )
        .unwrap();

        assert_eq!(
            once,
            "# Title\r\n\r\n<!--\r\nline one\r\nline two\r\n-->\r\n"
        );
        assert!(once
            .as_bytes()
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte != b'\n'
                || (index > 0 && once.as_bytes()[index - 1] == b'\r')));

        assert_idempotent(&once, text, &highlighter, 0);
    }

    #[test]
    fn rewrite_note_uses_the_slides_own_line_ending() {
        let source = concat!(
            "<!-- {\"key\":\"first\"} -->\r\n",
            "# First\r\n\r\n",
            "<!-- old first -->\r\n",
            "---\n",
            "<!-- {\"key\":\"second\"} -->\n",
            "# Second\n\n",
            "<!-- old second -->\n",
        );
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);

        let second = &deck.parsed_slides()[1];
        assert_eq!(
            rewrite_note(
                source,
                second.source_span,
                &second.note_spans,
                "new\nnote",
                &highlighter,
            )
            .unwrap(),
            concat!(
                "<!-- {\"key\":\"first\"} -->\r\n",
                "# First\r\n\r\n",
                "<!-- old first -->\r\n",
                "---\n",
                "<!-- {\"key\":\"second\"} -->\n",
                "# Second\n\n",
                "<!--\nnew\nnote\n-->\n",
            )
        );

        let first = &deck.parsed_slides()[0];
        assert_eq!(
            rewrite_note(
                source,
                first.source_span,
                &first.note_spans,
                "new\nnote",
                &highlighter,
            )
            .unwrap(),
            concat!(
                "<!-- {\"key\":\"first\"} -->\r\n",
                "# First\r\n\r\n",
                "<!--\r\nnew\r\nnote\r\n-->\r\n",
                "---\n",
                "<!-- {\"key\":\"second\"} -->\n",
                "# Second\n\n",
                "<!-- old second -->\n",
            )
        );
    }

    #[test]
    fn rewrite_note_takes_the_terminator_from_the_replaced_line() {
        let source = "<!-- {\"key\":\"a\"} -->\r\n# A\n\n<!-- old -->\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];

        assert_eq!(
            rewrite_note(
                source,
                slide.source_span,
                &slide.note_spans,
                "m1\nm2",
                &highlighter,
            )
            .unwrap(),
            "<!-- {\"key\":\"a\"} -->\r\n# A\n\n<!--\nm1\nm2\n-->\n"
        );

        let append_source = "<!-- {\"key\":\"append\"} -->\n# Append\r\n";
        let deck = parse(append_source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        assert_eq!(
            rewrite_note(
                append_source,
                slide.source_span,
                &slide.note_spans,
                "m1\nm2",
                &highlighter,
            )
            .unwrap(),
            concat!(
                "<!-- {\"key\":\"append\"} -->\n",
                "# Append\r\n\r\n",
                "<!--\r\nm1\r\nm2\r\n-->\r\n",
            )
        );
    }

    #[test]
    fn rewrite_note_append_ending_ignores_removed_note_lines() {
        let highlighter = Highlighter::defaults();
        let cases = [
            (
                "# A\nbody\n\n> <!-- n -->\r\n",
                "# A\nbody\n\n<!--\na\nb\n-->\n",
            ),
            (
                "# A\nbody <!-- first --> text\n\n> <!-- last -->\r\n",
                "# A\nbody  text\n\n<!--\na\nb\n-->\n",
            ),
        ];

        for (source, expected) in cases {
            let deck = parse(source, &highlighter);
            let slide = &deck.parsed_slides()[0];

            assert_eq!(
                rewrite_note(
                    source,
                    slide.source_span,
                    &slide.note_spans,
                    "a\nb",
                    &highlighter,
                )
                .unwrap(),
                expected
            );
        }
    }

    #[test]
    fn rewrite_note_rejects_comment_close_and_page_settings_prefix() {
        let source = "# Title\n\n<!-- old note -->\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let close_error = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "bad --> note",
            &highlighter,
        )
        .unwrap_err();

        assert_eq!(close_error.kind, ErrorKind::Parse);
        assert_eq!(close_error.line, Some(3));
        assert_eq!(close_error.message, "speaker note cannot contain '-->'");
        assert!(!close_error.help.is_empty());

        let source = "---\ntime: 1m\n---\n\n# Title\n";
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        assert!(source[slide.source_span.start..].starts_with("\n\n# Title"));
        let settings_error = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "  {not a note}  ",
            &highlighter,
        )
        .unwrap_err();

        assert_eq!(settings_error.kind, ErrorKind::Parse);
        assert_eq!(settings_error.line, Some(5));
        assert_eq!(settings_error.message, "speaker note cannot start with '{'");
        assert!(!settings_error.help.is_empty());
    }

    #[test]
    fn rewrite_note_rejects_note_swallowed_by_unclosed_fence() {
        let source = "# T\n\n```rust\ncode\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let error = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "note",
            &highlighter,
        )
        .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(error.line, Some(1));
        assert_eq!(
            error.message,
            "speaker note cannot be written at this position"
        );
        assert!(!error.help.is_empty());
    }

    #[test]
    fn rewrite_note_rejects_spans_that_do_not_match_the_source() {
        let highlighter = Highlighter::defaults();
        let assert_rejected = |source: &str, slide: SourceSpan, notes: &[SourceSpan]| {
            let error = rewrite_note(source, slide, notes, "new", &highlighter).unwrap_err();

            assert_eq!(error.kind, ErrorKind::Parse);
            assert_eq!(error.line, None);
            assert_eq!(
                error.message,
                "speaker note spans do not match the deck source"
            );
            assert_eq!(error.help, "reload the preview and retry");
        };

        let source = "# T\n";
        assert_rejected(
            source,
            SourceSpan {
                start: 0,
                end: source.len() + 1,
            },
            &[],
        );

        let source = "# T\n\n<!-- old -->\n";
        let note_start = source.find("<!--").unwrap();
        assert_rejected(
            source,
            SourceSpan {
                start: 0,
                end: source.len(),
            },
            &[SourceSpan {
                start: note_start,
                end: source.len() + 1,
            }],
        );

        let source = "こ<!-- old -->\n";
        assert_rejected(
            source,
            SourceSpan {
                start: 1,
                end: source.len(),
            },
            &[],
        );
        assert_rejected(
            source,
            SourceSpan {
                start: 0,
                end: source.len(),
            },
            &[SourceSpan {
                start: 1,
                end: source.len(),
            }],
        );

        let source = "# T\n<!-- first -->\ntext\n<!-- second -->\n";
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let unsorted = [slide.note_spans[1], slide.note_spans[0]];
        assert_rejected(source, slide.source_span, &unsorted);

        let source = "<!-- <!-- inner --> -->\n";
        let inner_start = source.rfind("<!--").unwrap();
        let overlapping = [
            SourceSpan {
                start: 0,
                end: source.len(),
            },
            SourceSpan {
                start: inner_start,
                end: inner_start + "<!-- inner -->".len(),
            },
        ];
        assert_rejected(
            source,
            SourceSpan {
                start: 0,
                end: source.len(),
            },
            &overlapping,
        );

        let source = "# T\n\ndelete me\n";
        let start = source.find("delete me").unwrap();
        assert_rejected(
            source,
            SourceSpan {
                start: 0,
                end: source.len(),
            },
            &[SourceSpan {
                start,
                end: start + "delete me".len(),
            }],
        );

        let source = "# T\n\n<!-- old -->\n";
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let mut shifted = slide.note_spans[0];
        shifted.start += 1;
        assert_rejected(source, slide.source_span, &[shifted]);

        let source = ">  <!-- a -->\n";
        assert_rejected(
            source,
            SourceSpan { start: 1, end: 14 },
            &[SourceSpan { start: 3, end: 13 }],
        );

        let source = ">  <!-- a -->  \n";
        assert_rejected(
            source,
            SourceSpan { start: 1, end: 13 },
            &[SourceSpan { start: 3, end: 13 }],
        );

        let source = "<!-- a -->\ncontent\n<!-- b -->";
        assert_rejected(
            source,
            SourceSpan {
                start: 0,
                end: source.len(),
            },
            &[SourceSpan {
                start: 0,
                end: source.len(),
            }],
        );

        let source = "<!-- {\"key\":\"a\"} -->\n";
        assert_rejected(
            source,
            SourceSpan {
                start: 0,
                end: source.len(),
            },
            &[SourceSpan {
                start: 0,
                end: source.len(),
            }],
        );

        let note_lf_source = "# T\n<!-- n -->\n";
        let note_lf_deck = parse(note_lf_source, &highlighter);
        let note_lf_slide = &note_lf_deck.parsed_slides()[0];
        assert_rejected(
            "# T\n<!-- n -->\r\n",
            note_lf_slide.source_span,
            &note_lf_slide.note_spans,
        );
    }

    #[test]
    fn rewrite_note_empty_without_spans_is_unparsed_no_op() {
        let source = "\u{feff}# T\n";
        let highlighter = Highlighter::defaults();

        assert_eq!(
            rewrite_note(
                source,
                SourceSpan { start: 0, end: 4 },
                &[],
                " \t ",
                &highlighter,
            )
            .unwrap(),
            source
        );
    }

    #[test]
    fn rewrite_note_strips_and_restores_bom() {
        let source = "\u{feff}# T\n\n<!-- old -->\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[0];
        let rewritten = rewrite_note(
            source,
            slide.source_span,
            &slide.note_spans,
            "new",
            &highlighter,
        )
        .unwrap();

        assert!(rewritten.starts_with('\u{feff}'));
        assert_eq!(rewritten, "\u{feff}# T\n\n<!-- new -->\n");
    }

    #[test]
    fn rewrite_note_reparse_preserves_slides_keys_settings_and_other_bytes() {
        let source = concat!(
            "---\n",
            "time: 4m\n",
            "page_numbers: current\n",
            "---\n",
            "<!-- {\"key\":\"cover\",\"layout\":\"cover\",\"section\":\"Setup\",\"time\":\"2m\"} -->\n",
            "# Cover\n\n",
            "<!-- cover note -->\n\n",
            "---\n",
            "<!-- {\"key\":\"details\"} -->\n",
            "# Details\n\n",
            "<!-- old detail -->\n\n",
            "---\n",
            "<!-- {\"key\":\"draft\",\"draft\":true} -->\n",
            "# Draft\n\n",
            "<!-- draft note -->\n\n",
            "---\n",
            "<!-- {\"key\":\"appendix\",\"section\":\"Appendix\",\"time\":\"2m\",\"skip\":true,\"page_number\":false} -->\n",
            "# Appendix\n\n",
            "---\n",
            "<!-- {\"key\":\"end\"} -->\n",
            "# End\n\n",
            "<!-- end note -->\n",
        );
        let highlighter = Highlighter::defaults();
        let before = parse(source, &highlighter);
        let target = &before.parsed_slides()[1];
        let rewritten = rewrite_note(
            source,
            target.source_span,
            &target.note_spans,
            "edited",
            &highlighter,
        )
        .unwrap();
        let after = parse(&rewritten, &highlighter);

        assert_eq!(
            after
                .parsed_slides()
                .iter()
                .map(|slide| &slide.key)
                .collect::<Vec<_>>(),
            before
                .parsed_slides()
                .iter()
                .map(|slide| &slide.key)
                .collect::<Vec<_>>()
        );
        assert_eq!(after.parsed_slides()[1].notes.as_deref(), Some("edited"));
        assert_eq!(
            after.parsed_slides()[0].layout_request,
            before.parsed_slides()[0].layout_request
        );
        assert_eq!(
            after.parsed_slides()[2].skip,
            before.parsed_slides()[2].skip
        );
        assert!(rewritten.starts_with(&source[..target.source_span.start]));
        assert!(rewritten.ends_with(&source[target.source_span.end..]));

        assert_eq!(before.parsed_slides().len(), 4);
        assert_eq!(after.parsed_slides().len(), before.parsed_slides().len());
        assert_eq!(
            after
                .parsed_slides()
                .iter()
                .map(|slide| slide.source_index)
                .collect::<Vec<_>>(),
            before
                .parsed_slides()
                .iter()
                .map(|slide| slide.source_index)
                .collect::<Vec<_>>()
        );
        assert_eq!(after.settings().sections(), before.settings().sections());
        assert!(before.parsed_slides()[2].page_number_hidden);

        for (index, (after_slide, before_slide)) in after
            .parsed_slides()
            .iter()
            .zip(before.parsed_slides())
            .enumerate()
        {
            assert_eq!(after_slide.key_source, before_slide.key_source);
            assert_eq!(after_slide.layout_request, before_slide.layout_request);
            assert_eq!(after_slide.skip, before_slide.skip);
            assert_eq!(
                after_slide.page_number_hidden,
                before_slide.page_number_hidden
            );
            if index != 1 {
                assert_eq!(after_slide.notes, before_slide.notes);
            }
        }
    }
}
