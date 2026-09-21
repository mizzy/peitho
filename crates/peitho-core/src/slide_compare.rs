//! Shared comparisons for parsed slides across source-edit paths.

use crate::phase::ParsedSlide;

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

#[cfg(test)]
mod tests {
    use super::{compare_except_key, SlideDifference};
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
}
