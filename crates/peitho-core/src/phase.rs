use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::{
    domain::{
        AspectRatio, RawImagePath, RenderedSlide, Resolution, ResolvedImageAsset,
        ResolvedImagePath, SlideKey, SlotContract, SlotName, SourceFragment,
    },
    error::{BuildError, Result},
    layout::Layout,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedTime(u64);

impl PlannedTime {
    pub(crate) const GREATER_THAN_ZERO_MESSAGE: &'static str = "time must be greater than zero";
    pub(crate) const MAX_SAFE_JAVASCRIPT_INTEGER_MILLIS: u64 = 9_007_199_254_740_991;
    pub(crate) const TOO_LARGE_MESSAGE: &'static str = "time is too large";

    pub(crate) fn from_millis(millis: u64) -> std::result::Result<Self, String> {
        if millis == 0 {
            Err(Self::GREATER_THAN_ZERO_MESSAGE.to_owned())
        } else if millis > Self::MAX_SAFE_JAVASCRIPT_INTEGER_MILLIS {
            Err(Self::TOO_LARGE_MESSAGE.to_owned())
        } else {
            Ok(Self(millis))
        }
    }

    pub fn as_millis(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetPath(PathBuf);

impl AssetPath {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckSection {
    name: String,
    planned: PlannedTime,
    start: usize,
    end: usize,
}

impl DeckSection {
    pub(crate) fn new(name: String, planned: PlannedTime, start: usize, end: usize) -> Self {
        Self {
            name,
            planned,
            start,
            end,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn planned(&self) -> PlannedTime {
        self.planned
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeckSettings {
    planned_time: Option<PlannedTime>,
    aspect_ratio: AspectRatio,
    resolution: Resolution,
    breaks: bool,
    sections: Vec<DeckSection>,
    layouts: Option<AssetPath>,
    css: Option<AssetPath>,
    syntaxes: Option<AssetPath>,
    fonts: Option<AssetPath>,
}

impl DeckSettings {
    /// Creates deck settings without enforcing parser-end invariants.
    ///
    /// When `sections` is non-empty, the `planned_time == sum(sections.time)`
    /// invariant is enforced only by `finalize_section_settings` at the end of
    /// parsing. Direct callers of this constructor must uphold that invariant
    /// themselves.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        planned_time: Option<PlannedTime>,
        aspect_ratio: AspectRatio,
        resolution: Option<Resolution>,
        breaks: bool,
        sections: Vec<DeckSection>,
        layouts: Option<AssetPath>,
        css: Option<AssetPath>,
        syntaxes: Option<AssetPath>,
        fonts: Option<AssetPath>,
    ) -> std::result::Result<Self, String> {
        let resolution =
            resolution.unwrap_or_else(|| Resolution::from_aspect_ratio_default(aspect_ratio));
        resolution.check_matches(aspect_ratio)?;
        resolution.check_not_smaller_than_canvas(aspect_ratio)?;
        Ok(Self {
            planned_time,
            aspect_ratio,
            resolution,
            breaks,
            sections,
            layouts,
            css,
            syntaxes,
            fonts,
        })
    }

    pub fn planned_time(&self) -> Option<PlannedTime> {
        self.planned_time
    }

    pub fn aspect_ratio(&self) -> AspectRatio {
        self.aspect_ratio
    }

    pub fn resolution(&self) -> Resolution {
        self.resolution
    }

    pub fn breaks(&self) -> bool {
        self.breaks
    }

    pub fn sections(&self) -> &[DeckSection] {
        &self.sections
    }

    pub fn layouts(&self) -> Option<&AssetPath> {
        self.layouts.as_ref()
    }

    pub fn css(&self) -> Option<&AssetPath> {
        self.css.as_ref()
    }

    pub fn syntaxes(&self) -> Option<&AssetPath> {
        self.syntaxes.as_ref()
    }

    pub fn fonts(&self) -> Option<&AssetPath> {
        self.fonts.as_ref()
    }

    pub(crate) fn with_sections(mut self, sections: Vec<DeckSection>) -> Self {
        self.sections = sections;
        self
    }

    pub(crate) fn with_planned_time(mut self, planned_time: Option<PlannedTime>) -> Self {
        self.planned_time = planned_time;
        self
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
    pub notes: Option<String>,
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
    pub(crate) notes: Option<String>,
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
pub struct Checked<S = RawImagePath> {
    slides: Vec<CheckedSlide<S>>,
}

#[derive(Debug, Clone)]
pub struct CheckedSlide<S = RawImagePath> {
    index: usize,
    key: SlideKey,
    layout: Layout,
    slots: BTreeMap<SlotName, CheckedSlot<S>>,
    notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CheckedSlot<S = RawImagePath> {
    contract: SlotContract,
    fragments: Vec<SourceFragment<S>>,
}

#[derive(Debug, Clone, Copy)]
/// Context passed to a caller-provided image resolver.
///
/// The resolver receives the validated raw Markdown path plus slide/line
/// context so it can report asset I/O errors without losing source location.
pub struct ImageRequest<'a> {
    /// Validated deck-relative path from Markdown.
    pub raw: &'a RawImagePath,
    /// Source line of the image fragment.
    pub line: usize,
    /// Zero-based slide index containing the image.
    pub slide_index: usize,
    /// Stable slide key containing the image.
    pub slide_key: &'a SlideKey,
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

impl<S> CheckedSlide<S> {
    pub(crate) fn new(
        index: usize,
        key: SlideKey,
        layout: Layout,
        slots: BTreeMap<SlotName, CheckedSlot<S>>,
        notes: Option<String>,
    ) -> Self {
        Self {
            index,
            key,
            layout,
            slots,
            notes,
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

    pub(crate) fn slots(&self) -> &BTreeMap<SlotName, CheckedSlot<S>> {
        &self.slots
    }

    pub(crate) fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }

    pub(crate) fn title_text(&self) -> Option<String> {
        let title = SlotName::new("title").ok()?;
        self.slots
            .get(&title)?
            .fragments()
            .iter()
            .find_map(SourceFragment::heading_text)
    }
}

impl<S> CheckedSlot<S> {
    pub(crate) fn new(contract: SlotContract, fragments: Vec<SourceFragment<S>>) -> Self {
        Self {
            contract,
            fragments,
        }
    }

    pub fn contract(&self) -> &SlotContract {
        &self.contract
    }

    pub fn fragments(&self) -> &[SourceFragment<S>] {
        &self.fragments
    }
}

impl<S> Deck<Checked<S>> {
    pub(crate) fn checked(settings: DeckSettings, slides: Vec<CheckedSlide<S>>) -> Self {
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

    pub(crate) fn checked_slides(&self) -> &[CheckedSlide<S>] {
        &self.phase.slides
    }

    pub(crate) fn into_checked_parts(self) -> (DeckSettings, Vec<CheckedSlide<S>>) {
        (self.settings, self.phase.slides)
    }
}

/// Convert a checked deck from raw Markdown image paths to renderable assets.
///
/// This is the only transition from `Deck<Checked<RawImagePath>>` to
/// `Deck<Checked<ResolvedImagePath>>`. Callers must provide a resolver that
/// turns each raw deck-relative path into a typed distribution-relative asset;
/// `render_deck` only accepts the resolved form.
pub fn resolve_image_paths<R>(
    deck: Deck<Checked<RawImagePath>>,
    mut resolver: R,
) -> Result<(Deck<Checked<ResolvedImagePath>>, Vec<ResolvedImageAsset>)>
where
    R: FnMut(ImageRequest<'_>) -> Result<ResolvedImageAsset>,
{
    let (settings, checked_slides) = deck.into_checked_parts();
    let mut slides = Vec::with_capacity(checked_slides.len());
    let mut assets = Vec::new();
    let mut asset_paths = BTreeSet::new();

    for slide in checked_slides {
        let CheckedSlide {
            index,
            key,
            layout,
            slots,
            notes,
        } = slide;
        let slide_number = index + 1;
        let slide_key_for_error = key.as_str().to_owned();
        let mut resolved_slots = BTreeMap::new();

        for (slot, checked_slot) in slots {
            let CheckedSlot {
                contract,
                fragments,
            } = checked_slot;
            let mut resolved_fragments = Vec::with_capacity(fragments.len());
            for fragment in fragments {
                let line = fragment.line();
                let resolved = fragment.try_map_image_src(|raw| -> Result<ResolvedImagePath> {
                    let request = ImageRequest {
                        raw: &raw,
                        line,
                        slide_index: index,
                        slide_key: &key,
                    };
                    let asset = resolver(request).map_err(|err| {
                        attach_image_resolve_context(
                            err,
                            line,
                            slide_number,
                            Some(&slide_key_for_error),
                        )
                    })?;
                    let dist_rel = asset.dist_rel.clone();
                    if asset_paths.insert(dist_rel.as_str().to_owned()) {
                        assets.push(asset);
                    }
                    Ok(dist_rel)
                })?;
                resolved_fragments.push(resolved);
            }
            resolved_slots.insert(slot, CheckedSlot::new(contract, resolved_fragments));
        }

        slides.push(CheckedSlide::new(index, key, layout, resolved_slots, notes));
    }

    Ok((Deck::checked(settings, slides), assets))
}

fn attach_image_resolve_context(
    mut err: BuildError,
    line: usize,
    slide_number: usize,
    slide_key: Option<&str>,
) -> BuildError {
    if err.line.is_none() {
        err.line = Some(line);
    }
    if err.slide.is_none() {
        err = err.with_slide(slide_number, slide_key);
    }
    err
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
pub fn require_checked_for_render(_: &Deck<Checked<ResolvedImagePath>>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{RawImagePath, ResolvedImageAsset, ResolvedImagePath, SlideKey, SourceFragment},
        layout::parse_layout,
    };
    use std::path::PathBuf;

    #[test]
    fn planned_time_accepts_javascript_safe_integer_boundary() {
        let planned = PlannedTime::from_millis(PlannedTime::MAX_SAFE_JAVASCRIPT_INTEGER_MILLIS)
            .expect("boundary is accepted");

        assert_eq!(
            planned.as_millis(),
            PlannedTime::MAX_SAFE_JAVASCRIPT_INTEGER_MILLIS
        );
    }

    #[test]
    fn planned_time_rejects_values_above_javascript_safe_integer_boundary() {
        let err = PlannedTime::from_millis(PlannedTime::MAX_SAFE_JAVASCRIPT_INTEGER_MILLIS + 1)
            .unwrap_err();

        assert_eq!(err, PlannedTime::TOO_LARGE_MESSAGE);
    }

    #[test]
    fn deck_settings_carry_owned_sections() {
        let setup = DeckSection::new(
            "Setup".to_owned(),
            PlannedTime::from_millis(60_000).unwrap(),
            0,
            1,
        );
        let settings = DeckSettings::new(
            Some(setup.planned()),
            AspectRatio::default(),
            Some(Resolution::from_aspect_ratio_default(AspectRatio::default())),
            false,
            vec![setup.clone()],
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(settings.planned_time().unwrap().as_millis(), 60_000);
        assert_eq!(settings.sections(), &[setup]);
        assert_eq!(settings.sections()[0].name(), "Setup");
        assert_eq!(settings.sections()[0].start(), 0);
        assert_eq!(settings.sections()[0].end(), 1);
    }

    #[test]
    fn deck_settings_with_planned_time_preserves_sections() {
        let setup = DeckSection::new(
            "Setup".to_owned(),
            PlannedTime::from_millis(60_000).unwrap(),
            0,
            1,
        );
        let settings = DeckSettings::new(
            None,
            AspectRatio::default(),
            Some(Resolution::from_aspect_ratio_default(AspectRatio::default())),
            false,
            vec![setup.clone()],
            None,
            None,
            None,
            None,
        )
        .unwrap()
        .with_planned_time(Some(PlannedTime::from_millis(120_000).unwrap()));

        assert_eq!(settings.planned_time().unwrap().as_millis(), 120_000);
        assert_eq!(settings.sections(), &[setup]);
    }

    #[test]
    fn deck_settings_new_derives_resolution_when_absent() {
        let settings = DeckSettings::new(
            None,
            AspectRatio::Ratio4To3,
            None,
            false,
            Vec::new(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(settings.resolution().width(), 1440);
        assert_eq!(settings.resolution().height(), 1080);
    }

    #[test]
    fn deck_settings_new_rejects_resolution_that_mismatches_aspect_ratio() {
        let err = DeckSettings::new(
            None,
            AspectRatio::Ratio16To9,
            Some(Resolution::from_frontmatter("1024x768").unwrap()),
            false,
            Vec::new(),
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(err, "resolution 1024x768 does not match aspect_ratio 16:9");
    }

    #[test]
    fn deck_settings_new_rejects_resolution_smaller_than_canvas() {
        let err = DeckSettings::new(
            None,
            AspectRatio::Ratio16To9,
            Some(Resolution::from_frontmatter("16x9").unwrap()),
            false,
            Vec::new(),
            None,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert_eq!(
            err,
            "resolution 16x9 is smaller than the canvas logical size 1280x720; use at least the canvas dimensions"
        );
    }

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
                notes: None,
            }],
        );

        assert_eq!(deck.parsed_slides()[0].fragments[0].line(), 3);
    }

    #[test]
    fn resolve_image_paths_deduplicates_assets_by_dist_path() {
        let layout = parse_layout(
            "images",
            r#"<section><slot name="hero" accepts="image" arity="1..*"></slot></section>"#,
        )
        .unwrap();
        let hero = SlotName::new("hero").unwrap();
        let contract = layout.slot("hero").unwrap().clone();
        let mut slots = BTreeMap::new();
        slots.insert(
            hero,
            CheckedSlot::new(
                contract,
                vec![
                    SourceFragment::image(3, "A", RawImagePath::new_unchecked("a.png".into())),
                    SourceFragment::image(5, "B", RawImagePath::new_unchecked("b.png".into())),
                ],
            ),
        );
        let deck = Deck::checked(
            DeckSettings::default(),
            vec![CheckedSlide::new(
                0,
                SlideKey::new("gallery").unwrap(),
                layout,
                slots,
                None,
            )],
        );
        let dist_rel = ResolvedImagePath::from_string("assets/same-a.png".to_owned());
        let mut calls = 0;

        let (_resolved, assets) = resolve_image_paths(deck, |_request| {
            calls += 1;
            Ok(ResolvedImageAsset {
                source_abs: PathBuf::from("/tmp/a.png"),
                dist_rel: dist_rel.clone(),
            })
        })
        .unwrap();

        assert_eq!(calls, 2);
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].dist_rel.as_str(), "assets/same-a.png");
    }
}
