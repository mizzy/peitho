use std::collections::{BTreeMap, BTreeSet};

use crate::{
    domain::{RenderedSlide, SlideKey, SlotContract, SlotName, SourceFragment},
    layout::Layout,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedTime(u64);

impl PlannedTime {
    pub(crate) const GREATER_THAN_ZERO_MESSAGE: &'static str = "time must be greater than zero";

    pub(crate) fn from_millis(millis: u64) -> std::result::Result<Self, String> {
        if millis == 0 {
            Err(Self::GREATER_THAN_ZERO_MESSAGE.to_owned())
        } else {
            Ok(Self(millis))
        }
    }

    pub fn as_millis(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeckSettings {
    planned_time: Option<PlannedTime>,
}

impl DeckSettings {
    pub fn new(planned_time: Option<PlannedTime>) -> Self {
        Self { planned_time }
    }

    pub fn planned_time(&self) -> Option<PlannedTime> {
        self.planned_time
    }
}

#[derive(Debug, Clone)]
pub struct Deck<P> {
    settings: DeckSettings,
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

/// An explicit `{"layout":"name"}` request from the slide's page settings
/// comment. The name is resolved against the provided layouts at dispatch;
/// the line makes an unknown name a位置付きビルドエラー.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutRequest {
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct ParsedSlide {
    pub index: usize,
    pub key: SlideKey,
    pub key_source: KeySource,
    pub layout_request: Option<LayoutRequest>,
    pub fragments: Vec<SourceFragment>,
}

#[derive(Debug, Clone)]
pub struct Mapped {
    slides: Vec<MappedSlide>,
}

/// A mapped slide carries the layout it was dispatched to, so the later
/// phases never look a layout up again (and thus have no failure path for
/// a missing layout).
#[derive(Debug, Clone)]
pub struct MappedSlide {
    pub(crate) index: usize,
    pub(crate) key: SlideKey,
    pub(crate) layout: Layout,
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
    layout: Layout,
    slots: BTreeMap<SlotName, Vec<SourceFragment>>,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    slides: Vec<RenderedSlide>,
    css: String,
}

impl<P> Deck<P> {
    pub fn settings(&self) -> &DeckSettings {
        &self.settings
    }
}

impl Deck<Parsed> {
    pub(crate) fn parsed(settings: DeckSettings, slides: Vec<ParsedSlide>) -> Self {
        Self {
            settings,
            phase: Parsed { slides },
        }
    }

    pub fn parsed_slides(&self) -> &[ParsedSlide] {
        &self.phase.slides
    }

    pub(crate) fn into_parsed_parts(self) -> (DeckSettings, Vec<ParsedSlide>) {
        (self.settings, self.phase.slides)
    }
}

impl Deck<Mapped> {
    pub(crate) fn mapped(settings: DeckSettings, slides: Vec<MappedSlide>) -> Self {
        Self {
            settings,
            phase: Mapped { slides },
        }
    }

    pub fn mapped_slides(&self) -> &[MappedSlide] {
        &self.phase.slides
    }

    pub(crate) fn into_mapped_parts(self) -> (DeckSettings, Vec<MappedSlide>) {
        (self.settings, self.phase.slides)
    }
}

impl CheckedSlide {
    pub(crate) fn new(
        index: usize,
        key: SlideKey,
        layout: Layout,
        slots: BTreeMap<SlotName, Vec<SourceFragment>>,
    ) -> Self {
        Self {
            index,
            key,
            layout,
            slots,
        }
    }

    pub(crate) fn layout(&self) -> &Layout {
        &self.layout
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
    pub(crate) fn checked(settings: DeckSettings, slides: Vec<CheckedSlide>) -> Self {
        Self {
            settings,
            phase: Checked { slides },
        }
    }

    pub fn slide_count(&self) -> usize {
        self.phase.slides.len()
    }

    pub fn slide_keys(&self) -> impl Iterator<Item = &SlideKey> {
        self.phase.slides.iter().map(|slide| &slide.key)
    }

    /// Slide key → slot class names of that slide's layout, plus the union
    /// used to validate override selectors that name no slide key.
    pub fn slide_slot_classes(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.phase
            .slides
            .iter()
            .map(|slide| {
                (
                    slide.key.as_str().to_owned(),
                    slide
                        .layout
                        .slots()
                        .keys()
                        .map(SlotName::class_name)
                        .collect(),
                )
            })
            .collect()
    }

    pub(crate) fn checked_slides(&self) -> &[CheckedSlide] {
        &self.phase.slides
    }

    pub(crate) fn into_checked_parts(self) -> (DeckSettings, Vec<CheckedSlide>) {
        (self.settings, self.phase.slides)
    }
}

impl Deck<Rendered> {
    pub(crate) fn rendered(
        settings: DeckSettings,
        slides: Vec<RenderedSlide>,
        css: String,
    ) -> Self {
        Self {
            settings,
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
        let deck = Deck::parsed(
            DeckSettings::default(),
            vec![ParsedSlide {
                key: SlideKey::new("arch-1").unwrap(),
                index: 0,
                key_source: KeySource::Explicit { line: 1 },
                layout_request: None,
                fragments: vec![SourceFragment::paragraph(3, "body")],
            }],
        );

        assert_eq!(deck.parsed_slides()[0].fragments[0].line(), 3);
    }
}
