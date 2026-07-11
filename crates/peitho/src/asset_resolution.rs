use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetKey {
    Layouts,
    Css,
    Syntaxes,
    Fonts,
}

impl AssetKey {
    pub(crate) const ALL: [Self; 4] = [Self::Layouts, Self::Css, Self::Syntaxes, Self::Fonts];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Layouts => "layouts",
            Self::Css => "css",
            Self::Syntaxes => "syntaxes",
            Self::Fonts => "fonts",
        }
    }
}

impl Provenance {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Explicit(path) | Self::DeckAdjacent(path) => Some(path),
            Self::Builtin => None,
        }
    }

    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Explicit(_) => "explicit",
            Self::DeckAdjacent(_) => "deck-adjacent",
            Self::Builtin => "built-in",
        }
    }
}

pub fn resolve_assets(
    deck: &Path,
    frontmatter: &ParsedFrontmatter,
) -> miette::Result<ResolvedAssets> {
    Ok(ResolvedAssets {
        layouts: core(resolve_asset(deck, frontmatter, AssetKey::Layouts))?,
        css: core(resolve_asset(deck, frontmatter, AssetKey::Css))?,
        syntaxes: core(resolve_asset(deck, frontmatter, AssetKey::Syntaxes))?,
        fonts: core(resolve_asset(deck, frontmatter, AssetKey::Fonts))?,
    })
}

pub(crate) fn resolve_asset(
    deck: &Path,
    frontmatter: &ParsedFrontmatter,
    key: AssetKey,
) -> peitho_core::Result<Provenance> {
    let settings = frontmatter.settings();
    let value = match key {
        AssetKey::Layouts => settings.layouts(),
        AssetKey::Css => settings.css(),
        AssetKey::Syntaxes => settings.syntaxes(),
        AssetKey::Fonts => settings.fonts(),
    };
    resolve_asset_value(deck, frontmatter, key, value)
}

fn resolve_asset_value(
    deck: &Path,
    frontmatter: &ParsedFrontmatter,
    key: AssetKey,
    value: Option<&AssetPath>,
) -> peitho_core::Result<Provenance> {
    let key_name = key.as_str();
    if let Some(value) = value {
        let path = resolve_against_deck(deck, value);
        let exists = path.try_exists().map_err(|err| {
            BuildError::new(
                ErrorKind::Parse,
                frontmatter.key_line(key_name),
                format!(
                    "{key_name} path could not be checked: {} ({err})",
                    path.display()
                ),
                format!(
                    "check filesystem permissions for the {key_name}: value in the frontmatter"
                ),
            )
        })?;
        if !exists {
            let line = frontmatter
                .key_line(key_name)
                .expect("explicit frontmatter asset keys are recorded with line numbers");
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(line),
                format!("{key_name} path does not exist: {}", path.display()),
                format!(
                    "check the {key_name}: value in the frontmatter, or remove the key to use deck-adjacent {key_name}/"
                ),
            ));
        }
        return Ok(Provenance::Explicit(path));
    }

    let conventional = deck_parent(deck).join(key_name);
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
