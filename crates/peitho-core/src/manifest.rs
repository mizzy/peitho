use serde::{Deserialize, Serialize};

use crate::{
    domain::SlideKey,
    error::{BuildError, ErrorKind, Result},
    phase::{Checked, CheckedSlide, Deck},
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
    slides: Vec<ManifestSlide>,
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

impl Manifest {
    pub fn new(title: impl Into<String>, slides: Vec<ManifestSlide>) -> Self {
        let slide_count = slides.len();
        Self {
            version: 1,
            peitho_version: env!("CARGO_PKG_VERSION").to_owned(),
            title: title.into(),
            slide_count,
            slides,
        }
    }

    pub fn slide_count(&self) -> usize {
        self.slide_count
    }

    pub fn slides(&self) -> &[ManifestSlide] {
        &self.slides
    }
}

pub fn manifest_json(manifest: &Manifest) -> Result<String> {
    let mut json = serde_json::to_string_pretty(manifest).map_err(|err| {
        BuildError::new(
            ErrorKind::Manifest,
            None,
            format!("failed to serialize manifest: {err}"),
            "keep manifest fields serializable",
        )
    })?;
    json.push('\n');
    Ok(json)
}

pub fn build_manifest(deck: &Deck<Checked>) -> Manifest {
    let title = deck
        .checked_slides()
        .first()
        .and_then(CheckedSlide::title_text)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Untitled".to_owned());

    let slides = deck
        .checked_slides()
        .iter()
        .map(|slide| {
            ManifestSlide::new(
                slide.index(),
                slide.key().clone(),
                fragment_src(slide.index(), slide.key()),
                false,
            )
        })
        .collect();

    Manifest::new(title, slides)
}

pub fn fragment_src(index: usize, key: &SlideKey) -> String {
    format!("slides/{index:03}-{}.html", key.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        check::check_deck,
        domain::SlideKey,
        layout::{parse_layout, Layout},
        mapping::map_by_convention,
        parser::parse_markdown,
    };

    #[test]
    fn serializes_manifest_schema_exactly() {
        let manifest = Manifest::new(
            "Peitho Architecture",
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
                "  ]\n",
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

        let manifest = build_manifest(&checked);
        let json = manifest_json(&manifest).unwrap();

        assert!(json.contains(r#""title": "Peitho Architecture""#));
        assert!(json.contains(r#""slideCount": 2"#));
        assert!(json.contains(r#""src": "slides/000-arch-1.html""#));
        assert!(json.contains(r#""src": "slides/001-details.html""#));
        assert!(json.contains(r#""hasNotes": false"#));
    }

    #[test]
    fn deserializes_manifest_schema_for_publish_validation() {
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

        assert_eq!(manifest.slide_count(), 1);
        assert_eq!(manifest.slides()[0].src(), "slides/000-arch-1.html");
        assert_eq!(manifest.slides()[0].key().as_str(), "arch-1");
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

    use super::{Manifest, ManifestSlide};

    #[test]
    fn exports_manifest_bindings_with_serde_field_names() {
        let cfg = Config::from_env();
        Manifest::export_all(&cfg).unwrap();
        ManifestSlide::export_all(&cfg).unwrap();

        let root_bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
        let manifest = fs::read_to_string(root_bindings.join("Manifest.ts")).unwrap();
        let slide = fs::read_to_string(root_bindings.join("ManifestSlide.ts")).unwrap();

        assert!(manifest.contains("peithoVersion: string"));
        assert!(manifest.contains("slideCount: number"));
        assert!(manifest.contains("slides: Array<ManifestSlide>"));
        assert!(slide.contains("key: string"));
        assert!(slide.contains("hasNotes: boolean"));
    }
}
