#![allow(clippy::result_large_err)]

pub mod check;
pub mod code_images;
pub mod domain;
pub mod error;
pub mod highlight;
pub mod include;
mod json;
pub mod layout;
pub mod manifest;
pub mod mapping;
pub mod math;
pub mod notes;
pub mod parser;
pub mod phase;
mod plain;
pub mod present_config;
pub mod rehearsal;
pub mod render;
pub mod theme;

pub const CODE_IMAGES_CACHE_DIR: &str = ".peitho/code-images-cache";

pub use check::check_deck;
pub use code_images::parse_deck_and_transform;
pub use domain::{AspectRatio, RawImagePath, ResolvedImageAsset, ResolvedImagePath};
pub use error::{BuildError, Result};
pub use layout::{describe_layouts, parse_layout, Layout, LayoutSummary, Layouts, SlotSummary};
pub use manifest::{
    build_manifest, fragment_src, manifest_json, Manifest, ManifestImage, ManifestSection,
    ManifestSlide, ManifestSlideText,
};
pub use mapping::{
    dispatch_by_convention, explain_dispatch, map_by_convention, Candidate, CandidateOutcome,
    DispatchResult, DispatchTrace,
};
pub use math::{MathAssets, MathFontAsset};
pub use notes::{notes_json, Notes};
pub use parser::{parse_frontmatter, ParsedFrontmatter};
pub use phase::{
    require_checked_for_render, resolve_image_paths, AssetPath, Checked, Deck, ImageRequest,
    Mapped, Rendered,
};
pub use present_config::{present_config_json, PresentConfig};
pub use rehearsal::{rehearsal_record_json, RehearsalRecord, RehearsalSection, RehearsalSnapshot};
pub use render::{
    render_deck, render_distribution_index, render_lint_document, render_pdf_document,
    render_present_index, render_presenter_index, render_preview_error_index, render_preview_index,
    render_remote_index,
};
pub use theme::{build_theme_css, CssFile};

/// ```compile_fail
/// use peitho_core::*;
///
/// fn raw_checked_deck_cannot_render(deck: Deck<Checked<RawImagePath>>) {
///     let _ = render_deck(deck, &highlight::Highlighter::defaults(), String::new());
/// }
/// ```
pub fn render_deck_requires_resolved_image_paths() {}
