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
pub mod present_config;
pub mod render;
pub mod theme;

pub use check::check_deck;
pub use error::{BuildError, Result};
pub use layout::{parse_layout, Layout, Layouts};
pub use manifest::{
    build_manifest, fragment_src, manifest_json, Manifest, ManifestSection, ManifestSlide,
};
pub use mapping::{dispatch_by_convention, map_by_convention};
pub use notes::{notes_json, Notes};
pub use parser::parse_markdown;
pub use phase::{require_checked_for_render, Deck, Mapped, Rendered};
pub use present_config::{present_config_json, PresentConfig};
pub use render::{
    render_deck, render_distribution_index, render_present_index, render_presenter_index,
};
pub use theme::{build_theme_css, CssFile};
