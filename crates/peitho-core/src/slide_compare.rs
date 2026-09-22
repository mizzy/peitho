//! Shared comparisons for parsed slides across source-edit paths.

use crate::{
    domain::{FragmentKind, SourceFragment},
    parser::{is_html_only_container, same_block_skeleton},
    phase::ParsedSlide,
};

#[derive(Clone, Copy)]
pub(crate) enum HtmlBlockComparison {
    Include,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FragmentShapeComparison {
    Equal,
    FragmentKindsDiffer,
    BlockSkeletonsDiffer,
}

impl FragmentShapeComparison {
    pub(crate) fn is_equal(self) -> bool {
        self == Self::Equal
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SlideDifference {
    Key,
    KeySourceKind,
    LayoutRequest,
    Skip,
    PageNumber,
    Notes,
}

impl SlideDifference {
    pub(crate) fn noun_phrase(&self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::KeySourceKind => "key source",
            Self::LayoutRequest => "layout request",
            Self::Skip => "skip flag",
            Self::PageNumber => "page-number flag",
            Self::Notes => "notes",
        }
    }
}

fn compare_layout_and_flags(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> std::result::Result<(), SlideDifference> {
    if before.layout_request_name() != after.layout_request_name() {
        return Err(SlideDifference::LayoutRequest);
    }
    if before.skip != after.skip {
        return Err(SlideDifference::Skip);
    }
    if before.page_number_hidden != after.page_number_hidden {
        return Err(SlideDifference::PageNumber);
    }
    Ok(())
}

fn compare_notes(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> std::result::Result<(), SlideDifference> {
    if before.notes != after.notes {
        return Err(SlideDifference::Notes);
    }
    Ok(())
}

/// Compares notes first, then layout and per-slide flags, excluding the key.
pub(crate) fn compare_except_key(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> std::result::Result<(), SlideDifference> {
    compare_notes(before, after)?;
    compare_layout_and_flags(before, after)
}

/// Compares key identity, layout, and per-slide flags, excluding notes.
pub(crate) fn compare_except_notes(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> std::result::Result<(), SlideDifference> {
    if before.key != after.key {
        return Err(SlideDifference::Key);
    }
    if !before.key_source.same_kind_as(&after.key_source) {
        return Err(SlideDifference::KeySourceKind);
    }
    compare_layout_and_flags(before, after)
}

/// Compares all source-edit invariants shared by parsed slides.
pub(crate) fn compare_all(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> std::result::Result<(), SlideDifference> {
    compare_except_notes(before, after)?;
    compare_notes(before, after)
}

pub(crate) fn target_key_change_allowed(before: &ParsedSlide, after: &ParsedSlide) -> bool {
    before.key == after.key || (before.key_source.is_derived() && after.key_source.is_derived())
}

pub(crate) fn compare_fragment_shape(
    before: &[SourceFragment],
    after: &[SourceFragment],
    html_blocks: HtmlBlockComparison,
) -> FragmentShapeComparison {
    let mut block_skeletons_match = true;
    if !same_fragment_kinds(before, after, &mut block_skeletons_match, html_blocks) {
        FragmentShapeComparison::FragmentKindsDiffer
    } else if block_skeletons_match {
        FragmentShapeComparison::Equal
    } else {
        FragmentShapeComparison::BlockSkeletonsDiffer
    }
}

fn same_fragment_kinds(
    before: &[SourceFragment],
    after: &[SourceFragment],
    block_skeletons_match: &mut bool,
    html_blocks: HtmlBlockComparison,
) -> bool {
    let keep = |fragment: &&SourceFragment| {
        matches!(html_blocks, HtmlBlockComparison::Include)
            || !is_html_only_container(fragment.markdown())
    };
    let mut before = before.iter().filter(&keep);
    let mut after = after.iter().filter(keep);
    loop {
        match (before.next(), after.next()) {
            (Some(before), Some(after)) => {
                if !same_fragment_kind(before, after, block_skeletons_match, html_blocks) {
                    return false;
                }
            }
            (None, None) => return true,
            (Some(_), None) | (None, Some(_)) => return false,
        }
    }
}

fn same_fragment_kind(
    before: &SourceFragment,
    after: &SourceFragment,
    block_skeletons_match: &mut bool,
    html_blocks: HtmlBlockComparison,
) -> bool {
    let source_skeleton_matches = match (before.source_span(), after.source_span()) {
        (Some(_), Some(_)) => same_block_skeleton(
            before.markdown(),
            after.markdown(),
            matches!(html_blocks, HtmlBlockComparison::Ignore),
        ),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    };
    *block_skeletons_match &= source_skeleton_matches;

    if std::mem::discriminant(before.kind()) != std::mem::discriminant(after.kind()) {
        return false;
    }

    match before.kind() {
        FragmentKind::Heading { level } => {
            matches!(after.kind(), FragmentKind::Heading { level: other } if level == other)
        }
        FragmentKind::SlotGroup { name, children } => {
            let FragmentKind::SlotGroup {
                name: after_name,
                children: after_children,
            } = after.kind()
            else {
                return false;
            };
            name == after_name
                && same_fragment_kinds(children, after_children, block_skeletons_match, html_blocks)
        }
        FragmentKind::Paragraph
        | FragmentKind::Text
        | FragmentKind::Code
        | FragmentKind::Math { .. }
        | FragmentKind::EmbedCard { .. }
        | FragmentKind::GenericEmbedCard { .. }
        | FragmentKind::Footnotes { .. }
        | FragmentKind::Image { .. }
        | FragmentKind::List
        | FragmentKind::Blockquote
        | FragmentKind::Table => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compare_except_key, compare_fragment_shape, FragmentShapeComparison, HtmlBlockComparison,
        SlideDifference,
    };
    use crate::{
        highlight::Highlighter,
        parser::{parse_frontmatter, parse_markdown},
    };

    #[test]
    fn comparison_reports_notes_before_layout_request() {
        let highlighter = Highlighter::defaults();
        let before_source = "# T\n<!-- before -->\n";
        let after_source = "<!-- {\"layout\":\"cover\"} -->\n# T\n<!-- after -->\n";
        let before = parse_markdown(
            before_source,
            parse_frontmatter(before_source).unwrap(),
            &highlighter,
        )
        .unwrap();
        let after = parse_markdown(
            after_source,
            parse_frontmatter(after_source).unwrap(),
            &highlighter,
        )
        .unwrap();

        assert_eq!(
            compare_except_key(&before.parsed_slides()[0], &after.parsed_slides()[0]),
            Err(SlideDifference::Notes)
        );
    }

    #[test]
    fn fragment_shape_includes_block_skeleton_equality() {
        let highlighter = Highlighter::defaults();
        let tight_source = "# T\n\n- a\n- b\n";
        let loose_source = "# T\n\n- a\n\n- b\n";
        let tight = parse_markdown(
            tight_source,
            parse_frontmatter(tight_source).unwrap(),
            &highlighter,
        )
        .unwrap();
        let loose = parse_markdown(
            loose_source,
            parse_frontmatter(loose_source).unwrap(),
            &highlighter,
        )
        .unwrap();
        assert_eq!(
            compare_fragment_shape(
                &tight.parsed_slides()[0].fragments,
                &loose.parsed_slides()[0].fragments,
                HtmlBlockComparison::Include,
            ),
            FragmentShapeComparison::BlockSkeletonsDiffer
        );
    }
}
