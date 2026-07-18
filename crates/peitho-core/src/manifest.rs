use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    domain::{AspectRatio, ResolvedImageAsset, SlideKey},
    error::Result,
    json::pretty_json,
    phase::{Checked, CheckedSlide, Deck, PlannedTime},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    version: u8,
    peitho_version: String,
    title: String,
    slide_count: usize,
    planned_duration_ms: Option<u64>,
    aspect_ratio: AspectRatio,
    sections: Vec<ManifestSection>,
    slides: Vec<ManifestSlide>,
    images: Vec<ManifestImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestWire {
    version: u8,
    #[serde(rename = "peithoVersion")]
    peitho_version: String,
    title: String,
    #[serde(rename = "slideCount")]
    slide_count: usize,
    #[serde(rename = "plannedDurationMs")]
    planned_duration_ms: Option<u64>,
    #[serde(rename = "aspectRatio", default)]
    aspect_ratio: AspectRatio,
    #[serde(rename = "canvasWidth", skip_deserializing)]
    canvas_width: u32,
    #[serde(rename = "canvasHeight", skip_deserializing)]
    canvas_height: u32,
    #[serde(default)]
    sections: Vec<ManifestSection>,
    slides: Vec<ManifestSlide>,
    #[serde(default)]
    images: Vec<ManifestImage>,
}

#[cfg(any(test, feature = "ts-bindings"))]
#[allow(dead_code)]
#[derive(ts_rs::TS)]
#[ts(rename = "Manifest", export, export_to = "../../bindings/Manifest.ts")]
struct ManifestBinding {
    version: u8,
    #[ts(rename = "peithoVersion")]
    peitho_version: String,
    title: String,
    #[ts(rename = "slideCount")]
    slide_count: usize,
    #[ts(rename = "plannedDurationMs", type = "number | null")]
    planned_duration_ms: Option<u64>,
    #[ts(rename = "aspectRatio")]
    aspect_ratio: AspectRatio,
    #[ts(rename = "canvasWidth")]
    canvas_width: u32,
    #[ts(rename = "canvasHeight")]
    canvas_height: u32,
    sections: Vec<ManifestSection>,
    slides: Vec<ManifestSlide>,
    images: Vec<ManifestImage>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSection {
    name: String,
    #[serde(rename = "startIndex")]
    start_index: usize,
    #[serde(rename = "endIndex")]
    end_index: usize,
    #[serde(rename = "plannedDurationMs")]
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number"))]
    planned_duration_ms: u64,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSlide {
    pub(crate) index: usize,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "string"))]
    pub(crate) key: SlideKey,
    pub(crate) src: String,
    #[serde(rename = "hasNotes")]
    pub(crate) has_notes: bool,
    #[serde(default)]
    pub(crate) skip: bool,
    #[serde(default)]
    pub(crate) text: ManifestSlideText,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ManifestSlideText {
    title: String,
    body: String,
    code: String,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestImage {
    src: String,
}

impl ManifestSection {
    pub fn new(
        name: impl Into<String>,
        start_index: usize,
        end_index: usize,
        planned_duration_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            start_index,
            end_index,
            planned_duration_ms,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn start_index(&self) -> usize {
        self.start_index
    }

    pub fn end_index(&self) -> usize {
        self.end_index
    }

    pub fn planned_duration_ms(&self) -> u64 {
        self.planned_duration_ms
    }
}

impl ManifestSlide {
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn key(&self) -> &SlideKey {
        &self.key
    }

    pub fn src(&self) -> &str {
        &self.src
    }

    pub fn has_notes(&self) -> bool {
        self.has_notes
    }

    pub fn skip(&self) -> bool {
        self.skip
    }

    pub fn text(&self) -> &ManifestSlideText {
        &self.text
    }
}

impl ManifestSlideText {
    pub fn new(title: impl Into<String>, body: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            code: code.into(),
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn code(&self) -> &str {
        &self.code
    }
}

impl ManifestImage {
    pub fn new(src: impl Into<String>) -> Self {
        Self { src: src.into() }
    }

    pub fn src(&self) -> &str {
        &self.src
    }
}

impl Manifest {
    pub fn new(
        title: impl Into<String>,
        planned_duration_ms: Option<u64>,
        aspect_ratio: AspectRatio,
        sections: Vec<ManifestSection>,
        slides: Vec<ManifestSlide>,
    ) -> Self {
        Self::with_images(
            title,
            planned_duration_ms,
            aspect_ratio,
            sections,
            slides,
            Vec::new(),
        )
    }

    pub fn with_images(
        title: impl Into<String>,
        planned_duration_ms: Option<u64>,
        aspect_ratio: AspectRatio,
        sections: Vec<ManifestSection>,
        slides: Vec<ManifestSlide>,
        images: Vec<ManifestImage>,
    ) -> Self {
        let slide_count = slides.len();
        Self {
            version: 1,
            peitho_version: env!("CARGO_PKG_VERSION").to_owned(),
            title: title.into(),
            slide_count,
            planned_duration_ms,
            aspect_ratio,
            sections,
            slides,
            images,
        }
    }

    pub fn slide_count(&self) -> usize {
        self.slide_count
    }

    pub fn planned_duration_ms(&self) -> Option<u64> {
        self.planned_duration_ms
    }

    /// The deck's canvas aspect ratio, from frontmatter `aspect_ratio:` or default 16:9.
    pub fn aspect_ratio(&self) -> AspectRatio {
        self.aspect_ratio
    }

    /// Derived logical canvas width in pixels; equals `aspect_ratio().width()`.
    pub fn canvas_width(&self) -> u32 {
        self.aspect_ratio.width()
    }

    /// Derived logical canvas height in pixels; equals `aspect_ratio().height()`.
    pub fn canvas_height(&self) -> u32 {
        self.aspect_ratio.height()
    }

    pub fn sections(&self) -> &[ManifestSection] {
        &self.sections
    }

    pub fn slides(&self) -> &[ManifestSlide] {
        &self.slides
    }

    pub fn images(&self) -> &[ManifestImage] {
        &self.images
    }
}

impl From<&Manifest> for ManifestWire {
    fn from(manifest: &Manifest) -> Self {
        Self {
            version: manifest.version,
            peitho_version: manifest.peitho_version.clone(),
            title: manifest.title.clone(),
            slide_count: manifest.slide_count,
            planned_duration_ms: manifest.planned_duration_ms,
            aspect_ratio: manifest.aspect_ratio,
            canvas_width: manifest.canvas_width(),
            canvas_height: manifest.canvas_height(),
            sections: manifest.sections.clone(),
            slides: manifest.slides.clone(),
            images: manifest.images.clone(),
        }
    }
}

impl From<ManifestWire> for Manifest {
    fn from(wire: ManifestWire) -> Self {
        Self {
            version: wire.version,
            peitho_version: wire.peitho_version,
            title: wire.title,
            slide_count: wire.slide_count,
            planned_duration_ms: wire.planned_duration_ms,
            aspect_ratio: wire.aspect_ratio,
            sections: wire.sections,
            slides: wire.slides,
            images: wire.images,
        }
    }
}

impl Serialize for Manifest {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ManifestWire::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Manifest {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ManifestWire::deserialize(deserializer).map(Self::from)
    }
}

pub fn manifest_json(manifest: &Manifest) -> Result<String> {
    pretty_json(manifest, "manifest", "keep manifest fields serializable")
}

pub fn build_manifest<S>(deck: &Deck<Checked<S>>, image_assets: &[ResolvedImageAsset]) -> Manifest {
    let title = deck
        .checked_slides()
        .first()
        .and_then(CheckedSlide::title_text)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Untitled".to_owned());

    let sections = deck
        .settings()
        .sections()
        .iter()
        .map(|section| {
            ManifestSection::new(
                section.name(),
                section.start(),
                section.end(),
                section.planned().as_millis(),
            )
        })
        .collect();

    let slides = deck
        .checked_slides()
        .iter()
        .map(|slide| {
            let text = crate::plain::slide_text(slide);
            ManifestSlide {
                index: slide.index(),
                key: slide.key().clone(),
                src: fragment_src(slide.index(), slide.key()),
                has_notes: slide.notes().is_some(),
                skip: slide.skip(),
                text,
            }
        })
        .collect();

    let images = image_assets
        .iter()
        .map(|asset| ManifestImage::new(asset.dist_rel.as_str()))
        .collect();

    Manifest::with_images(
        title,
        deck.settings().planned_time().map(PlannedTime::as_millis),
        deck.settings().aspect_ratio(),
        sections,
        slides,
        images,
    )
}

pub fn fragment_src(index: usize, key: &SlideKey) -> String {
    format!("slides/{index:03}-{}.html", key.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        check::check_deck,
        code_images::{parse_deck_and_transform, SvgRunner},
        domain::{AspectRatio, CodeImageCommand, ResolvedImageAsset, ResolvedImagePath, SlideKey},
        layout::{parse_layout, Layout},
        mapping::map_by_convention,
        parser::{parse_frontmatter, parse_markdown},
    };

    #[test]
    fn manifest_slide_text_and_slide_accessors() {
        let text = ManifestSlideText::new("Title", "Body", "Code");
        let slide = ManifestSlide {
            index: 0,
            key: SlideKey::new("intro").unwrap(),
            src: "slides/000-intro.html".to_owned(),
            has_notes: true,
            skip: true,
            text: text.clone(),
        };

        assert_eq!(text.title(), "Title");
        assert_eq!(text.body(), "Body");
        assert_eq!(text.code(), "Code");
        assert!(slide.has_notes());
        assert!(slide.skip());
        assert_eq!(slide.text(), &text);
    }

    #[test]
    fn serializes_manifest_schema_exactly() {
        let manifest = Manifest::new(
            "Peitho Architecture",
            None,
            AspectRatio::Ratio16To9,
            Vec::new(),
            vec![
                ManifestSlide {
                    index: 0,
                    key: SlideKey::new("arch-1").unwrap(),
                    src: "slides/000-arch-1.html".to_owned(),
                    has_notes: false,
                    skip: false,
                    text: ManifestSlideText::default(),
                },
                ManifestSlide {
                    index: 1,
                    key: SlideKey::new("details").unwrap(),
                    src: "slides/001-details.html".to_owned(),
                    has_notes: false,
                    skip: false,
                    text: ManifestSlideText::default(),
                },
            ],
        );

        let json = manifest_json(&manifest).unwrap();

        assert_eq!(
            json,
            concat!(
                "{\n",
                "  \"version\": 1,\n",
                "  \"peithoVersion\": \"",
                env!("CARGO_PKG_VERSION"),
                "\",\n",
                "  \"title\": \"Peitho Architecture\",\n",
                "  \"slideCount\": 2,\n",
                "  \"plannedDurationMs\": null,\n",
                "  \"aspectRatio\": \"16:9\",\n",
                "  \"canvasWidth\": 1280,\n",
                "  \"canvasHeight\": 720,\n",
                "  \"sections\": [],\n",
                "  \"slides\": [\n",
                "    {\n",
                "      \"index\": 0,\n",
                "      \"key\": \"arch-1\",\n",
                "      \"src\": \"slides/000-arch-1.html\",\n",
                "      \"hasNotes\": false,\n",
                "      \"skip\": false,\n",
                "      \"text\": {\n",
                "        \"title\": \"\",\n",
                "        \"body\": \"\",\n",
                "        \"code\": \"\"\n",
                "      }\n",
                "    },\n",
                "    {\n",
                "      \"index\": 1,\n",
                "      \"key\": \"details\",\n",
                "      \"src\": \"slides/001-details.html\",\n",
                "      \"hasNotes\": false,\n",
                "      \"skip\": false,\n",
                "      \"text\": {\n",
                "        \"title\": \"\",\n",
                "        \"body\": \"\",\n",
                "        \"code\": \"\"\n",
                "      }\n",
                "    }\n",
                "  ],\n",
                "  \"images\": []\n",
                "}\n"
            )
        );
    }

    #[test]
    fn builds_manifest_from_checked_deck() {
        let checked = checked_deck(
            "<!-- {\"key\":\"arch-1\"} -->\n# Peitho Architecture\n\n---\n# Details",
            title_body_layout(),
        );

        let manifest = build_manifest(&checked, &[]);
        let json = manifest_json(&manifest).unwrap();

        assert!(json.contains(r#""title": "Peitho Architecture""#));
        assert!(json.contains(r#""slideCount": 2"#));
        assert!(json.contains(r#""src": "slides/000-arch-1.html""#));
        assert!(json.contains(r#""src": "slides/001-details.html""#));
        assert!(json.contains(r#""hasNotes": false"#));
    }

    #[test]
    fn build_manifest_populates_text_on_slides() {
        let checked = checked_deck(
            "# Peitho\n\nBody text\n\n```rust\nfn main() {}\n```",
            title_body_code_layout(),
        );

        let manifest = build_manifest(&checked, &[]);
        let text = manifest.slides()[0].text();

        assert_eq!(text.title(), "Peitho");
        assert_eq!(text.body(), "Body text");
        assert_eq!(text.code(), "fn main() {}\n");
    }

    #[test]
    fn build_manifest_uses_trimmed_latex_source_for_math_text_from_parser() {
        let checked = checked_deck_with_code_images(
            "# Math\n\nBefore\n\n```math\n\\frac{1}{2}\n```\n\nAfter",
            title_body_layout(),
        );

        let manifest = build_manifest(&checked, &[]);
        let text = manifest.slides()[0].text();

        assert_eq!(text.body(), "Before\n\\frac{1}{2}\nAfter");
        assert!(!text.body().contains("katex-display"));
    }

    #[test]
    fn manifest_json_includes_slide_text() {
        let manifest = Manifest::new(
            "Deck",
            None,
            AspectRatio::Ratio16To9,
            Vec::new(),
            vec![ManifestSlide {
                index: 0,
                key: SlideKey::new("intro").unwrap(),
                src: "slides/000-intro.html".to_owned(),
                has_notes: false,
                skip: false,
                text: ManifestSlideText::new("Intro", "Body text", "fn main() {}\n"),
            }],
        );

        let json = manifest_json(&manifest).unwrap();

        assert!(json.contains(r#""text": {"#));
        assert!(json.contains(r#""title": "Intro""#));
        assert!(json.contains(r#""body": "Body text""#));
        assert!(json.contains(r#""code": "fn main() {}\n""#));
    }

    #[test]
    fn build_manifest_serializes_sections_from_checked_deck() {
        let checked = checked_deck(
            "---\ntime: 3m\n---\n\
             <!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# Intro\n\n---\n# More\n\n---\n\
             <!-- {\"section\":\"Demo\",\"time\":\"2m\"} -->\n# Demo",
            title_body_layout(),
        );

        let manifest = build_manifest(&checked, &[]);

        assert_eq!(manifest.sections()[0].name(), "Setup");
        assert_eq!(manifest.sections()[0].start_index(), 0);
        assert_eq!(manifest.sections()[0].end_index(), 1);
        assert_eq!(manifest.sections()[0].planned_duration_ms(), 60_000);
        assert_eq!(manifest.sections()[1].name(), "Demo");
    }

    #[test]
    fn build_manifest_serializes_aspect_ratio_from_checked_deck() {
        let checked = checked_deck("---\naspect_ratio: 4:3\n---\n# Intro", title_body_layout());

        let manifest = build_manifest(&checked, &[]);
        let json = manifest_json(&manifest).unwrap();

        assert_eq!(manifest.aspect_ratio().width(), 960);
        assert_eq!(manifest.aspect_ratio().height(), 720);
        assert_eq!(manifest.canvas_width(), 960);
        assert_eq!(manifest.canvas_height(), 720);
        assert!(json.contains(r#""aspectRatio": "4:3""#));
        assert!(json.contains(r#""canvasWidth": 960"#));
        assert!(json.contains(r#""canvasHeight": 720"#));
    }

    #[test]
    fn manifest_serializes_default_aspect_ratio() {
        let manifest = Manifest::new(
            "Deck",
            None,
            AspectRatio::Ratio16To9,
            Vec::new(),
            vec![ManifestSlide {
                index: 0,
                key: SlideKey::new("intro").unwrap(),
                src: "slides/000-intro.html".to_owned(),
                has_notes: false,
                skip: false,
                text: ManifestSlideText::default(),
            }],
        );

        let json = manifest_json(&manifest).unwrap();

        assert!(json.contains(r#""aspectRatio": "16:9""#));
        assert!(json.contains(r#""canvasWidth": 1280"#));
        assert!(json.contains(r#""canvasHeight": 720"#));
    }

    #[test]
    fn manifest_serializes_planned_duration_ms() {
        let manifest = Manifest::new(
            "Deck",
            Some(900_000),
            AspectRatio::Ratio16To9,
            Vec::new(),
            vec![ManifestSlide {
                index: 0,
                key: SlideKey::new("intro").unwrap(),
                src: "slides/000-intro.html".to_owned(),
                has_notes: false,
                skip: false,
                text: ManifestSlideText::default(),
            }],
        );

        let json = manifest_json(&manifest).unwrap();

        assert!(json.contains(r#""plannedDurationMs": 900000"#));
    }

    #[test]
    fn build_manifest_marks_has_notes_from_slide_notes() {
        let markdown = "# Intro\n\n<!-- pre-show reminder -->\n\n---\n\n# Plain";
        let checked = checked_deck(markdown, title_body_layout());
        let manifest = build_manifest(&checked, &[]);

        assert!(manifest.slides()[0].has_notes());
        assert!(!manifest.slides()[1].has_notes());
    }

    #[test]
    fn build_manifest_marks_skip_from_checked_slides() {
        let checked = checked_deck(
            "<!-- {\"skip\":true} -->\n# Appendix\n\n---\n# Normal",
            title_body_layout(),
        );

        let manifest = build_manifest(&checked, &[]);
        let json = manifest_json(&manifest).unwrap();

        assert!(manifest.slides()[0].skip());
        assert!(!manifest.slides()[1].skip());
        assert!(json.contains(r#""skip": true"#));
        assert!(json.contains(r#""skip": false"#));
    }

    #[test]
    fn build_manifest_reads_planned_duration_from_checked_deck() {
        let checked = checked_deck("---\ntime: 15m\n---\n# Intro", title_body_layout());
        let manifest = build_manifest(&checked, &[]);

        assert_eq!(manifest.planned_duration_ms(), Some(900_000));
    }

    #[test]
    fn build_manifest_serializes_null_planned_duration_without_frontmatter() {
        let checked = checked_deck("# Intro", title_body_layout());
        let manifest = build_manifest(&checked, &[]);
        let json = manifest_json(&manifest).unwrap();

        assert_eq!(manifest.planned_duration_ms(), None);
        assert!(json.contains(r#""plannedDurationMs": null"#));
    }

    #[test]
    fn manifest_serializes_images_array() {
        let checked = checked_deck("# Intro", title_body_layout());
        let assets = vec![ResolvedImageAsset {
            source_abs: std::path::PathBuf::from("/tmp/arch.png"),
            dist_rel: ResolvedImagePath::from_hashed_asset("0123456789abcdef", "arch.png").unwrap(),
        }];

        let manifest = build_manifest(&checked, &assets);
        let json = manifest_json(&manifest).unwrap();

        assert_eq!(
            manifest.images()[0].src(),
            "assets/0123456789abcdef-arch.png"
        );
        assert!(json.contains(r#""images": ["#));
        assert!(json.contains(r#""src": "assets/0123456789abcdef-arch.png""#));
    }

    #[test]
    fn deserializes_manifest_missing_sections_as_empty_for_additive_compatibility() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": null,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}]\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert!(manifest.sections().is_empty());
    }

    #[test]
    fn deserializes_manifest_missing_slide_skip_as_false() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": null,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"sections\": [],\n",
            "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}],\n",
            "  \"images\": []\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert!(!manifest.slides()[0].skip());
    }

    #[test]
    fn deserializes_manifest_missing_aspect_ratio_and_canvas_as_legacy_16_9() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.4.1\",\n",
            "  \"title\": \"Legacy Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": null,\n",
            "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}]\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.aspect_ratio(), AspectRatio::default());
        assert_eq!(manifest.canvas_width(), 1280);
        assert_eq!(manifest.canvas_height(), 720);
    }

    #[test]
    fn deserializes_manifest_canvas_dimensions_from_aspect_ratio_when_wire_conflicts() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.4.1\",\n",
            "  \"title\": \"Contradictory Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": null,\n",
            "  \"aspectRatio\": \"4:3\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"sections\": [],\n",
            "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}],\n",
            "  \"images\": []\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.aspect_ratio(), AspectRatio::Ratio4To3);
        assert_eq!(manifest.canvas_width(), 960);
        assert_eq!(manifest.canvas_height(), 720);
    }

    #[test]
    fn deserializes_manifest_missing_images_as_empty() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": null,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"sections\": [],\n",
            "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}]\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert!(manifest.images().is_empty());
    }

    #[test]
    fn deserializes_manifest_missing_text_as_empty() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": null,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"sections\": [],\n",
            "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}],\n",
            "  \"images\": []\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.slides()[0].text(), &ManifestSlideText::default());
    }

    #[test]
    fn deserializes_manifest_schema_for_publish_validation() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": null,\n",
            "  \"aspectRatio\": \"4:3\",\n",
            "  \"canvasWidth\": 960,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.slide_count(), 1);
        assert_eq!(manifest.planned_duration_ms(), None);
        assert_eq!(manifest.aspect_ratio(), AspectRatio::Ratio4To3);
        assert_eq!(manifest.canvas_width(), 960);
        assert_eq!(manifest.canvas_height(), 720);
        assert_eq!(manifest.slides()[0].src(), "slides/000-arch-1.html");
        assert_eq!(manifest.slides()[0].key().as_str(), "arch-1");
    }

    #[test]
    fn deserializes_numeric_planned_duration_for_publish_validation() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": 900000,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.planned_duration_ms(), Some(900_000));
    }

    #[test]
    fn deserializes_manifest_missing_planned_duration_as_none_for_additive_compatibility() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.planned_duration_ms(), None);
    }

    #[test]
    fn rejects_invalid_slide_key_when_deserializing_manifest() {
        let json = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"bad key\",\n",
            "      \"src\": \"slides/000-bad.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        );

        let err = serde_json::from_str::<Manifest>(json).unwrap_err();

        assert!(err
            .to_string()
            .contains("slide key must use lowercase ascii"));
    }

    fn checked_deck(markdown: &str, layout: Layout) -> crate::phase::Deck<crate::phase::Checked> {
        let frontmatter = parse_frontmatter(markdown).unwrap();
        check_deck(
            map_by_convention(
                parse_markdown(
                    markdown,
                    frontmatter,
                    &crate::highlight::Highlighter::defaults(),
                )
                .unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn checked_deck_with_code_images(
        markdown: &str,
        layout: Layout,
    ) -> crate::phase::Deck<crate::phase::Checked> {
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let cache = tempfile::tempdir().unwrap();
        let parsed = parse_deck_and_transform(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
            &NoSvgRunner,
            cache.path(),
        )
        .unwrap();
        check_deck(map_by_convention(parsed, &layout).unwrap()).unwrap()
    }

    struct NoSvgRunner;

    impl SvgRunner for NoSvgRunner {
        fn run(&self, _command: &CodeImageCommand, _stdin: &str) -> crate::Result<Vec<u8>> {
            panic!("math manifest test must not invoke external SVG runner");
        }
    }

    fn title_body_layout() -> Layout {
        parse_layout(
            "title-body",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap()
    }

    fn title_body_code_layout() -> Layout {
        parse_layout(
            "title-body-code",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="code" accepts="code" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap()
    }
}

#[cfg(test)]
mod ts_tests {
    use std::{fs, path::Path};

    use ts_rs::{Config, TS};

    use crate::domain::AspectRatio;

    use super::{
        ManifestBinding, ManifestImage, ManifestSection, ManifestSlide, ManifestSlideText,
    };

    #[test]
    fn exports_manifest_bindings_with_serde_field_names() {
        let cfg = Config::from_env();
        AspectRatio::export_all(&cfg).unwrap();
        ManifestBinding::export_all(&cfg).unwrap();
        ManifestImage::export_all(&cfg).unwrap();
        ManifestSection::export_all(&cfg).unwrap();
        ManifestSlide::export_all(&cfg).unwrap();
        ManifestSlideText::export_all(&cfg).unwrap();

        let root_bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
        let aspect_ratio = fs::read_to_string(root_bindings.join("AspectRatio.ts")).unwrap();
        let manifest = fs::read_to_string(root_bindings.join("Manifest.ts")).unwrap();
        let image = fs::read_to_string(root_bindings.join("ManifestImage.ts")).unwrap();
        let section = fs::read_to_string(root_bindings.join("ManifestSection.ts")).unwrap();
        let slide = fs::read_to_string(root_bindings.join("ManifestSlide.ts")).unwrap();
        let slide_text = fs::read_to_string(root_bindings.join("ManifestSlideText.ts")).unwrap();

        assert!(manifest.contains(r#"import type { AspectRatio } from "./AspectRatio";"#));
        assert!(manifest.contains(r#"import type { ManifestImage } from "./ManifestImage";"#));
        assert!(manifest.contains(r#"import type { ManifestSection } from "./ManifestSection";"#));
        assert!(manifest.contains("peithoVersion: string"));
        assert!(manifest.contains("slideCount: number"));
        assert!(manifest.contains("plannedDurationMs: number | null"));
        assert!(manifest.contains("aspectRatio: AspectRatio"));
        assert!(manifest.contains("canvasWidth: number"));
        assert!(manifest.contains("canvasHeight: number"));
        assert!(manifest.contains("sections: Array<ManifestSection>"));
        assert!(manifest.contains("slides: Array<ManifestSlide>"));
        assert!(manifest.contains("images: Array<ManifestImage>"));
        assert!(image.contains("src: string"));
        assert!(section.contains("startIndex: number"));
        assert!(section.contains("endIndex: number"));
        assert!(section.contains("plannedDurationMs: number"));
        assert!(slide.contains("key: string"));
        assert!(slide.contains("hasNotes: boolean"));
        assert!(slide.contains("skip: boolean"));
        assert!(slide.contains(r#"import type { ManifestSlideText } from "./ManifestSlideText";"#));
        assert!(slide.contains("text: ManifestSlideText"));
        assert!(slide_text.contains("title: string"));
        assert!(slide_text.contains("body: string"));
        assert!(slide_text.contains("code: string"));
        assert!(aspect_ratio.contains(r#"export type AspectRatio = "16:9" | "4:3";"#));
    }
}
