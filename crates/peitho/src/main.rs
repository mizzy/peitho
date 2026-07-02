use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use miette::IntoDiagnostic;

#[derive(Debug, Parser)]
#[command(name = "peitho")]
#[command(about = "Build HTML-native presentations from Markdown")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Build {
        input: PathBuf,
        #[arg(long, default_value = "templates/title-body-code.html")]
        template: PathBuf,
        #[arg(long, default_value = "themes/base.css")]
        base_css: PathBuf,
        #[arg(long, default_value = "themes/overrides.css")]
        overrides_css: PathBuf,
        #[arg(long, default_value = "dist")]
        out: PathBuf,
    },
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            input,
            template,
            base_css,
            overrides_css,
            out,
        } => build(&input, &template, &base_css, &overrides_css, &out),
    }
}

fn build(
    input: &Path,
    template_path: &Path,
    base_path: &Path,
    overrides_path: &Path,
    out: &Path,
) -> miette::Result<()> {
    let markdown = fs::read_to_string(input).into_diagnostic()?;
    let template_html = fs::read_to_string(template_path).into_diagnostic()?;
    let base_css = fs::read_to_string(base_path).into_diagnostic()?;
    let overrides_css = fs::read_to_string(overrides_path).into_diagnostic()?;

    let template_name = template_name(template_path);
    let template = core(peitho_core::parse_template(template_name, &template_html))?;
    let parsed = core(peitho_core::parse_markdown(&markdown))?;
    let mapped = core(peitho_core::map_by_convention(parsed, &template))?;
    let checked = core(peitho_core::check_deck(mapped, &template))?;
    let slide_count = checked.slide_count();
    let manifest = peitho_core::build_manifest(&checked);
    let manifest_json = core(peitho_core::manifest_json(&manifest))?;
    let css = core(peitho_core::build_theme_css(
        &base_css,
        &overrides_css,
        checked.slide_keys(),
        &template,
    ))?;
    let rendered = core(peitho_core::render_deck(checked, &template))?;

    fs::create_dir_all(out).into_diagnostic()?;
    fs::write(out.join("peitho.css"), css).into_diagnostic()?;
    write_slide_fragments(out, &rendered)?;
    fs::write(out.join("manifest.json"), manifest_json).into_diagnostic()?;
    fs::write(
        out.join("index.html"),
        peitho_core::render_distribution_index(),
    )
    .into_diagnostic()?;
    println!("built {} slide(s) into {}", slide_count, out.display());
    Ok(())
}

fn write_slide_fragments(
    out: &Path,
    rendered: &peitho_core::Deck<peitho_core::Rendered>,
) -> miette::Result<()> {
    let slides_dir = out.join("slides");
    fs::create_dir_all(&slides_dir).into_diagnostic()?;
    for slide in rendered.slides() {
        fs::write(out.join(slide.src()), slide.html()).into_diagnostic()?;
    }
    Ok(())
}

fn core<T>(result: peitho_core::Result<T>) -> miette::Result<T> {
    result.map_err(|err| miette::miette!("{err}"))
}

fn template_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}
