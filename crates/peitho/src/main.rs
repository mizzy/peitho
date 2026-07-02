use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use miette::IntoDiagnostic;

use peitho::server;

struct BuildArtifacts {
    slide_count: usize,
    rendered: peitho_core::Deck<peitho_core::Rendered>,
    manifest_json: String,
    css: String,
}

struct PresentOptions {
    input: PathBuf,
    template: PathBuf,
    base_css: PathBuf,
    overrides_css: PathBuf,
    shell: PathBuf,
    port: u16,
    no_open: bool,
    no_serve: bool,
}

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
    Present {
        input: PathBuf,
        #[arg(long, default_value = "templates/title-body-code.html")]
        template: PathBuf,
        #[arg(long, default_value = "themes/base.css")]
        base_css: PathBuf,
        #[arg(long, default_value = "themes/overrides.css")]
        overrides_css: PathBuf,
        #[arg(long, default_value = "packages/peitho-present/dist/shell.js")]
        shell: PathBuf,
        #[arg(long, default_value_t = 0)]
        port: u16,
        #[arg(long)]
        no_open: bool,
        #[arg(long)]
        no_serve: bool,
    },
}

const PRESENT_CACHE: &str = ".peitho/present-cache";

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
        Command::Present {
            input,
            template,
            base_css,
            overrides_css,
            shell,
            port,
            no_open,
            no_serve,
        } => present(PresentOptions {
            input,
            template,
            base_css,
            overrides_css,
            shell,
            port,
            no_open,
            no_serve,
        }),
    }
}

fn build(
    input: &Path,
    template_path: &Path,
    base_path: &Path,
    overrides_path: &Path,
    out: &Path,
) -> miette::Result<()> {
    let artifacts = build_artifacts(input, template_path, base_path, overrides_path)?;
    emit_distribution(out, &artifacts)?;
    println!(
        "built {} slide(s) into {}",
        artifacts.slide_count,
        out.display()
    );
    Ok(())
}

fn build_artifacts(
    input: &Path,
    template_path: &Path,
    base_path: &Path,
    overrides_path: &Path,
) -> miette::Result<BuildArtifacts> {
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

    Ok(BuildArtifacts {
        slide_count,
        rendered,
        manifest_json,
        css,
    })
}

fn emit_distribution(out: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    fs::create_dir_all(out).into_diagnostic()?;
    fs::write(out.join("peitho.css"), &artifacts.css).into_diagnostic()?;
    write_slide_fragments(out, &artifacts.rendered)?;
    fs::write(out.join("manifest.json"), &artifacts.manifest_json).into_diagnostic()?;
    fs::write(
        out.join("index.html"),
        peitho_core::render_distribution_index(),
    )
    .into_diagnostic()?;
    Ok(())
}

fn present(options: PresentOptions) -> miette::Result<()> {
    let cache = PathBuf::from(PRESENT_CACHE);
    if cache.exists() {
        fs::remove_dir_all(&cache).into_diagnostic()?;
    }
    fs::create_dir_all(&cache).into_diagnostic()?;

    let artifacts = build_artifacts(
        &options.input,
        &options.template,
        &options.base_css,
        &options.overrides_css,
    )?;
    emit_present_cache(&cache, &artifacts, &options.shell)?;
    if options.no_serve {
        println!("generated present cache at {}", cache.display());
        return Ok(());
    }

    let server = server::PresentServer::bind(cache, options.port)?;
    let url = server.url();
    println!("serving presentation at {url}");
    std::io::stdout().flush().into_diagnostic()?;
    if !options.no_open {
        open_browser(&url);
    }
    server.serve_forever()
}

fn emit_present_cache(
    cache: &Path,
    artifacts: &BuildArtifacts,
    shell: &Path,
) -> miette::Result<()> {
    ensure_shell_bundle(shell)?;
    fs::write(cache.join("peitho.css"), &artifacts.css).into_diagnostic()?;
    write_slide_fragments(cache, &artifacts.rendered)?;
    fs::write(cache.join("manifest.json"), &artifacts.manifest_json).into_diagnostic()?;
    fs::write(
        cache.join("notes.json"),
        core(peitho_core::notes_json(&peitho_core::Notes::empty()))?,
    )
    .into_diagnostic()?;
    fs::write(
        cache.join("present.html"),
        peitho_core::render_present_index(),
    )
    .into_diagnostic()?;
    fs::copy(shell, cache.join("shell.js")).into_diagnostic()?;
    Ok(())
}

fn ensure_shell_bundle(shell: &Path) -> miette::Result<()> {
    if shell.exists() {
        return Ok(());
    }
    Err(miette::miette!(
        "shell bundle not found: {}\nhelp: run `cd packages/peitho-present && npm run build` or pass --shell <path>",
        shell.display()
    ))
}

fn write_slide_fragments(
    out: &Path,
    rendered: &peitho_core::Deck<peitho_core::Rendered>,
) -> miette::Result<()> {
    let slides_dir = out.join("slides");
    if slides_dir.exists() {
        fs::remove_dir_all(&slides_dir).into_diagnostic()?;
    }
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

fn browser_command() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("open")
    } else if cfg!(target_os = "linux") {
        Some("xdg-open")
    } else {
        None
    }
}

fn open_browser(url: &str) {
    let Some(command) = browser_command() else {
        eprintln!("warning: browser auto-open is not supported on this platform");
        return;
    };
    if let Err(err) = std::process::Command::new(command).arg(url).spawn() {
        eprintln!("warning: failed to open browser with {command}: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_command_matches_supported_platforms() {
        let command = browser_command();
        if cfg!(target_os = "macos") {
            assert_eq!(command, Some("open"));
        } else if cfg!(target_os = "linux") {
            assert_eq!(command, Some("xdg-open"));
        } else {
            assert_eq!(command, None);
        }
    }
}
