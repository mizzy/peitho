pub mod check;
pub mod domain;
pub mod error;
pub mod mapping;
pub mod parser;
pub mod phase;
pub mod render;
pub mod template;
pub mod theme;

pub use check::check_deck;
pub use error::{BuildError, Result};
pub use mapping::map_by_convention;
pub use parser::parse_markdown;
pub use phase::{require_checked_for_render, Deck, Mapped};
pub use render::{render_deck, render_index};
pub use template::parse_template;
pub use theme::build_theme_css;
