use std::path::{Path, PathBuf};

use miette::IntoDiagnostic;
use peitho_core::{
    error::{BuildError, ErrorKind},
    AssetPath, ParsedFrontmatter,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssets {
    pub layouts: Option<PathBuf>,
    pub css: Option<PathBuf>,
    pub syntaxes: Option<PathBuf>,
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
    })
}

fn resolve_asset(
    deck: &Path,
    frontmatter: &ParsedFrontmatter,
    key: &'static str,
    value: Option<&AssetPath>,
) -> miette::Result<Option<PathBuf>> {
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
        return Ok(Some(path));
    }

    let conventional = deck_parent(deck).join(key);
    Ok(conventional.is_dir().then_some(conventional))
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

        assert_eq!(assets.layouts, Some(layouts));
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
    fn deck_adjacent_directory_is_used_without_frontmatter_key() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let css = dir.path().join("css");
        std::fs::create_dir_all(&css).unwrap();
        let frontmatter = parse("# Intro\n");

        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.css, Some(css));
    }
}
