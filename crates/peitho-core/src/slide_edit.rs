//! Pure source rewriting for one parser-authorized slide block.

use crate::{
    domain::{EditableBlockKind, EditableSpan, SourceSpan},
    error::{BuildError, ErrorKind, Result},
    highlight::Highlighter,
    notes_edit::{normalized_note_text, restore_bom, strip_bom},
    parser::{line_for_offset, parse_frontmatter, parse_markdown},
    phase::{Deck, Parsed, ParsedSlide},
    slide_compare::{
        compare_all, compare_except_key, compare_fragment_shape, target_key_change_allowed,
        FragmentShapeComparison, HtmlBlockComparison,
    },
};

const STRUCTURAL_EDIT_HELP: &str =
    "make structural changes in the Markdown editor, then reload the preview and retry";

/// One validated block rewrite: the full rewritten source and the range of the
/// original source it replaced.
///
/// `replaced` is where the splice actually landed, which is wider than the
/// edited span when an ATX heading becomes setext, so an origin writer scopes
/// itself by this range rather than restating it from the span.
#[derive(Debug)]
pub struct BlockRewrite {
    source: String,
    replaced: SourceSpan,
}

impl BlockRewrite {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn replaced(&self) -> SourceSpan {
        self.replaced
    }
}

/// Returns the full source after one validated parser-authorized block rewrite.
///
/// The supplied slide and span are treated only as capability tokens. The
/// source is parsed again, both tokens are checked against that fresh parse,
/// and the candidate is returned only when a second parse proves that deck
/// structure was preserved.
pub fn rewrite_block(
    source: &str,
    target: &ParsedSlide,
    span: EditableSpan,
    new_markdown: &str,
    highlighter: &Highlighter,
) -> Result<BlockRewrite> {
    let (source, had_bom) = strip_bom(source);
    let before = parse_source(source, highlighter)?;
    let source_span = span.source_span();
    let error_line = Some(safe_line_for_offset(source, source_span.start));
    let supplied_spans = target.editable_spans();
    let Some(fresh_target) = before.parsed_slides().get(target.index) else {
        return Err(refusal(
            error_line,
            "inline edit target does not match the current deck source",
        ));
    };
    let fresh_spans = fresh_target.editable_spans();

    if fresh_target.key != target.key
        || fresh_target.source_index != target.source_index
        || fresh_target.source_span != target.source_span
        || fresh_spans != supplied_spans
    {
        return Err(refusal(
            error_line,
            "inline edit target does not match the current deck source",
        ));
    }

    if source_span.start >= source_span.end || source_span.end > source.len() {
        return Err(refusal(
            error_line,
            "inline edit span is outside the deck source",
        ));
    }
    if !source.is_char_boundary(source_span.start) || !source.is_char_boundary(source_span.end) {
        return Err(refusal(
            error_line,
            "inline edit span is not on UTF-8 boundaries in the deck source",
        ));
    }
    let Some(edited_span_index) = fresh_spans.iter().position(|candidate| *candidate == span)
    else {
        return Err(refusal(
            error_line,
            "inline edit span is not authorized for the target slide",
        ));
    };
    let normalized_replacement = normalized_note_text(new_markdown);
    let replacement = normalized_replacement.trim_matches(|ch| matches!(ch, '\r' | '\n'));
    if source[source_span.start..source_span.end] == *replacement {
        return Ok(BlockRewrite {
            source: restore_bom(source.to_owned(), had_bom),
            replaced: source_span,
        });
    }

    // An ATX heading is one line, so a newline rewrites the whole heading to
    // setext form; the reparse below still proves nothing else changed.
    let (splice, spliced) = match span.atx() {
        Some(heading) if replacement.contains('\n') => {
            let underline = match heading.level() {
                1 => "====",
                // Four, not three: a lone `---` line is the slide separator.
                2 => "----",
                level => {
                    return Err(refusal(
                        error_line,
                        format!(
                            "an H{level} heading cannot span lines; only H1 and H2 have a multi-line (setext) form"
                        ),
                    ))
                }
            };
            (heading.line(), format!("{replacement}\n{underline}"))
        }
        _ => (source_span, replacement.to_owned()),
    };
    let mut candidate = source.to_owned();
    candidate.replace_range(splice.start..splice.end, &spliced);
    let after = parse_source(&candidate, highlighter)?;
    preserves_deck_for_block_edit(
        source,
        &before,
        &candidate,
        &after,
        target.index,
        edited_span_index,
        replacement,
    )
    .map_err(|mut error| {
        error.line = error_line;
        error
    })?;

    Ok(BlockRewrite {
        source: restore_bom(candidate, had_bom),
        replaced: splice,
    })
}

fn parse_source(source: &str, highlighter: &Highlighter) -> Result<Deck<Parsed>> {
    let frontmatter = parse_frontmatter(source)?;
    parse_markdown(source, frontmatter, highlighter)
}

fn safe_line_for_offset(source: &str, requested: usize) -> usize {
    let mut offset = requested.min(source.len());
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    line_for_offset(source, offset)
}

fn refusal(line: Option<usize>, message: impl Into<String>) -> BuildError {
    BuildError::new(ErrorKind::Parse, line, message, STRUCTURAL_EDIT_HELP)
}

fn preserves_deck_for_block_edit(
    before_source: &str,
    before: &Deck<Parsed>,
    after_source: &str,
    after: &Deck<Parsed>,
    target_index: usize,
    edited_span_index: usize,
    replacement: &str,
) -> Result<()> {
    let before_slides = before.parsed_slides();
    let after_slides = after.parsed_slides();
    if before_slides.len() != after_slides.len() {
        return Err(refusal(
            None,
            "inline edit would change the deck's slide count",
        ));
    }
    if before.settings().sections() != after.settings().sections() {
        return Err(refusal(
            None,
            "inline edit would change the deck's sections",
        ));
    }
    for (index, (before_slide, after_slide)) in before_slides.iter().zip(after_slides).enumerate() {
        if index == target_index {
            compare_target_slide_for_block_edit(
                before_source,
                before_slide,
                after_source,
                after_slide,
                edited_span_index,
                replacement,
            )?;
        } else {
            compare_non_target_slide_for_block_edit(before_slide, after_slide)?;
        }
    }

    Ok(())
}

fn compare_non_target_slide_for_block_edit(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> Result<()> {
    compare_all(before, after).map_err(|difference| {
        refusal(
            None,
            format!(
                "inline edit would change another slide's {}",
                difference.noun_phrase()
            ),
        )
    })
}

fn compare_target_slide_for_block_edit(
    before_source: &str,
    before: &ParsedSlide,
    after_source: &str,
    after: &ParsedSlide,
    edited_span_index: usize,
    replacement: &str,
) -> Result<()> {
    if let Err(difference) = compare_except_key(before, after) {
        return Err(refusal(
            None,
            format!(
                "inline edit would change the edited slide's {}",
                difference.noun_phrase()
            ),
        ));
    }
    let fragment_shape = compare_fragment_shape(
        &before.fragments,
        &after.fragments,
        HtmlBlockComparison::Include,
    );
    if fragment_shape == FragmentShapeComparison::FragmentKindsDiffer {
        return Err(refusal(
            None,
            "inline edit would change the edited slide's block structure",
        ));
    }

    let before_spans = before.editable_spans();
    let after_spans = after.editable_spans();
    if before_spans.len() != after_spans.len() {
        return Err(refusal(
            None,
            "inline edit would change the edited slide's editable block count",
        ));
    }
    let before_kinds = editable_block_kinds(&before_spans);
    let after_kinds = editable_block_kinds(&after_spans);
    if before_kinds != after_kinds {
        return Err(refusal(
            None,
            "inline edit would change the edited slide's editable block kinds",
        ));
    }
    if fragment_shape == FragmentShapeComparison::BlockSkeletonsDiffer {
        return Err(refusal(
            None,
            "inline edit would change the edited slide's block structure",
        ));
    }
    compare_editable_block_text(
        before_source,
        &before_spans,
        after_source,
        &after_spans,
        edited_span_index,
        replacement,
    )?;
    if before.step_count != after.step_count {
        return Err(refusal(
            None,
            "inline edit would change the edited slide's reveal step count",
        ));
    }
    if !target_key_change_allowed(before, after) {
        return Err(refusal(
            None,
            "inline edit would change an explicit key on the edited slide",
        ));
    }

    Ok(())
}

fn editable_block_kinds(spans: &[EditableSpan]) -> Vec<EditableBlockKind> {
    spans.iter().copied().map(EditableSpan::kind).collect()
}

fn compare_editable_block_text(
    before_source: &str,
    before_spans: &[EditableSpan],
    after_source: &str,
    after_spans: &[EditableSpan],
    edited_span_index: usize,
    replacement: &str,
) -> Result<()> {
    for (index, (before_span, after_span)) in before_spans.iter().zip(after_spans).enumerate() {
        if index == edited_span_index {
            continue;
        }
        let before_span = before_span.source_span();
        let before_text = &before_source[before_span.start..before_span.end];
        let after_span = after_span.source_span();
        let after_text = &after_source[after_span.start..after_span.end];
        if before_text != after_text {
            return Err(refusal(
                None,
                "inline edit would change another block on the edited slide",
            ));
        }
    }

    let after_span = after_spans[edited_span_index].source_span();
    let after_text = &after_source[after_span.start..after_span.end];
    if trim_ascii_horizontal(after_text) != trim_ascii_horizontal(replacement) {
        return Err(refusal(
            None,
            "inline edit did not stay inside the edited block",
        ));
    }

    Ok(())
}

fn trim_ascii_horizontal(value: &str) -> &str {
    value.trim_matches(|character| matches!(character, ' ' | '\t'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::{BuildError, ErrorKind},
        highlight::Highlighter,
        parser::{parse_frontmatter, parse_markdown},
        phase::{Deck, Parsed},
    };

    const STRUCTURAL_EDIT_HELP: &str =
        "make structural changes in the Markdown editor, then reload the preview and retry";

    fn parse(source: &str, highlighter: &Highlighter) -> Deck<Parsed> {
        let frontmatter = parse_frontmatter(source).unwrap();
        parse_markdown(source, frontmatter, highlighter).unwrap()
    }

    fn rewrite(
        source: &str,
        slide_index: usize,
        span_index: usize,
        replacement: &str,
    ) -> crate::Result<String> {
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let slide = &deck.parsed_slides()[slide_index];
        let span = slide.editable_spans()[span_index];
        rewrite_block(source, slide, span, replacement, &highlighter)
            .map(|rewrite| rewrite.source().to_owned())
    }

    fn assert_rewrite_refusal(
        source: &str,
        slide_index: usize,
        span_index: usize,
        replacement: &str,
        expected_message: &str,
    ) -> BuildError {
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let target = &deck.parsed_slides()[slide_index];
        let span = target.editable_spans()[span_index];
        let expected_line = line_for_offset(source, span.source_span().start);
        let error = rewrite_block(source, target, span, replacement, &highlighter).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(error.message, expected_message);
        assert_eq!(error.help, STRUCTURAL_EDIT_HELP);
        assert_eq!(error.line, Some(expected_line));
        error
    }

    fn assert_comparison_refusal(
        before_source: &str,
        after_source: &str,
        target_index: usize,
        expected_message: &str,
    ) {
        let highlighter = Highlighter::defaults();
        let before = parse(before_source, &highlighter);
        let after = parse(after_source, &highlighter);
        let after_target = &after.parsed_slides()[target_index];
        let edited_span_index = 0;
        let edited_after_span = after_target.editable_spans()[edited_span_index].source_span();
        let replacement = &after_source[edited_after_span.start..edited_after_span.end];
        let error = preserves_deck_for_block_edit(
            before_source,
            &before,
            after_source,
            &after,
            target_index,
            edited_span_index,
            replacement,
        )
        .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(error.message, expected_message);
        assert_eq!(error.help, STRUCTURAL_EDIT_HELP);
    }

    fn rewrite_heading(source: &str, replacement: &str) -> crate::Result<String> {
        rewrite(source, 0, 0, replacement)
    }

    #[test]
    fn rewrites_emphasis_across_inline_boundaries() {
        let source = "# Title\n\nPeitho is a *fast* tool.\n";
        let once = rewrite(source, 0, 1, "Peitho is a **very fast** tool.").unwrap();
        assert_eq!(once, "# Title\n\nPeitho is a **very fast** tool.\n");

        let twice = rewrite(&once, 0, 1, "Peitho is a very fast tool.").unwrap();
        assert_eq!(twice, "# Title\n\nPeitho is a very fast tool.\n");
    }

    #[test]
    fn rewrites_links_and_inline_code_in_a_paragraph() {
        let source = "# Title\n\nRead [the docs](https://example.com) with `old()`.\n";
        let replacement = "Read [the guide](https://example.com/guide) with `new()`.";

        assert_eq!(
            rewrite(source, 0, 1, replacement).unwrap(),
            format!("# Title\n\n{replacement}\n")
        );
    }

    #[test]
    fn normalizes_only_edge_newlines() {
        let source = "# Title\n\nold text\n";

        assert_eq!(
            rewrite(source, 0, 1, "\r\n  new text  \n\r").unwrap(),
            "# Title\n\n  new text  \n"
        );
    }

    #[test]
    fn normalizes_crlf_and_cr_inside_replacement_to_lf() {
        let source = "---\nbreaks: true\n---\n# T\n\nold\n";
        let expected = "---\nbreaks: true\n---\n# T\n\none\ntwo\n";

        for replacement in ["one\r\ntwo", "one\rtwo"] {
            assert_eq!(rewrite(source, 0, 1, replacement).unwrap(), expected);
        }
    }

    #[test]
    fn treats_crlf_replacement_as_no_op_against_lf_source() {
        let source = "---\nbreaks: true\n---\n# T\n\none\ntwo\n";

        assert_eq!(rewrite(source, 0, 1, "one\r\ntwo").unwrap(), source);
    }

    #[test]
    fn accepts_harmless_leading_and_trailing_space_around_replacement() {
        let source = "# Title\n\nold text\n";

        assert_eq!(
            rewrite(source, 0, 1, " new text\t").unwrap(),
            "# Title\n\n new text\t\n"
        );
    }

    #[test]
    fn returns_byte_identical_source_for_a_no_op() {
        let source = "# Title\r\n\r\nSame *bytes*.\r\n";

        assert_eq!(rewrite(source, 0, 1, "Same *bytes*.").unwrap(), source);
    }

    #[test]
    fn rewrites_bom_source_without_skewing_the_span() {
        let source = "\u{feff}## Title\n\nbody\n";

        assert_eq!(
            rewrite(source, 0, 0, "## XX").unwrap(),
            "\u{feff}## ## XX\n\nbody\n"
        );
    }

    #[test]
    fn returns_byte_identical_bom_source_for_a_no_op() {
        let source = "\u{feff}## Title\n\nbody\n";

        assert_eq!(rewrite(source, 0, 0, "Title").unwrap(), source);
    }

    #[test]
    fn rewrites_cjk_bytes() {
        let source = "# 日本語\n\n日本語の **文章** です\n";

        assert_eq!(
            rewrite(source, 0, 1, "日本語の **更新文** です").unwrap(),
            "# 日本語\n\n日本語の **更新文** です\n"
        );
    }

    #[test]
    fn rewrites_a_paragraph_that_keeps_its_footnote_reference() {
        let source = "# Title\n\nRead this [^note] today.\n\n[^note]: Definition\n";

        assert_eq!(
            rewrite(source, 0, 1, "Read that [^note] tomorrow.").unwrap(),
            "# Title\n\nRead that [^note] tomorrow.\n\n[^note]: Definition\n"
        );
    }

    #[test]
    fn rewrites_a_footnote_definition_body() {
        let source = "# Title\n\nRead [^note].\n\n[^note]: Old [link](https://a.example).\n";

        assert_eq!(
            rewrite(source, 0, 2, "New [link](https://b.example).").unwrap(),
            "# Title\n\nRead [^note].\n\n[^note]: New [link](https://b.example).\n"
        );
    }

    #[test]
    fn rejects_a_footnote_body_split_into_a_second_paragraph() {
        assert_rewrite_refusal(
            "# Title\n\nRead [^note].\n\n[^note]: Definition\n",
            0,
            2,
            "one\n\ntwo",
            "inline edit would change the edited slide's block structure",
        );
    }

    #[test]
    fn accepts_internal_newlines_without_comparing_diagnostic_lines() {
        let source = concat!(
            "---\n",
            "breaks: true\n",
            "---\n",
            "# First\n\n",
            "first line\n",
            "second line\n\n",
            "---\n",
            "<!-- {\"key\":\"later\",\"layout\":\"cover\"} -->\n",
            "# Later\n\n",
            "<!-- later note -->\n",
        );
        let replacement = "first line\nchanged line\nthird line";
        let rewritten = rewrite(source, 0, 1, replacement).unwrap();
        let highlighter = Highlighter::defaults();
        let after = parse(&rewritten, &highlighter);

        assert!(rewritten.contains("first line\nchanged line\nthird line"));
        assert_eq!(after.parsed_slides().len(), 2);
        assert_eq!(after.parsed_slides()[0].fragments.len(), 2);
        assert_eq!(
            after.parsed_slides()[1]
                .layout_request
                .as_ref()
                .map(|request| request.name.as_str()),
            Some("cover")
        );
        assert_eq!(
            after.parsed_slides()[1].notes.as_deref(),
            Some("later note")
        );
    }

    #[test]
    fn allows_a_derived_heading_key_to_change() {
        let derived = rewrite_heading("# Before\n", "After").unwrap();
        let highlighter = Highlighter::defaults();

        assert_eq!(
            parse(&derived, &highlighter).parsed_slides()[0]
                .key
                .as_str(),
            "after"
        );
    }

    #[test]
    fn preserves_an_explicit_heading_key() {
        let explicit =
            rewrite_heading("<!-- {\"key\":\"fixed\"} -->\n# Before\n", "After").unwrap();
        let highlighter = Highlighter::defaults();

        assert_eq!(
            parse(&explicit, &highlighter).parsed_slides()[0]
                .key
                .as_str(),
            "fixed"
        );
    }

    #[test]
    fn rejects_original_parse_failure() {
        let fixture = "# Valid\n\nbody\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(fixture, &highlighter);
        let target = &deck.parsed_slides()[0];
        let span = target.editable_spans()[1];
        let error = rewrite_block(" \n\n", target, span, "new", &highlighter).unwrap_err();

        assert_eq!(error.message, "deck has no slides");
    }

    #[test]
    fn rejects_candidate_parse_failure_with_the_parser_diagnostic() {
        let source = "# One\n\nbody\n\n---\n\n# Two\n";
        let error = rewrite(source, 0, 0, "Two").unwrap_err();

        assert_eq!(error.message, "duplicate slide key 'two'");
    }

    #[test]
    fn rejects_slide_count_change() {
        assert_comparison_refusal(
            "# Target\n\nbody\n",
            "# Target\n\nbody\n\n---\n\n# Added\n",
            0,
            "inline edit would change the deck's slide count",
        );
    }

    #[test]
    fn rejects_section_change() {
        let before = concat!(
            "---\n",
            "time: 2m\n",
            "---\n",
            "<!-- {\"section\":\"First\",\"time\":\"1m\"} -->\n",
            "# Target\n\n",
            "---\n",
            "<!-- {\"section\":\"Second\",\"time\":\"1m\"} -->\n",
            "# Other\n",
        );
        let after = before.replace("\"First\"", "\"Changed\"");

        assert_comparison_refusal(
            before,
            &after,
            0,
            "inline edit would change the deck's sections",
        );
    }

    #[test]
    fn rejects_non_target_key_change() {
        assert_comparison_refusal(
            "# Target\n\n---\n\n# Before\n",
            "# Target\n\n---\n\n# After\n",
            0,
            "inline edit would change another slide's key",
        );
    }

    #[test]
    fn rejects_non_target_key_source_change() {
        assert_comparison_refusal(
            "# Target\n\n---\n\n# Stable\n",
            "# Target\n\n---\n\n<!-- {\"key\":\"stable\"} -->\n# Stable\n",
            0,
            "inline edit would change another slide's key source",
        );
    }

    #[test]
    fn rejects_non_target_layout_request_change() {
        assert_comparison_refusal(
            concat!(
                "<!-- {\"key\":\"target\"} -->\n# Target\n\n---\n\n",
                "<!-- {\"key\":\"other\"} -->\n# Other\n",
            ),
            concat!(
                "<!-- {\"key\":\"target\"} -->\n# Target\n\n---\n\n",
                "<!-- {\"key\":\"other\",\"layout\":\"cover\"} -->\n# Other\n",
            ),
            0,
            "inline edit would change another slide's layout request",
        );
    }

    #[test]
    fn rejects_non_target_skip_change() {
        assert_comparison_refusal(
            concat!(
                "<!-- {\"key\":\"target\"} -->\n# Target\n\n---\n\n",
                "<!-- {\"key\":\"other\"} -->\n# Other\n",
            ),
            concat!(
                "<!-- {\"key\":\"target\"} -->\n# Target\n\n---\n\n",
                "<!-- {\"key\":\"other\",\"skip\":true} -->\n# Other\n",
            ),
            0,
            "inline edit would change another slide's skip flag",
        );
    }

    #[test]
    fn rejects_non_target_page_number_flag_change() {
        assert_comparison_refusal(
            concat!(
                "---\npage_numbers: current\n---\n",
                "<!-- {\"key\":\"target\"} -->\n# Target\n\n---\n\n",
                "<!-- {\"key\":\"other\"} -->\n# Other\n",
            ),
            concat!(
                "---\npage_numbers: current\n---\n",
                "<!-- {\"key\":\"target\"} -->\n# Target\n\n---\n\n",
                "<!-- {\"key\":\"other\",\"page_number\":false} -->\n# Other\n",
            ),
            0,
            "inline edit would change another slide's page-number flag",
        );
    }

    #[test]
    fn rejects_non_target_notes_change() {
        assert_comparison_refusal(
            "# Target\n\n---\n\n# Other\n\n<!-- before note -->\n",
            "# Target\n\n---\n\n# Other\n\n<!-- after note -->\n",
            0,
            "inline edit would change another slide's notes",
        );
    }

    #[test]
    fn rejects_target_notes_change() {
        assert_comparison_refusal(
            "# Target\n\n<!-- before note -->\n",
            "# Target\n\n<!-- after note -->\n",
            0,
            "inline edit would change the edited slide's notes",
        );
    }

    #[test]
    fn rejects_target_layout_request_change() {
        assert_comparison_refusal(
            "<!-- {\"key\":\"target\"} -->\n# Target\n",
            "<!-- {\"key\":\"target\",\"layout\":\"cover\"} -->\n# Target\n",
            0,
            "inline edit would change the edited slide's layout request",
        );
    }

    #[test]
    fn rejects_target_skip_change() {
        assert_comparison_refusal(
            "<!-- {\"key\":\"target\"} -->\n# Target\n",
            "<!-- {\"key\":\"target\",\"skip\":true} -->\n# Target\n",
            0,
            "inline edit would change the edited slide's skip flag",
        );
    }

    #[test]
    fn rejects_target_page_number_flag_change() {
        assert_comparison_refusal(
            concat!(
                "---\npage_numbers: current\n---\n",
                "<!-- {\"key\":\"target\"} -->\n# Target\n",
            ),
            concat!(
                "---\npage_numbers: current\n---\n",
                "<!-- {\"key\":\"target\",\"page_number\":false} -->\n# Target\n",
            ),
            0,
            "inline edit would change the edited slide's page-number flag",
        );
    }

    #[test]
    fn rejects_target_fragment_kind_change_inside_slot_group() {
        assert_comparison_refusal(
            concat!(
                "<!-- {\"key\":\"target\"} -->\n",
                "# Target\n\n",
                "::: {slot=body}\n\n",
                "paragraph\n\n",
                ":::\n",
            ),
            concat!(
                "<!-- {\"key\":\"target\"} -->\n",
                "# Target\n\n",
                "::: {slot=body}\n\n",
                "- item\n\n",
                ":::\n",
            ),
            0,
            "inline edit would change the edited slide's block structure",
        );
    }

    #[test]
    fn rejects_target_heading_level_change() {
        assert_comparison_refusal(
            "<!-- {\"key\":\"target\"} -->\n# Target\n",
            "<!-- {\"key\":\"target\"} -->\n## Target\n",
            0,
            "inline edit would change the edited slide's block structure",
        );
    }

    #[test]
    fn rejects_target_editable_span_count_change() {
        assert_comparison_refusal(
            "<!-- {\"key\":\"target\"} -->\n# Target\n\n- one\n",
            "<!-- {\"key\":\"target\"} -->\n# Target\n\n- one\n- two\n",
            0,
            "inline edit would change the edited slide's editable block count",
        );
    }

    #[test]
    fn rejects_target_reveal_step_count_change() {
        assert_comparison_refusal(
            "<!-- {\"key\":\"target\"} -->\n# Target\n\nBody\n",
            concat!(
                "<!-- {\"key\":\"target\"} -->\n",
                "# Target\n\n",
                "::: {reveal}\n\n",
                "Body\n\n",
                ":::\n",
            ),
            0,
            "inline edit would change the edited slide's reveal step count",
        );
    }

    #[test]
    fn rejects_target_key_change_when_either_key_source_is_explicit() {
        let cases = [
            (
                "<!-- {\"key\":\"before\"} -->\n# Same\n",
                "<!-- {\"key\":\"after\"} -->\n# Same\n",
            ),
            ("<!-- {\"key\":\"before\"} -->\n# Before\n", "# After\n"),
            ("# Before\n", "<!-- {\"key\":\"after\"} -->\n# Before\n"),
        ];

        for (before, after) in cases {
            assert_comparison_refusal(
                before,
                after,
                0,
                "inline edit would change an explicit key on the edited slide",
            );
        }
    }

    #[test]
    fn rejects_empty_block() {
        assert_rewrite_refusal(
            "# Title\n\nBody\n",
            0,
            1,
            "",
            "inline edit would change the edited slide's block structure",
        );
    }

    #[test]
    fn rejects_list_marker_prefix() {
        assert_rewrite_refusal(
            "# Title\n\nBody\n",
            0,
            1,
            "- Body",
            "inline edit would change the edited slide's block structure",
        );
    }

    #[test]
    fn rejects_heading_marker_prefix() {
        assert_rewrite_refusal(
            "# Title\n\nBody\n",
            0,
            1,
            "# Body",
            "inline edit would change the edited slide's block structure",
        );
    }

    #[test]
    fn rejects_blank_line_block_split() {
        assert_rewrite_refusal(
            "# Title\n\nBody\n",
            0,
            1,
            "one\n\ntwo",
            "inline edit would change the edited slide's block structure",
        );
    }

    #[test]
    fn rejects_slide_separator() {
        assert_rewrite_refusal(
            "# Title\n\nBody\n",
            0,
            1,
            "Body\n\n---\n\nNext",
            "inline edit would change the deck's slide count",
        );
    }

    #[test]
    fn rejects_note_comment() {
        assert_rewrite_refusal(
            "# Title\n\nBody\n",
            0,
            1,
            "<!-- note -->",
            "inline edit would change the edited slide's notes",
        );
    }

    #[test]
    fn rejects_div_fence() {
        let error = rewrite("# Title\n\nBody\n", 0, 1, ":::").unwrap_err();

        assert_eq!(error.message, "closing `:::` has no matching opening fence");
    }

    #[test]
    fn rejects_removing_the_last_footnote_reference_with_the_parser_diagnostic() {
        let source = "# Title\n\nRead [^note].\n\n[^note]: Definition\n";
        let error = rewrite(source, 0, 1, "Read without it.").unwrap_err();

        assert_eq!(error.message, "unused footnote definition `[^note]`");
    }

    #[test]
    fn rejects_tight_to_loose_list_flip() {
        let source = "# Title\n\n- a\n- b\n";
        let highlighter = Highlighter::defaults();
        let before = parse(source, &highlighter);
        let target = &before.parsed_slides()[0];
        let span = target.editable_spans()[1];
        let mut candidate = source.to_owned();
        candidate.replace_range(span.source_span().start..span.source_span().end, "a\n\n  ");
        let after = parse(&candidate, &highlighter);
        let before_spans = target.editable_spans();
        let after_spans = after.parsed_slides()[0].editable_spans();

        assert_eq!(before_spans.len(), after_spans.len());
        assert_eq!(
            compare_fragment_shape(
                &target.fragments,
                &after.parsed_slides()[0].fragments,
                HtmlBlockComparison::Include,
            ),
            FragmentShapeComparison::BlockSkeletonsDiffer
        );
        assert_ne!(before_spans[1].kind(), after_spans[1].kind());

        let error = rewrite_block(source, target, span, "a\n\n  ", &highlighter).unwrap_err();
        assert_eq!(
            error.message,
            "inline edit would change the edited slide's editable block kinds"
        );
    }

    #[test]
    fn rejects_leading_whitespace_that_unnests_a_child_list() {
        let cases = [
            ("# T\n\n- one\n- two\n  - nested\n- three\n", "   two"),
            ("# T\n\n- one\n- two\n  - nested\n- three\n", "  two"),
            ("# T\n\n- one\n- two\n  - nested\n- three\n", "\ttwo"),
            (
                "# T\n\n- one\n- two\n  cont\n  - nested\n- three\n",
                "   two\n  cont",
            ),
            (
                "# T\n\n> - one\n> - two\n>   - nested\n> - three\n",
                "   two",
            ),
        ];

        for (source, replacement) in cases {
            assert_rewrite_refusal(
                source,
                0,
                2,
                replacement,
                "inline edit would change the edited slide's block structure",
            );
        }
    }

    #[test]
    fn rejects_table_cell_pipe_that_displaces_a_sibling_cell() {
        let source = "| Name | Value |\n| --- | --- |\n| x | y |\n";
        let highlighter = Highlighter::defaults();
        let before = parse(source, &highlighter);
        let target = &before.parsed_slides()[0];
        let spans = target.editable_spans();

        assert_eq!(spans.len(), 4);
        let error = rewrite_block(source, target, spans[2], "x | z", &highlighter).unwrap_err();
        assert_eq!(
            error.message,
            "inline edit would change another block on the edited slide"
        );
    }

    #[test]
    fn rejects_when_replacement_did_not_stay_inside_edited_block() {
        let before_source = "# Title\n\nBefore\n";
        let after_source = "# Title\n\nAfter\n";
        let highlighter = Highlighter::defaults();
        let before = parse(before_source, &highlighter);
        let after = parse(after_source, &highlighter);
        let error = preserves_deck_for_block_edit(
            before_source,
            &before,
            after_source,
            &after,
            0,
            1,
            "Expected",
        )
        .unwrap_err();

        assert_eq!(
            error.message,
            "inline edit did not stay inside the edited block"
        );
    }

    #[test]
    fn rejects_a_span_not_owned_by_the_target_slide() {
        let source = "# First\n\nbody\n\n---\n\n# Second\n\nbody\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let target = &deck.parsed_slides()[0];
        let other_span = deck.parsed_slides()[1].editable_spans()[0];
        let error = rewrite_block(source, target, other_span, "Changed", &highlighter).unwrap_err();

        assert_eq!(
            error.message,
            "inline edit span is not authorized for the target slide"
        );
        assert_eq!(error.help, STRUCTURAL_EDIT_HELP);
    }

    #[test]
    fn rejects_an_invalid_utf8_boundary_or_range() {
        let source = "# あ\n";
        let token_source = "# T\n\nabc\n";
        let highlighter = Highlighter::defaults();
        let deck = parse(source, &highlighter);
        let target = &deck.parsed_slides()[0];
        let token_deck = parse(token_source, &highlighter);
        let invalid_boundary = token_deck.parsed_slides()[0].editable_spans()[0];
        let error =
            rewrite_block(source, target, invalid_boundary, "Changed", &highlighter).unwrap_err();
        assert_eq!(
            error.message,
            "inline edit span is not on UTF-8 boundaries in the deck source"
        );

        let long_token_source = "# Title\n\nlong body that exceeds the target source\n";
        let long_token_deck = parse(long_token_source, &highlighter);
        let invalid_range = long_token_deck.parsed_slides()[0].editable_spans()[1];
        let error =
            rewrite_block(source, target, invalid_range, "Changed", &highlighter).unwrap_err();
        assert_eq!(error.message, "inline edit span is outside the deck source");
    }

    #[test]
    fn rejects_a_stale_target_slide() {
        let old_source = "# Before\n\nBody\n";
        let current_source = "# After\n\nBody\n";
        let highlighter = Highlighter::defaults();
        let old = parse(old_source, &highlighter);
        let target = &old.parsed_slides()[0];
        let span = target.editable_spans()[1];
        let error =
            rewrite_block(current_source, target, span, "Changed", &highlighter).unwrap_err();

        assert_eq!(
            error.message,
            "inline edit target does not match the current deck source"
        );
        assert_eq!(error.help, STRUCTURAL_EDIT_HELP);
    }

    #[test]
    fn newline_in_atx_h1_becomes_a_setext_heading() {
        assert_eq!(
            rewrite_heading("# Title\n\nBody\n", "Title\nline two").unwrap(),
            "Title\nline two\n====\n\nBody\n"
        );
    }

    #[test]
    fn newline_in_atx_h2_becomes_a_setext_heading_that_is_not_a_slide_separator() {
        let source = "# First\n\n---\n\n## Title\n\nBody\n";
        let rewritten = rewrite(source, 1, 0, "Title\nline two").unwrap();

        assert_eq!(
            rewritten,
            "# First\n\n---\n\nTitle\nline two\n----\n\nBody\n"
        );
        assert_eq!(
            parse(&rewritten, &Highlighter::defaults())
                .parsed_slides()
                .len(),
            2
        );
    }

    #[test]
    fn newline_in_atx_heading_drops_the_closing_sequence() {
        assert_eq!(
            rewrite_heading("# Title #\n", "Title\nline two").unwrap(),
            "Title\nline two\n====\n"
        );
    }

    #[test]
    fn newline_in_atx_heading_keeps_a_crlf_line_ending_outside_the_heading() {
        assert_eq!(
            rewrite_heading("# Title\r\n\r\nBody\r\n", "Title\nline two").unwrap(),
            "Title\nline two\n====\r\n\r\nBody\r\n"
        );
    }

    #[test]
    fn rejects_newline_in_atx_h3_to_h6() {
        for level in 3..=6 {
            let source = format!("{} Title\n", "#".repeat(level));
            assert_rewrite_refusal(
                &source,
                0,
                0,
                "Title\nline two",
                &format!(
                    "an H{level} heading cannot span lines; only H1 and H2 have a multi-line (setext) form"
                ),
            );
        }
    }

    #[test]
    fn atx_heading_without_newline_keeps_atx_form() {
        assert_eq!(rewrite_heading("## Title\n", "New").unwrap(), "## New\n");
    }

    #[test]
    fn setext_heading_stays_setext_with_or_without_newlines() {
        assert_eq!(
            rewrite_heading("Title\n=====\n", "Title\nline two").unwrap(),
            "Title\nline two\n=====\n"
        );
        assert_eq!(
            rewrite_heading("Title\nline two\n=====\n", "Title").unwrap(),
            "Title\n=====\n"
        );
    }

    #[test]
    fn rejects_newline_in_a_heading_inside_a_container() {
        for source in ["> # Title\n", "- # Title\n"] {
            assert_rewrite_refusal(
                source,
                0,
                0,
                "Title\nline two",
                "inline edit would change the edited slide's editable block kinds",
            );
        }
    }

    #[test]
    fn newline_in_an_indented_atx_heading_keeps_the_indentation() {
        assert_eq!(
            rewrite_heading("   # Title\n\nBody\n", "Title\nline two").unwrap(),
            "   Title\nline two\n====\n\nBody\n"
        );
    }

    #[test]
    fn converted_heading_keeps_an_explicit_key_and_moves_a_derived_one() {
        let highlighter = Highlighter::defaults();
        let explicit =
            rewrite_heading("<!-- {\"key\":\"fixed\"} -->\n# Before\n", "Before\nAfter").unwrap();
        assert_eq!(
            parse(&explicit, &highlighter).parsed_slides()[0]
                .key
                .as_str(),
            "fixed"
        );
        let derived = rewrite_heading("# Before\n", "Before\nAfter").unwrap();
        assert_ne!(
            parse(&derived, &highlighter).parsed_slides()[0]
                .key
                .as_str(),
            "before"
        );
    }

    #[test]
    fn newline_in_atx_heading_keeps_a_leading_bom() {
        assert_eq!(
            rewrite_heading("\u{feff}# Title\n\nBody\n", "Title\nline two").unwrap(),
            "\u{feff}Title\nline two\n====\n\nBody\n"
        );
    }
}
