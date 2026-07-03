use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use clap::{Parser, Subcommand};
use miette::IntoDiagnostic;
use notify::{PollWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer_opt, Config as DebounceConfig, DebounceEventResult};

use peitho::{browser, displays, server};

struct BuildArtifacts {
    slide_count: usize,
    rendered: peitho_core::Deck<peitho_core::Rendered>,
    manifest_json: String,
    css: String,
}

#[derive(Debug, Clone)]
struct BuildOptions {
    input: PathBuf,
    template: PathBuf,
    base_css: PathBuf,
    overrides_css: PathBuf,
    out: PathBuf,
}

impl BuildOptions {
    fn watch_paths(&self) -> [PathBuf; 4] {
        [
            self.input.clone(),
            self.template.clone(),
            self.base_css.clone(),
            self.overrides_css.clone(),
        ]
    }

    fn watch_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for path in self.watch_paths() {
            let dir = parent_dir_for_watch(&path);
            if !dirs.iter().any(|existing| same_watch_path(existing, &dir)) {
                dirs.push(dir);
            }
        }
        dirs
    }
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
    no_presenter: bool,
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
        #[arg(long)]
        watch: bool,
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
        #[arg(long)]
        no_presenter: bool,
    },
    Publish {
        #[arg(long, default_value = "dist")]
        dist: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
}

const PRESENT_CACHE: &str = ".peitho/present-cache";
const PRESENTATION_ONLY_DIST_FILES: &[&str] =
    &["present.html", "presenter.html", "notes.json", "shell.js"];

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            input,
            template,
            base_css,
            overrides_css,
            out,
            watch,
        } => {
            let options = BuildOptions {
                input,
                template,
                base_css,
                overrides_css,
                out,
            };
            if watch {
                watch_build(options)
            } else {
                build(&options)
            }
        }
        Command::Present {
            input,
            template,
            base_css,
            overrides_css,
            shell,
            port,
            no_open,
            no_serve,
            no_presenter,
        } => present(PresentOptions {
            input,
            template,
            base_css,
            overrides_css,
            shell,
            port,
            no_open,
            no_serve,
            no_presenter,
        }),
        Command::Publish { dist, command } => {
            let code = publish(&dist, &command)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
    }
}

fn build(options: &BuildOptions) -> miette::Result<()> {
    let artifacts = build_artifacts(
        &options.input,
        &options.template,
        &options.base_css,
        &options.overrides_css,
    )?;
    emit_distribution(&options.out, &artifacts)?;
    println!(
        "built {} slide(s) into {}",
        artifacts.slide_count,
        options.out.display()
    );
    Ok(())
}

fn rebuild_once_for_watch(
    options: &BuildOptions,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    match build_artifacts(
        &options.input,
        &options.template,
        &options.base_css,
        &options.overrides_css,
    ) {
        Ok(artifacts) => match emit_distribution(&options.out, &artifacts) {
            Ok(()) => {
                writeln!(
                    stdout,
                    "built {} slide(s) into {}",
                    artifacts.slide_count,
                    options.out.display()
                )
                .into_diagnostic()?;
                stdout.flush().into_diagnostic()?;
            }
            Err(err) => {
                writeln!(stderr, "build failed: {err}").into_diagnostic()?;
                stderr.flush().into_diagnostic()?;
            }
        },
        Err(err) => {
            writeln!(stderr, "build failed: {err}").into_diagnostic()?;
            stderr.flush().into_diagnostic()?;
        }
    }

    Ok(())
}

fn handle_watch_paths(
    options: &BuildOptions,
    changed_paths: &[PathBuf],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    let watched = options.watch_paths();
    let relevant = changed_paths
        .iter()
        .any(|changed| watched.iter().any(|path| same_watch_path(path, changed)));

    if relevant {
        rebuild_once_for_watch(options, stdout, stderr)?;
    }

    Ok(())
}

fn watch_build(options: BuildOptions) -> miette::Result<()> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let notify_config = notify::Config::default().with_poll_interval(Duration::from_millis(200));
    let debounce_config = DebounceConfig::default()
        .with_timeout(Duration::from_millis(200))
        .with_notify_config(notify_config);
    let mut debouncer =
        new_debouncer_opt::<_, PollWatcher>(debounce_config, tx).map_err(|err| {
            miette::miette!(
                "failed to start file watcher\nhelp: check file watcher permissions\ncaused by: {err}"
            )
        })?;

    for dir in options.watch_dirs() {
        debouncer
            .watcher()
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|err| {
                miette::miette!(
                    "failed to watch {}\nhelp: verify the parent directory exists before starting --watch\ncaused by: {err}",
                    dir.display()
                )
            })?;
    }

    println!("watching parent directories for markdown, template, base css, and overrides css");
    rebuild_once_for_watch(&options, &mut std::io::stdout(), &mut std::io::stderr())?;

    for result in rx {
        match result {
            Ok(events) => {
                let paths = events
                    .into_iter()
                    .map(|event| event.path)
                    .collect::<Vec<_>>();
                handle_watch_paths(
                    &options,
                    &paths,
                    &mut std::io::stdout(),
                    &mut std::io::stderr(),
                )?;
            }
            Err(err) => {
                eprintln!("watch error: {err}");
            }
        }
    }

    Ok(())
}

fn parent_dir_for_watch(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn same_watch_path(left: &Path, right: &Path) -> bool {
    left == right
        || match (fs::canonicalize(left), fs::canonicalize(right)) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
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

struct PublishDistribution {
    dist: PathBuf,
}

fn publish(dist: &Path, command: &[OsString]) -> miette::Result<i32> {
    let distribution = validate_publish_dist(dist)?;
    if command.is_empty() {
        return Err(miette::miette!(
            "publish command is missing\nhelp: deployment is delegated to IaC or CI; example: peitho publish -- aws s3 sync dist/ s3://bucket"
        ));
    }

    run_publish_command(&distribution.dist, command)
}

fn run_publish_command(dist: &Path, command: &[OsString]) -> miette::Result<i32> {
    let executable = &command[0];
    let status = std::process::Command::new(executable)
        .args(&command[1..])
        .env("PEITHO_DIST", dist)
        .status()
        .map_err(|err| {
            miette::miette!(
                "failed to run publish command: {}\nhelp: check that the command exists and is executable\ncaused by: {err}",
                executable.to_string_lossy()
            )
        })?;

    Ok(status.code().unwrap_or(1))
}

fn validate_publish_dist(dist: &Path) -> miette::Result<PublishDistribution> {
    require_dist_file(dist, "index.html")?;
    require_dist_file(dist, "manifest.json")?;
    require_dist_file(dist, "peitho.css")?;
    require_slides_dir_with_files(dist)?;
    reject_presentation_only_files(dist)?;

    read_publish_manifest(dist)?;
    let canonical = fs::canonicalize(dist).map_err(|err| {
        miette::miette!(
            "distribution is incomplete: failed to resolve {}\nhelp: run `peitho build` first\ncaused by: {err}",
            dist.display()
        )
    })?;

    Ok(PublishDistribution { dist: canonical })
}

fn reject_presentation_only_files(dist: &Path) -> miette::Result<()> {
    for file in PRESENTATION_ONLY_DIST_FILES {
        if dist.join(file).exists() {
            return Err(miette::miette!(
                "distribution contains presentation-only file: {file}\nhelp: remove presentation artifacts or run `peitho build` again"
            ));
        }
    }
    Ok(())
}

fn read_publish_manifest(dist: &Path) -> miette::Result<peitho_core::Manifest> {
    let path = dist.join("manifest.json");
    let json = fs::read_to_string(&path).map_err(|err| {
        miette::miette!(
            "failed to read manifest.json\nhelp: run `peitho build` first\ncaused by: {err}"
        )
    })?;

    let manifest: peitho_core::Manifest = serde_json::from_str(&json).map_err(|err| {
        miette::miette!(
            "failed to parse manifest.json\nhelp: run `peitho build` first\ncaused by: {err}"
        )
    })?;

    validate_manifest_slide_refs(dist, &manifest)?;
    Ok(manifest)
}

fn validate_manifest_slide_refs(
    dist: &Path,
    manifest: &peitho_core::Manifest,
) -> miette::Result<()> {
    if manifest.slide_count() != manifest.slides().len() {
        return Err(miette::miette!(
            "manifest slideCount does not match slides length\nhelp: run `peitho build` first"
        ));
    }

    if manifest.slides().is_empty() || manifest.slide_count() == 0 {
        return Err(miette::miette!(
            "manifest has no slides\nhelp: run `peitho build` first"
        ));
    }

    for slide in manifest.slides() {
        let src = slide.src();
        let path = Path::new(src);
        let invalid_component = path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        });
        if src.is_empty() || path.is_absolute() || invalid_component {
            return Err(miette::miette!(
                "manifest contains invalid slide src: {src}\nhelp: slide src must be a relative path inside dist/"
            ));
        }

        if !dist.join(path).is_file() {
            return Err(miette::miette!(
                "manifest references missing slide fragment: {src}\nhelp: run `peitho build` first"
            ));
        }
    }

    Ok(())
}

fn require_dist_file(dist: &Path, file: &str) -> miette::Result<()> {
    let path = dist.join(file);
    if path.is_file() {
        return Ok(());
    }

    Err(miette::miette!(
        "distribution is incomplete: missing {file}\nhelp: run `peitho build` first"
    ))
}

fn require_slides_dir_with_files(dist: &Path) -> miette::Result<()> {
    let slides = dist.join("slides");
    if !slides.is_dir() {
        return Err(miette::miette!(
            "distribution is incomplete: missing slides/\nhelp: run `peitho build` first"
        ));
    }

    let mut has_file = false;
    for entry in fs::read_dir(&slides).into_diagnostic()? {
        if entry
            .into_diagnostic()?
            .file_type()
            .into_diagnostic()?
            .is_file()
        {
            has_file = true;
            break;
        }
    }
    if has_file {
        Ok(())
    } else {
        Err(miette::miette!(
            "distribution is incomplete: slides/ must contain at least one file\nhelp: run `peitho build` first"
        ))
    }
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
    let presenter_url = browser::presenter_url(&url);
    println!("serving presentation at {url}");
    std::io::stdout().flush().into_diagnostic()?;
    if !options.no_open {
        browser::open_browser_with_request(
            browser::BrowserOpenRequest {
                slides_url: &url,
                presenter_url: &presenter_url,
                no_presenter: options.no_presenter,
            },
            displays::detect_presentation_layout(),
        );
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
    fs::write(
        cache.join("presenter.html"),
        peitho_core::render_presenter_index(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_dependency_types_are_available() {
        fn accepts_recursive_mode(_mode: notify::RecursiveMode) {}

        accepts_recursive_mode(notify::RecursiveMode::NonRecursive);
        let result: notify_debouncer_mini::DebounceEventResult = Ok(Vec::new());

        assert!(matches!(result, Ok(events) if events.is_empty()));
    }

    #[test]
    fn build_options_lists_watched_input_paths() {
        let options = BuildOptions {
            input: PathBuf::from("deck.md"),
            template: PathBuf::from("template.html"),
            base_css: PathBuf::from("base.css"),
            overrides_css: PathBuf::from("overrides.css"),
            out: PathBuf::from("dist"),
        };

        assert_eq!(
            options.watch_paths(),
            [
                PathBuf::from("deck.md"),
                PathBuf::from("template.html"),
                PathBuf::from("base.css"),
                PathBuf::from("overrides.css"),
            ]
        );
    }

    #[test]
    fn build_options_deduplicates_watch_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let options = BuildOptions {
            input: dir.path().join("deck.md"),
            template: dir.path().join("title-body-code.html"),
            base_css: dir.path().join("base.css"),
            overrides_css: dir.path().join("overrides.css"),
            out: dir.path().join("dist"),
        };

        assert_eq!(options.watch_dirs(), vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn watch_rebuild_once_writes_distribution_and_success_line() {
        let fixture = WatchFixture::new("# Intro\n\nBody\n");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();

        assert!(stderr.is_empty());
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
        assert!(fixture.options.out.join("manifest.json").exists());
        assert!(fixture.options.out.join("slides/000-intro.html").exists());
    }

    #[test]
    fn watch_rebuild_once_reports_failure_without_returning_error() {
        let fixture =
            WatchFixture::new("# Intro\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();

        assert!(stdout.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("build failed:"));
        assert!(stderr.contains("slot 'code' got 2 item(s)"));
        assert!(
            stderr.contains("help: use a layout with more code capacity or remove one code block")
        );
    }

    #[test]
    fn watch_rebuild_once_reports_emit_failure_without_returning_error() {
        let fixture = WatchFixture::new("# Intro\n");
        fs::write(&fixture.options.out, "not a directory").unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();

        assert!(stdout.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("build failed:"));
    }

    #[test]
    fn watch_path_handler_rebuilds_after_markdown_change() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
        fs::write(&fixture.options.input, "# Intro\n\n---\n# Details\n").unwrap();

        handle_watch_paths(
            &fixture.options,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        let manifest = fs::read_to_string(fixture.options.out.join("manifest.json")).unwrap();
        assert!(manifest.contains(r#""slideCount": 2"#));
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 2 slide(s)"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn watch_path_handler_ignores_unwatched_file() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let unrelated = fixture._dir.path().join("outside").join("ignored.txt");

        handle_watch_paths(&fixture.options, &[unrelated], &mut stdout, &mut stderr).unwrap();

        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        assert!(!fixture.options.out.join("manifest.json").exists());
    }

    #[test]
    fn watch_path_handler_ignores_output_directory_event_in_watched_parent() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
        stdout.clear();
        stderr.clear();

        handle_watch_paths(
            &fixture.options,
            std::slice::from_ref(&fixture.options.out),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    #[test]
    fn watch_path_handler_rebuilds_after_atomic_save_final_path() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let temp = fixture._dir.path().join("deck-new.md");

        fs::write(&temp, "# Atomic one\n\n---\n# Atomic two\n").unwrap();
        fs::rename(&temp, &fixture.options.input).unwrap();

        handle_watch_paths(
            &fixture.options,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        let manifest = fs::read_to_string(fixture.options.out.join("manifest.json")).unwrap();
        assert!(manifest.contains(r#""slideCount": 2"#));
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 2 slide(s)"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn watch_build_function_is_available_for_cli_dispatch() {
        let _watch: fn(BuildOptions) -> miette::Result<()> = watch_build;
    }

    #[test]
    fn build_command_accepts_watch_flag() {
        let cli = Cli::parse_from(["peitho", "build", "deck.md", "--watch"]);

        match cli.command {
            Command::Build { input, watch, .. } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert!(watch);
            }
            Command::Present { .. } | Command::Publish { .. } => {
                panic!("expected build command");
            }
        }
    }

    struct WatchFixture {
        _dir: tempfile::TempDir,
        options: BuildOptions,
    }

    impl WatchFixture {
        fn new(markdown: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let deck = dir.path().join("deck.md");
            let template = dir.path().join("title-body-code.html");
            let base = dir.path().join("base.css");
            let overrides = dir.path().join("overrides.css");
            let out = dir.path().join("dist");

            fs::write(&deck, markdown).unwrap();
            fs::write(
                &template,
                r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#,
            )
            .unwrap();
            fs::write(&base, ".slot-title { font-weight: 700; }\n").unwrap();
            fs::write(&overrides, "").unwrap();

            Self {
                _dir: dir,
                options: BuildOptions {
                    input: deck,
                    template,
                    base_css: base,
                    overrides_css: overrides,
                    out,
                },
            }
        }
    }
}
