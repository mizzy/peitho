pub mod check;
pub mod domain;
pub mod error;
pub mod highlight;
mod json;
pub mod layout;
pub mod manifest;
pub mod mapping;
pub mod notes;
pub mod parser;
pub mod phase;
mod plain;
pub mod present_config;
pub mod render;
pub mod theme;

pub use check::check_deck;
pub use domain::{AspectRatio, RawImagePath, ResolvedImageAsset, ResolvedImagePath};
pub use error::{BuildError, Result};
pub use layout::{parse_layout, Layout, Layouts};
pub use manifest::{
    build_manifest, fragment_src, manifest_json, Manifest, ManifestImage, ManifestSection,
    ManifestSlide, ManifestSlideText,
};
pub use mapping::{dispatch_by_convention, map_by_convention};
pub use notes::{notes_json, Notes};
pub use parser::{parse_frontmatter, parse_markdown, ParsedFrontmatter};
pub use phase::{
    require_checked_for_render, resolve_image_paths, AssetPath, Checked, Deck, ImageRequest,
    Mapped, Rendered,
};
pub use present_config::{present_config_json, PresentConfig};
pub use render::{
    render_deck, render_distribution_index, render_pdf_document, render_present_index,
    render_presenter_index,
};
pub use theme::{build_theme_css, CssFile};

/// ```compile_fail
/// use peitho_core::*;
///
/// fn raw_checked_deck_cannot_render(deck: Deck<Checked<RawImagePath>>) {
///     let _ = render_deck(deck, &highlight::Highlighter::defaults());
/// }
/// ```
pub fn render_deck_requires_resolved_image_paths() {}
