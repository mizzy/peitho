use serde::Serialize;

use crate::{
    domain::SlideKey,
    error::{BuildError, ErrorKind, Result},
    phase::{Checked, CheckedSlide, Deck},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    version: u8,
    #[serde(rename = "peithoVersion")]
    peitho_version: String,
    title: String,
    #[serde(rename = "slideCount")]
    slide_count: usize,
    slides: Vec<ManifestSlide>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestSlide {
    index: usize,
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
        mapping::map_by_convention,
        parser::parse_markdown,
        template::{parse_template, Template},
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
            title_body_template(),
        );

        let manifest = build_manifest(&checked);
        let json = manifest_json(&manifest).unwrap();

        assert!(json.contains(r#""title": "Peitho Architecture""#));
        assert!(json.contains(r#""slideCount": 2"#));
        assert!(json.contains(r#""src": "slides/000-arch-1.html""#));
        assert!(json.contains(r#""src": "slides/001-details.html""#));
        assert!(json.contains(r#""hasNotes": false"#));
    }

    fn checked_deck(
        markdown: &str,
        template: Template,
    ) -> crate::phase::Deck<crate::phase::Checked> {
        check_deck(
            map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap(),
            &template,
        )
        .unwrap()
    }

    fn title_body_template() -> Template {
        parse_template(
            "title-body",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap()
    }
}
