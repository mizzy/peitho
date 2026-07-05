use serde::{Deserialize, Serialize};

use crate::{
    domain::{ResolvedImageAsset, SlideKey},
    error::Result,
    json::pretty_json,
    phase::{Checked, CheckedSlide, Deck, PlannedTime},
};

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    version: u8,
    #[serde(rename = "peithoVersion")]
    peitho_version: String,
    title: String,
    #[serde(rename = "slideCount")]
    slide_count: usize,
    #[serde(rename = "plannedDurationMs")]
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "number | null"))]
    planned_duration_ms: Option<u64>,
    #[serde(default)]
    sections: Vec<ManifestSection>,
    slides: Vec<ManifestSlide>,
    #[serde(default)]
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
    index: usize,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "string"))]
    key: SlideKey,
    src: String,
    #[serde(rename = "hasNotes")]
    has_notes: bool,
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
    pub fn new(index: usize, key: SlideKey, src: impl Into<String>, has_notes: bool) -> Self {
        Self {
            index,
            key,
            src: src.into(),
            has_notes,
        }
    }

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
        sections: Vec<ManifestSection>,
        slides: Vec<ManifestSlide>,
    ) -> Self {
        Self::with_images(title, planned_duration_ms, sections, slides, Vec::new())
    }

    pub fn with_images(
        title: impl Into<String>,
        planned_duration_ms: Option<u64>,
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
            ManifestSlide::new(
                slide.index(),
                slide.key().clone(),
                fragment_src(slide.index(), slide.key()),
                slide.notes().is_some(),
            )
        })
        .collect();

    let images = image_assets
        .iter()
        .map(|asset| ManifestImage::new(asset.dist_rel.as_str()))
        .collect();

    Manifest::with_images(
        title,
        deck.settings().planned_time().map(PlannedTime::as_millis),
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
        domain::{ResolvedImageAsset, ResolvedImagePath, SlideKey},
        layout::{parse_layout, Layout},
        mapping::map_by_convention,
        parser::parse_markdown,
    };

    #[test]
    fn serializes_manifest_schema_exactly() {
        let manifest = Manifest::new(
            "Peitho Architecture",
            None,
            Vec::new(),
            vec![
                ManifestSlide::new(
                    0,
                    SlideKey::new("arch-1").unwrap(),
                    "slides/000-arch-1.html",
                    false,
                ),
                ManifestSlide::new(
                    1,
                    SlideKey::new("details").unwrap(),
                    "slides/001-details.html",
                    false,
                ),
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
                "  \"sections\": [],\n",
                "  \"slides\": [\n",
                "    {\n",
                "      \"index\": 0,\n",
                "      \"key\": \"arch-1\",\n",
                "      \"src\": \"slides/000-arch-1.html\",\n",
                "      \"hasNotes\": false\n",
                "    },\n",
                "    {\n",
                "      \"index\": 1,\n",
                "      \"key\": \"details\",\n",
                "      \"src\": \"slides/001-details.html\",\n",
                "      \"hasNotes\": false\n",
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
    fn manifest_serializes_planned_duration_ms() {
        let manifest = Manifest::new(
            "Deck",
            Some(900_000),
            Vec::new(),
            vec![ManifestSlide::new(
                0,
                SlideKey::new("intro").unwrap(),
                "slides/000-intro.html",
                false,
            )],
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
            "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}]\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert!(manifest.sections().is_empty());
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
            "  \"sections\": [],\n",
            "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}]\n",
            "}\n"
        );

        let manifest: Manifest = serde_json::from_str(json).unwrap();

        assert!(manifest.images().is_empty());
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
        check_deck(map_by_convention(parse_markdown(markdown).unwrap(), &layout).unwrap()).unwrap()
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
}

#[cfg(test)]
mod ts_tests {
    use std::{fs, path::Path};

    use ts_rs::{Config, TS};

    use super::{Manifest, ManifestImage, ManifestSection, ManifestSlide};

    #[test]
    fn exports_manifest_bindings_with_serde_field_names() {
        let cfg = Config::from_env();
        Manifest::export_all(&cfg).unwrap();
        ManifestImage::export_all(&cfg).unwrap();
        ManifestSection::export_all(&cfg).unwrap();
        ManifestSlide::export_all(&cfg).unwrap();

        let root_bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
        let manifest = fs::read_to_string(root_bindings.join("Manifest.ts")).unwrap();
        let image = fs::read_to_string(root_bindings.join("ManifestImage.ts")).unwrap();
        let section = fs::read_to_string(root_bindings.join("ManifestSection.ts")).unwrap();
        let slide = fs::read_to_string(root_bindings.join("ManifestSlide.ts")).unwrap();

        assert!(manifest.contains(r#"import type { ManifestImage } from "./ManifestImage";"#));
        assert!(manifest.contains(r#"import type { ManifestSection } from "./ManifestSection";"#));
        assert!(manifest.contains("peithoVersion: string"));
        assert!(manifest.contains("slideCount: number"));
        assert!(manifest.contains("plannedDurationMs: number | null"));
        assert!(manifest.contains("sections: Array<ManifestSection>"));
        assert!(manifest.contains("slides: Array<ManifestSlide>"));
        assert!(manifest.contains("images: Array<ManifestImage>"));
        assert!(image.contains("src: string"));
        assert!(section.contains("startIndex: number"));
        assert!(section.contains("endIndex: number"));
        assert!(section.contains("plannedDurationMs: number"));
        assert!(slide.contains("key: string"));
        assert!(slide.contains("hasNotes: boolean"));
    }
}
