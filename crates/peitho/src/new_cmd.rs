use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use clap::ValueEnum;
use miette::IntoDiagnostic;

const DECK_DEFAULT_MD: &str = include_str!("../templates/new/deck-default.md");
const DECK_SPLIT_MD: &str = include_str!("../templates/new/deck-split.md");
const DECK_COVER_MD: &str = include_str!("../templates/new/deck-cover.md");
const TWO_COLUMN_HTML: &str = include_str!("../templates/new/two-column.html");
const COVER_HTML: &str = include_str!("../templates/new/cover.html");
const TWO_COLUMN_CSS: &str = include_str!("../templates/new/two-column.css");
const COVER_CSS: &str = include_str!("../templates/new/cover.css");
const DARK_CSS: &str = include_str!("../templates/new/dark.css");
const DARK_TWO_COLUMN_CSS: &str = include_str!("../templates/new/dark-two-column.css");
const DARK_COVER_CSS: &str = include_str!("../templates/new/dark-cover.css");
const GITIGNORE: &str = include_str!("../templates/new/gitignore");

const BASE_CSS_HEADER: &str = "/*\n  This file replaces peitho's embedded themes/base.css for this deck.\n  Edit it as your deck's complete theme.\n*/\n\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LayoutVariant {
    Default,
    Split,
    Cover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ThemeVariant {
    Light,
    Dark,
}

#[derive(Debug)]
pub(crate) struct ScaffoldFile {
    pub(crate) relative_path: PathBuf,
    pub(crate) content: String,
}

#[derive(Debug)]
pub(crate) struct NewOptions {
    pub(crate) target: PathBuf,
    pub(crate) layouts: LayoutVariant,
    pub(crate) theme: ThemeVariant,
    pub(crate) force: bool,
}

pub(crate) fn plan(layouts: LayoutVariant, theme: ThemeVariant) -> Vec<ScaffoldFile> {
    let mut files = vec![
        scaffold_file("deck.md", deck_template(layouts)),
        scaffold_file("layouts/title-body-code.html", crate::BUILTIN_LAYOUT_HTML),
    ];

    match layouts {
        LayoutVariant::Default => {}
        LayoutVariant::Split => {
            files.push(scaffold_file("layouts/two-column.html", TWO_COLUMN_HTML));
        }
        LayoutVariant::Cover => {
            files.push(scaffold_file("layouts/cover.html", COVER_HTML));
        }
    }

    files.push(ScaffoldFile {
        relative_path: PathBuf::from("css/base.css"),
        content: base_css(layouts, theme),
    });
    files.push(scaffold_file(".gitignore", GITIGNORE));
    files
}

pub(crate) fn run(options: NewOptions, stdout: &mut dyn Write) -> miette::Result<()> {
    ensure_target_dir(&options.target, options.force)?;
    let files = plan(options.layouts, options.theme);

    for file in &files {
        write_scaffold_file(&options.target, file)?;
    }

    print_success(stdout, &options.target, &files)?;
    Ok(())
}

fn scaffold_file(path: impl Into<PathBuf>, content: &str) -> ScaffoldFile {
    ScaffoldFile {
        relative_path: path.into(),
        content: content.to_owned(),
    }
}

fn deck_template(layouts: LayoutVariant) -> &'static str {
    match layouts {
        LayoutVariant::Default => DECK_DEFAULT_MD,
        LayoutVariant::Split => DECK_SPLIT_MD,
        LayoutVariant::Cover => DECK_COVER_MD,
    }
}

fn base_css(layouts: LayoutVariant, theme: ThemeVariant) -> String {
    let mut css = String::new();
    css.push_str(BASE_CSS_HEADER);
    css.push_str(crate::BUILTIN_BASE_CSS);

    match layouts {
        LayoutVariant::Default => {}
        LayoutVariant::Split => append_css_block(&mut css, TWO_COLUMN_CSS),
        LayoutVariant::Cover => append_css_block(&mut css, COVER_CSS),
    }

    if matches!(theme, ThemeVariant::Dark) {
        append_css_block(&mut css, DARK_CSS);
        match layouts {
            LayoutVariant::Default => {}
            LayoutVariant::Split => append_css_block(&mut css, DARK_TWO_COLUMN_CSS),
            LayoutVariant::Cover => append_css_block(&mut css, DARK_COVER_CSS),
        }
    }

    css
}

fn append_css_block(css: &mut String, block: &str) {
    if !css.ends_with('\n') {
        css.push('\n');
    }
    css.push('\n');
    css.push_str(block.trim_end());
    css.push('\n');
}

fn ensure_target_dir(target: &Path, force: bool) -> miette::Result<()> {
    if !target.exists() {
        fs::create_dir_all(target).map_err(|err| {
            miette::miette!(
                "failed to create target directory {}\nhelp: check the path and parent directory permissions\ncaused by: {err}",
                target.display()
            )
        })?;
        return Ok(());
    }

    let metadata = fs::metadata(target).map_err(|err| {
        miette::miette!(
            "failed to inspect target path {}\nhelp: check filesystem permissions\ncaused by: {err}",
            target.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(miette::miette!(
            "target path exists and is not a directory: {}\nhelp: choose a directory path for `peitho new`",
            target.display()
        ));
    }

    if !force && directory_has_entries(target)? {
        return Err(miette::miette!(
            "target directory is not empty: {}\nhelp: pass --force to overwrite scaffold files in this directory, or choose a fresh directory",
            target.display()
        ));
    }

    Ok(())
}

fn directory_has_entries(target: &Path) -> miette::Result<bool> {
    let mut entries = fs::read_dir(target).map_err(|err| {
        miette::miette!(
            "failed to read target directory {}\nhelp: check directory permissions\ncaused by: {err}",
            target.display()
        )
    })?;
    match entries.next() {
        Some(Ok(_entry)) => Ok(true),
        Some(Err(err)) => Err(miette::miette!(
            "failed to read entry in target directory {}\nhelp: check directory permissions\ncaused by: {err}",
            target.display()
        )),
        None => Ok(false),
    }
}

fn write_scaffold_file(target: &Path, file: &ScaffoldFile) -> miette::Result<()> {
    let path = target.join(&file.relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            miette::miette!(
                "failed to create scaffold directory {}\nhelp: check filesystem permissions\ncaused by: {err}",
                parent.display()
            )
        })?;
    }
    fs::write(&path, &file.content).map_err(|err| {
        miette::miette!(
            "failed to write scaffold file {}\nhelp: fix the cause, then re-run with --force to overwrite the partial scaffold (or remove the directory and retry)\ncaused by: {err}",
            path.display()
        )
    })
}

fn print_success(
    stdout: &mut dyn Write,
    target: &Path,
    files: &[ScaffoldFile],
) -> miette::Result<()> {
    writeln!(stdout, "generated peitho deck in {}", target.display()).into_diagnostic()?;
    writeln!(stdout, "files:").into_diagnostic()?;
    for file in files {
        writeln!(stdout, "  {}", file.relative_path.display()).into_diagnostic()?;
    }
    let next = if target == Path::new(".") {
        "peitho preview".to_owned()
    } else {
        format!("cd {} && peitho preview", shell_single_quoted(target))
    };
    writeln!(stdout, "next: {next}").into_diagnostic()?;
    Ok(())
}

fn shell_single_quoted(path: &Path) -> String {
    let mut quoted = String::from("'");
    for ch in path.display().to_string().chars() {
        if ch == '\'' {
            quoted.push_str(r#"'\''"#);
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn paths(files: &[ScaffoldFile]) -> Vec<&Path> {
        files
            .iter()
            .map(|file| file.relative_path.as_path())
            .collect()
    }

    fn content<'a>(files: &'a [ScaffoldFile], path: &str) -> &'a str {
        files
            .iter()
            .find(|file| file.relative_path == Path::new(path))
            .unwrap_or_else(|| panic!("missing scaffold file: {path}"))
            .content
            .as_str()
    }

    fn default_options(target: PathBuf) -> NewOptions {
        NewOptions {
            target,
            layouts: LayoutVariant::Default,
            theme: ThemeVariant::Light,
            force: false,
        }
    }

    #[test]
    fn default_plan_has_the_complete_minimum_tree() {
        let files = plan(LayoutVariant::Default, ThemeVariant::Light);

        assert_eq!(
            paths(&files),
            vec![
                Path::new("deck.md"),
                Path::new("layouts/title-body-code.html"),
                Path::new("css/base.css"),
                Path::new(".gitignore"),
            ]
        );
        assert_eq!(
            content(&files, "layouts/title-body-code.html"),
            crate::BUILTIN_LAYOUT_HTML
        );
        assert!(content(&files, "css/base.css").contains(crate::BUILTIN_BASE_CSS));
        assert!(!content(&files, "deck.md").contains("\nlayouts:"));
        assert!(!content(&files, "deck.md").contains("\ncss:"));
    }

    #[test]
    fn split_and_cover_plans_add_only_their_extra_layout() {
        let split = plan(LayoutVariant::Split, ThemeVariant::Light);
        let cover = plan(LayoutVariant::Cover, ThemeVariant::Light);

        assert_eq!(
            paths(&split),
            vec![
                Path::new("deck.md"),
                Path::new("layouts/title-body-code.html"),
                Path::new("layouts/two-column.html"),
                Path::new("css/base.css"),
                Path::new(".gitignore"),
            ]
        );
        assert_eq!(
            paths(&cover),
            vec![
                Path::new("deck.md"),
                Path::new("layouts/title-body-code.html"),
                Path::new("layouts/cover.html"),
                Path::new("css/base.css"),
                Path::new(".gitignore"),
            ]
        );
    }

    #[test]
    fn dark_theme_appends_overrides_after_the_embedded_base_css() {
        let light = plan(LayoutVariant::Default, ThemeVariant::Light);
        let dark = plan(LayoutVariant::Default, ThemeVariant::Dark);

        let light_css = content(&light, "css/base.css");
        let dark_css = content(&dark, "css/base.css");

        assert!(dark_css.starts_with(light_css));
        assert!(dark_css.contains("Dark scaffold theme overrides"));
        assert!(
            !dark_css.contains(".two-column"),
            "default dark CSS must not reference split-only selectors:\n{dark_css}"
        );
        assert!(
            !dark_css.contains(".cover"),
            "default dark CSS must not reference cover-only selectors:\n{dark_css}"
        );

        let split = plan(LayoutVariant::Split, ThemeVariant::Dark);
        let split_css = content(&split, "css/base.css");
        assert!(split_css.contains(DARK_TWO_COLUMN_CSS.trim_end()));
        assert!(
            !split_css.contains(".cover"),
            "split dark CSS must not reference cover-only selectors:\n{split_css}"
        );

        let cover = plan(LayoutVariant::Cover, ThemeVariant::Dark);
        let cover_css = content(&cover, "css/base.css");
        assert!(cover_css.contains(DARK_COVER_CSS.trim_end()));
        assert!(
            !cover_css.contains(".two-column"),
            "cover dark CSS must not reference split-only selectors:\n{cover_css}"
        );
    }

    #[test]
    fn run_creates_missing_target_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested").join("deck");
        let mut stdout = Vec::new();

        run(default_options(target.clone()), &mut stdout).unwrap();

        assert!(target.join("deck.md").is_file());
        assert!(target.join("layouts/title-body-code.html").is_file());
        assert!(target.join("css/base.css").is_file());
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("peitho preview"));
    }

    #[test]
    fn run_quotes_next_step_path_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("it's a deck with space");
        let mut stdout = Vec::new();

        run(default_options(target.clone()), &mut stdout).unwrap();

        let stdout = String::from_utf8(stdout).unwrap();
        let quoted_target = target.display().to_string().replace('\'', r#"'\''"#);
        assert!(
            stdout.contains(&format!("next: cd '{quoted_target}' && peitho preview")),
            "actual stdout: {stdout}"
        );
    }

    #[test]
    fn print_success_omits_cd_for_current_directory() {
        let mut stdout = Vec::new();

        print_success(&mut stdout, Path::new("."), &[]).unwrap();

        let stdout = String::from_utf8(stdout).unwrap();
        assert!(stdout.contains("next: peitho preview\n"));
        assert!(!stdout.contains("next: cd "), "actual stdout: {stdout}");
    }

    #[test]
    fn run_allows_empty_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deck");
        fs::create_dir(&target).unwrap();
        let mut stdout = Vec::new();

        run(default_options(target.clone()), &mut stdout).unwrap();

        assert!(target.join("deck.md").is_file());
    }

    #[test]
    fn run_refuses_non_empty_directory_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deck");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("notes.txt"), "keep me").unwrap();
        let mut stdout = Vec::new();

        let err = run(default_options(target), &mut stdout).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("--force"), "actual error: {message}");
        assert!(stdout.is_empty());
    }

    #[test]
    fn run_rejects_target_path_that_is_not_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deck.md");
        fs::write(&target, "not a directory").unwrap();
        let mut stdout = Vec::new();

        let err = run(default_options(target), &mut stdout).unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("target path exists and is not a directory"),
            "actual error: {message}"
        );
        assert!(
            message.contains("help: choose a directory path for `peitho new`"),
            "actual error: {message}"
        );
        assert!(stdout.is_empty());
    }

    #[test]
    fn force_overwrites_scaffold_files_and_leaves_other_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deck");
        fs::create_dir_all(target.join("layouts")).unwrap();
        fs::write(target.join("deck.md"), "old deck").unwrap();
        fs::write(target.join("keep.txt"), "keep me").unwrap();
        let mut options = default_options(target.clone());
        options.force = true;
        let mut stdout = Vec::new();

        run(options, &mut stdout).unwrap();

        assert_ne!(
            fs::read_to_string(target.join("deck.md")).unwrap(),
            "old deck"
        );
        assert_eq!(
            fs::read_to_string(target.join("keep.txt")).unwrap(),
            "keep me"
        );
    }
}
