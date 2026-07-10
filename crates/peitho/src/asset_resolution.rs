use std::path::{Path, PathBuf};

use miette::IntoDiagnostic;
use peitho_core::{
    error::{BuildError, ErrorKind},
    AssetPath, ParsedFrontmatter,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssets {
    pub layouts: Provenance,
    pub css: Provenance,
    pub syntaxes: Provenance,
    pub fonts: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    Explicit(PathBuf),
    DeckAdjacent(PathBuf),
    Builtin,
}

impl Provenance {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Explicit(path) | Self::DeckAdjacent(path) => Some(path),
            Self::Builtin => None,
        }
    }
}

pub fn resolve_assets(
    deck: &Path,
    frontmatter: &ParsedFrontmatter,
) -> miette::Result<ResolvedAssets> {
    let settings = frontmatter.settings();
    Ok(ResolvedAssets {
        layouts: resolve_asset(deck, frontmatter, "layouts", settings.layouts())?,
        css: resolve_asset(deck, frontmatter, "css", settings.css())?,
        syntaxes: resolve_asset(deck, frontmatter, "syntaxes", settings.syntaxes())?,
        fonts: resolve_asset(deck, frontmatter, "fonts", settings.fonts())?,
    })
}

fn resolve_asset(
    deck: &Path,
    frontmatter: &ParsedFrontmatter,
    key: &'static str,
    value: Option<&AssetPath>,
) -> miette::Result<Provenance> {
    if let Some(value) = value {
        let path = resolve_against_deck(deck, value);
        if !path.try_exists().into_diagnostic()? {
            let line = frontmatter
                .key_line(key)
                .expect("explicit frontmatter asset keys are recorded with line numbers");
            return core(Err(BuildError::new(
                ErrorKind::Parse,
                Some(line),
                format!("{key} path does not exist: {}", path.display()),
                format!(
                    "check the {key}: value in the frontmatter, or remove the key to use deck-adjacent {key}/"
                ),
            )));
        }
        return Ok(Provenance::Explicit(path));
    }

    let conventional = deck_parent(deck).join(key);
    if conventional.is_dir() {
        Ok(Provenance::DeckAdjacent(conventional))
    } else {
        Ok(Provenance::Builtin)
    }
}

fn resolve_against_deck(deck: &Path, value: &AssetPath) -> PathBuf {
    let path = value.as_path();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        deck_parent(deck).join(path)
    }
}

pub(crate) fn deck_parent(deck: &Path) -> &Path {
    deck.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn core<T>(result: peitho_core::Result<T>) -> miette::Result<T> {
    result.map_err(|err| miette::miette!("{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(markdown: &str) -> ParsedFrontmatter {
        peitho_core::parse_frontmatter(markdown).unwrap()
    }

    #[test]
    fn frontmatter_path_resolves_relative_to_deck_parent() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("theme").join("layouts");
        std::fs::create_dir_all(&layouts).unwrap();
        let frontmatter = parse("---\nlayouts: ./theme/layouts\n---\n# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.layouts, Provenance::Explicit(layouts));
    }

    #[test]
    fn explicit_layouts_path_records_explicit_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("theme").join("layouts");
        std::fs::create_dir_all(&layouts).unwrap();
        let frontmatter = parse("---\nlayouts: ./theme/layouts\n---\n# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.layouts, Provenance::Explicit(layouts));
    }

    #[test]
    fn frontmatter_fonts_path_resolves_relative_to_deck_parent() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let fonts = dir.path().join("theme").join("fonts");
        std::fs::create_dir_all(&fonts).unwrap();
        let frontmatter = parse("---\nfonts: ./theme/fonts\n---\n# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.fonts, Provenance::Explicit(fonts));
    }

    #[test]
    fn missing_frontmatter_path_reports_key_line_and_help() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let frontmatter = parse("---\ntime: 15m\ncss: ./missing\n---\n# Intro\n");

        let err = resolve_assets(&deck, &frontmatter).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("css path does not exist"));
        assert!(message.contains("line 3"));
        assert!(message.contains("help: check the css: value"));
        assert!(!message.contains("-->"));
    }

    #[test]
    fn missing_frontmatter_fonts_path_reports_key_line_and_help() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let frontmatter = parse("---\ntime: 15m\nfonts: ./missing\n---\n# Intro\n");

        let err = resolve_assets(&deck, &frontmatter).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("fonts path does not exist"));
        assert!(message.contains("line 3"));
        assert!(message.contains("help: check the fonts: value"));
        assert!(!message.contains("-->"));
    }

    #[test]
    fn explicit_css_path_records_explicit_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let css = dir.path().join("theme").join("css");
        std::fs::create_dir_all(&css).unwrap();
        let frontmatter = parse("---\ncss: ./theme/css\n---\n# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.css, Provenance::Explicit(css));
    }

    #[test]
    fn explicit_syntaxes_path_records_explicit_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let syntaxes = dir.path().join("theme").join("syntaxes");
        std::fs::create_dir_all(&syntaxes).unwrap();
        let frontmatter = parse("---\nsyntaxes: ./theme/syntaxes\n---\n# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.syntaxes, Provenance::Explicit(syntaxes));
    }

    #[test]
    fn deck_adjacent_layouts_directory_records_deck_adjacent_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("layouts");
        std::fs::create_dir_all(&layouts).unwrap();
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.layouts, Provenance::DeckAdjacent(layouts));
    }

    #[test]
    fn deck_adjacent_css_directory_records_deck_adjacent_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let css = dir.path().join("css");
        std::fs::create_dir_all(&css).unwrap();
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.css, Provenance::DeckAdjacent(css));
    }

    #[test]
    fn deck_adjacent_syntaxes_directory_records_deck_adjacent_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let syntaxes = dir.path().join("syntaxes");
        std::fs::create_dir_all(&syntaxes).unwrap();
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.syntaxes, Provenance::DeckAdjacent(syntaxes));
    }

    #[test]
    fn deck_adjacent_fonts_directory_records_deck_adjacent_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let fonts = dir.path().join("fonts");
        std::fs::create_dir_all(&fonts).unwrap();
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.fonts, Provenance::DeckAdjacent(fonts));
    }

    #[test]
    fn missing_layouts_directory_records_builtin_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.layouts, Provenance::Builtin);
    }

    #[test]
    fn missing_css_directory_records_builtin_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.css, Provenance::Builtin);
    }

    #[test]
    fn missing_syntaxes_directory_records_builtin_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.syntaxes, Provenance::Builtin);
    }

    #[test]
    fn missing_fonts_directory_records_builtin_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.fonts, Provenance::Builtin);
    }
}
