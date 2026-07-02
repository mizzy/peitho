use std::collections::BTreeMap;

use crate::domain::{RenderedSlide, SlideKey, SlotContract, SlotName, SourceFragment};

#[derive(Debug, Clone)]
pub struct Deck<P> {
    phase: P,
}

#[derive(Debug, Clone)]
pub struct Parsed {
    slides: Vec<ParsedSlide>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    Explicit { line: usize },
    Derived { line: Option<usize> },
}

#[derive(Debug, Clone)]
pub struct ParsedSlide {
    pub index: usize,
    pub key: SlideKey,
    pub key_source: KeySource,
    pub fragments: Vec<SourceFragment>,
}

#[derive(Debug, Clone)]
pub struct Mapped {
    slides: Vec<MappedSlide>,
}

#[derive(Debug, Clone)]
pub struct MappedSlide {
    pub(crate) index: usize,
    pub(crate) key: SlideKey,
    pub(crate) slots: BTreeMap<SlotName, MappedSlot>,
    pub(crate) unassigned: Vec<UnassignedFragment>,
}

#[derive(Debug, Clone)]
pub struct MappedSlot {
    contract: SlotContract,
    fragments: Vec<SourceFragment>,
}

impl MappedSlot {
    pub(crate) fn new(contract: SlotContract) -> Self {
        Self {
            contract,
            fragments: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, fragment: SourceFragment) {
        self.fragments.push(fragment);
    }

    pub fn contract(&self) -> &SlotContract {
        &self.contract
    }

    pub fn fragments(&self) -> &[SourceFragment] {
        &self.fragments
    }
}

#[derive(Debug, Clone)]
pub struct UnassignedFragment {
    expected_slot: SlotName,
    fragment: SourceFragment,
}

impl UnassignedFragment {
    pub(crate) fn new(expected_slot: SlotName, fragment: SourceFragment) -> Self {
        Self {
            expected_slot,
            fragment,
        }
    }

    pub fn expected_slot(&self) -> &SlotName {
        &self.expected_slot
    }

    pub fn fragment(&self) -> &SourceFragment {
        &self.fragment
    }
}

#[derive(Debug, Clone)]
pub struct Checked {
    slides: Vec<CheckedSlide>,
}

#[derive(Debug, Clone)]
pub struct CheckedSlide {
    index: usize,
    key: SlideKey,
    slots: BTreeMap<SlotName, Vec<SourceFragment>>,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    slides: Vec<RenderedSlide>,
    css: String,
}

impl Deck<Parsed> {
    pub(crate) fn parsed(slides: Vec<ParsedSlide>) -> Self {
        Self {
            phase: Parsed { slides },
        }
    }

    pub fn parsed_slides(&self) -> &[ParsedSlide] {
        &self.phase.slides
    }

    pub(crate) fn into_parsed_slides(self) -> Vec<ParsedSlide> {
        self.phase.slides
    }
}

impl Deck<Mapped> {
    pub(crate) fn mapped(slides: Vec<MappedSlide>) -> Self {
        Self {
            phase: Mapped { slides },
        }
    }

    pub fn mapped_slides(&self) -> &[MappedSlide] {
        &self.phase.slides
    }

    pub(crate) fn into_mapped_slides(self) -> Vec<MappedSlide> {
        self.phase.slides
    }
}

impl CheckedSlide {
    pub(crate) fn new(
        index: usize,
        key: SlideKey,
        slots: BTreeMap<SlotName, Vec<SourceFragment>>,
    ) -> Self {
        Self { index, key, slots }
    }

    pub(crate) fn index(&self) -> usize {
        self.index
    }

    pub(crate) fn key(&self) -> &SlideKey {
        &self.key
    }

    pub(crate) fn slots(&self) -> &BTreeMap<SlotName, Vec<SourceFragment>> {
        &self.slots
    }

    pub(crate) fn title_text(&self) -> Option<String> {
        let title = SlotName::new("title").ok()?;
        self.slots
            .get(&title)?
            .iter()
            .find_map(SourceFragment::heading_text)
    }
}

impl Deck<Checked> {
    pub(crate) fn checked(slides: Vec<CheckedSlide>) -> Self {
        Self {
            phase: Checked { slides },
        }
    }

    pub fn slide_count(&self) -> usize {
        self.phase.slides.len()
    }

    pub fn slide_keys(&self) -> impl Iterator<Item = &SlideKey> {
        self.phase.slides.iter().map(|slide| &slide.key)
    }

    pub(crate) fn checked_slides(&self) -> &[CheckedSlide] {
        &self.phase.slides
    }

    pub(crate) fn into_checked_slides(self) -> Vec<CheckedSlide> {
        self.phase.slides
    }
}

impl Deck<Rendered> {
    pub(crate) fn rendered(slides: Vec<RenderedSlide>, css: String) -> Self {
        Self {
            phase: Rendered { slides, css },
        }
    }

    pub fn slide_count(&self) -> usize {
        self.phase.slides.len()
    }

    pub fn slides(&self) -> &[RenderedSlide] {
        &self.phase.slides
    }

    pub fn css(&self) -> &str {
        &self.phase.css
    }
}

/// ```compile_fail
/// use peitho_core::{require_checked_for_render, Deck, Mapped};
///
/// fn cannot_render_mapped(deck: &Deck<Mapped>) {
///     require_checked_for_render(deck);
/// }
/// ```
pub fn require_checked_for_render(_: &Deck<Checked>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SlideKey, SourceFragment};

    #[test]
    fn parsed_deck_owns_source_fragments() {
        let deck = Deck::parsed(vec![ParsedSlide {
            key: SlideKey::new("arch-1").unwrap(),
            index: 0,
            key_source: KeySource::Explicit { line: 1 },
            fragments: vec![SourceFragment::paragraph(3, "body")],
        }]);

        assert_eq!(deck.parsed_slides()[0].fragments[0].line(), 3);
    }
}
