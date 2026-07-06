use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use clap::{Parser, Subcommand};
use miette::IntoDiagnostic;
use notify::{PollWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer_opt, Config as DebounceConfig, DebounceEventResult};
use sha2::{Digest, Sha256};

mod asset_resolution;

use asset_resolution::{resolve_assets, ResolvedAssets};
use peitho::{browser, server};

struct BuildArtifacts {
    slide_count: usize,
    rendered: peitho_core::Deck<peitho_core::Rendered>,
    manifest_json: String,
    css: String,
    image_assets: Vec<peitho_core::ResolvedImageAsset>,
}

#[derive(Debug, Clone)]
struct BuildOptions {
    input: PathBuf,
    out: PathBuf,
}

#[derive(Debug, Clone)]
struct WatchRoot {
    path: PathBuf,
    ext: &'static str,
}

#[derive(Debug, Clone)]
struct WatchTargets {
    roots: Vec<WatchRoot>,
    assets: ResolvedAssets,
}

impl WatchTargets {
    /// The deck file plus the resolved asset paths. Each asset path may be a
    /// single file or a directory whose matching extension files are watched.
    fn new(input: PathBuf, assets: ResolvedAssets) -> Self {
        let mut roots = vec![WatchRoot {
            path: input,
            ext: "md",
        }];
        if let Some(path) = &assets.layouts {
            roots.push(WatchRoot {
                path: path.clone(),
                ext: "html",
            });
        }
        if let Some(path) = &assets.css {
            roots.push(WatchRoot {
                path: path.clone(),
                ext: "css",
            });
        }
        if let Some(path) = &assets.syntaxes {
            roots.push(WatchRoot {
                path: path.clone(),
                ext: "sublime-syntax",
            });
        }
        Self { roots, assets }
    }

    fn is_relevant_change(&self, changed: &Path) -> bool {
        self.roots.iter().any(|root| {
            if same_watch_path(&root.path, changed) {
                return true;
            }
            root.path.is_dir()
                && changed.extension().and_then(|e| e.to_str()) == Some(root.ext)
                && changed
                    .parent()
                    .is_some_and(|parent| same_watch_path(&root.path, parent))
        })
    }

    fn watch_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for root in &self.roots {
            let dir = if root.path.is_dir() {
                root.path.clone()
            } else {
                parent_dir_for_watch(&root.path)
            };
            if !dirs.iter().any(|existing| same_watch_path(existing, &dir)) {
                dirs.push(dir);
            }
        }
        dirs
    }
}

trait WatchController {
    fn watch_dir(&mut self, dir: &Path) -> miette::Result<()>;
    fn unwatch_dir(&mut self, dir: &Path) -> miette::Result<()>;
}

struct NotifyWatchController<'a> {
    watcher: &'a mut dyn notify::Watcher,
}

impl<'a> NotifyWatchController<'a> {
    fn new(watcher: &'a mut dyn notify::Watcher) -> Self {
        Self { watcher }
    }
}

impl WatchController for NotifyWatchController<'_> {
    fn watch_dir(&mut self, dir: &Path) -> miette::Result<()> {
        self.watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|err| {
                miette::miette!(
                    "failed to watch {}\nhelp: verify the parent directory exists before starting --watch\ncaused by: {err}",
                    dir.display()
                )
            })
    }

    fn unwatch_dir(&mut self, dir: &Path) -> miette::Result<()> {
        self.watcher.unwatch(dir).map_err(|err| {
            miette::miette!(
                "failed to stop watching {}\nhelp: restart --watch if the watcher state is stale\ncaused by: {err}",
                dir.display()
            )
        })
    }
}

struct PresentOptions {
    input: PathBuf,
    shell: Option<PathBuf>,
    port: u16,
    no_open: bool,
    no_serve: bool,
    no_presenter: bool,
    presenter_windowed: bool,
}

#[derive(Debug, Parser)]
#[command(name = "peitho")]
#[command(version)]
#[command(about = "Build HTML-native presentations from Markdown")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Build {
        input: PathBuf,
        #[arg(long, default_value = "dist")]
        out: PathBuf,
        #[arg(long)]
        watch: bool,
    },
    Present {
        input: PathBuf,
        #[arg(long, help = "shell bundle path (default: built-in present shell)")]
        shell: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        port: u16,
        #[arg(long)]
        no_open: bool,
        #[arg(long)]
        no_serve: bool,
        #[arg(long)]
        no_presenter: bool,
        #[arg(long)]
        presenter_windowed: bool,
    },
    Publish {
        #[arg(long, default_value = "dist")]
        dist: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
    Export {
        #[command(subcommand)]
        command: ExportCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    Pdf {
        input: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    Pptx {
        input: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

/// Built-in defaults compiled from the repository's own layout and theme,
/// so `peitho build deck.md` works outside the repository. `include_str!`
/// keeps the checked-in files as the single source: the binary embeds them
/// at compile time and cannot drift.
const BUILTIN_LAYOUT_NAME: &str = "title-body-code";
const BUILTIN_LAYOUT_HTML: &str = include_str!("../../../layouts/title-body-code.html");
const BUILTIN_BASE_CSS: &str = include_str!("../../../themes/base.css");
/// The committed esbuild bundle; CI rebuilds it and fails on drift, the same
/// discipline as the generated TS types in bindings/.
const BUILTIN_SHELL_JS: &str = include_str!("../../../packages/peitho-present/dist/shell.js");

const PRESENT_CACHE: &str = ".peitho/present-cache";
const PRESENTATION_ONLY_DIST_FILES: &[&str] =
    &["present.html", "presenter.html", "notes.json", "shell.js"];

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { input, out, watch } => {
            let options = BuildOptions { input, out };
            if watch {
                watch_build(options)
            } else {
                build(&options)
            }
        }
        Command::Present {
            input,
            shell,
            port,
            no_open,
            no_serve,
            no_presenter,
            presenter_windowed,
        } => present(PresentOptions {
            input,
            shell,
            port,
            no_open,
            no_serve,
            no_presenter,
            presenter_windowed,
        }),
        Command::Publish { dist, command } => {
            let code = publish(&dist, &command)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Command::Export { command } => match command {
            ExportCommand::Pdf { input, out } => export_pdf(input, out),
            ExportCommand::Pptx { input, out } => export_pptx(input, out),
        },
    }
}

fn build(options: &BuildOptions) -> miette::Result<()> {
    let artifacts = build_artifacts(&options.input)?;
    emit_distribution(&options.out, &artifacts)?;
    println!(
        "built {} slide(s) into {}",
        artifacts.slide_count,
        options.out.display()
    );
    Ok(())
}

fn export_pdf(input: PathBuf, out: Option<PathBuf>) -> miette::Result<()> {
    let artifacts = build_artifacts(&input)?;
    let out = out.unwrap_or_else(|| input.with_extension("pdf"));
    let tmp = tempfile::tempdir().into_diagnostic()?;
    emit_pdf_workspace(tmp.path(), &artifacts)?;
    let chrome = locate_chrome()?;
    run_chrome_print(&chrome, tmp.path(), &out)?;
    println!(
        "exported {} slide(s) to {}",
        artifacts.slide_count,
        out.display()
    );
    Ok(())
}

fn export_pptx(input: PathBuf, out: Option<PathBuf>) -> miette::Result<()> {
    let artifacts = build_artifacts(&input)?;
    let out = out.unwrap_or_else(|| input.with_extension("pptx"));
    let tmp = tempfile::tempdir().into_diagnostic()?;
    emit_measure_workspace(tmp.path(), &artifacts)?;
    let chrome = locate_chrome()?;
    let dumped_dom = run_chrome_dump_dom(&chrome, tmp.path())?;
    let measurement_json = extract_measure_json(&dumped_dom)?;
    let measured: peitho_core::MeasuredDeck =
        serde_json::from_str(&measurement_json).map_err(|err| {
            miette::miette!(
                "failed to parse measurement JSON\nhelp: rerun export; if this persists, inspect measure.html and the peitho-measure script payload\ncaused by: {err}"
            )
        })?;
    if measured.slides.len() != artifacts.rendered.slide_count() {
        return Err(miette::miette!(
            "measured slide count {} does not match rendered slide count {}\nhelp: rerun export so Chrome measures the same rendered deck",
            measured.slides.len(),
            artifacts.rendered.slide_count()
        ));
    }
    let pptx = core(peitho_core::build_pptx(
        &measured,
        &artifacts.rendered,
        &artifacts.image_assets,
    ))?;
    fs::write(&out, pptx).into_diagnostic()?;
    println!(
        "exported {} slide(s) to {}",
        artifacts.slide_count,
        out.display()
    );
    Ok(())
}

fn rebuild_once_for_watch(
    options: &BuildOptions,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    match build_artifacts(&options.input) {
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
    targets: &mut WatchTargets,
    watcher: &mut dyn WatchController,
    changed_paths: &[PathBuf],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    let relevant = changed_paths
        .iter()
        .any(|changed| targets.is_relevant_change(changed));

    if relevant {
        if changed_paths
            .iter()
            .any(|changed| same_watch_path(&options.input, changed))
        {
            refresh_watch_targets_after_deck_change(&options.input, targets, watcher, stderr)?;
        }
        rebuild_once_for_watch(options, stdout, stderr)?;
    }

    Ok(())
}

fn watch_build(options: BuildOptions) -> miette::Result<()> {
    let mut targets = resolve_watch_targets(&options.input)?;
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

    {
        let mut watcher = NotifyWatchController::new(debouncer.watcher());
        watch_all_dirs(&mut watcher, &targets.watch_dirs())?;
    }

    println!("watching deck and resolved asset paths");
    rebuild_once_for_watch(&options, &mut std::io::stdout(), &mut std::io::stderr())?;

    for result in rx {
        match result {
            Ok(events) => {
                let paths = events
                    .into_iter()
                    .map(|event| event.path)
                    .collect::<Vec<_>>();
                let mut watcher = NotifyWatchController::new(debouncer.watcher());
                handle_watch_paths(
                    &options,
                    &mut targets,
                    &mut watcher,
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

fn resolve_watch_targets(input: &Path) -> miette::Result<WatchTargets> {
    let assets = resolve_deck_assets(input)?;
    Ok(WatchTargets::new(input.to_path_buf(), assets))
}

fn resolve_deck_assets(input: &Path) -> miette::Result<ResolvedAssets> {
    let markdown = fs::read_to_string(input).into_diagnostic()?;
    let frontmatter = core(peitho_core::parse_frontmatter(&markdown))?;
    resolve_assets(input, &frontmatter)
}

fn refresh_watch_targets_after_deck_change(
    input: &Path,
    targets: &mut WatchTargets,
    watcher: &mut dyn WatchController,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    let current_assets = match resolve_deck_assets(input) {
        Ok(assets) => assets,
        Err(_) => {
            return Ok(());
        }
    };
    if targets.assets == current_assets {
        return Ok(());
    }

    let next_targets = WatchTargets::new(input.to_path_buf(), current_assets);
    update_watched_dirs(watcher, &targets.watch_dirs(), &next_targets.watch_dirs())?;
    *targets = next_targets;
    writeln!(
        stderr,
        "note: watching new asset paths from frontmatter: {}",
        describe_resolved_assets(&targets.assets)
    )
    .into_diagnostic()?;
    stderr.flush().into_diagnostic()?;
    Ok(())
}

fn watch_all_dirs(watcher: &mut dyn WatchController, dirs: &[PathBuf]) -> miette::Result<()> {
    for dir in dirs {
        watcher.watch_dir(dir)?;
    }
    Ok(())
}

fn update_watched_dirs(
    watcher: &mut dyn WatchController,
    old_dirs: &[PathBuf],
    new_dirs: &[PathBuf],
) -> miette::Result<()> {
    for old in old_dirs {
        if !new_dirs.iter().any(|new| same_watch_path(old, new)) {
            watcher.unwatch_dir(old)?;
        }
    }
    for new in new_dirs {
        if !old_dirs.iter().any(|old| same_watch_path(old, new)) {
            watcher.watch_dir(new)?;
        }
    }
    Ok(())
}

fn describe_resolved_assets(assets: &ResolvedAssets) -> String {
    let mut parts = Vec::new();
    if let Some(path) = &assets.layouts {
        parts.push(format!("layouts={}", path.display()));
    }
    if let Some(path) = &assets.css {
        parts.push(format!("css={}", path.display()));
    }
    if let Some(path) = &assets.syntaxes {
        parts.push(format!("syntaxes={}", path.display()));
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join(", ")
    }
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

/// Resolve an asset path to concrete files: a file stands for itself, a
/// directory contributes its `*.{ext}` files in filename order (deterministic
/// — this is also the dispatch probe order and the CSS cascade order).
fn collect_asset_files(path: &Path, ext: &str) -> miette::Result<Vec<PathBuf>> {
    let metadata = fs::metadata(path).map_err(|err| {
        miette::miette!(
            "cannot read {}\nhelp: pass a .{ext} file or a directory containing them\ncaused by: {err}",
            path.display()
        )
    })?;
    if metadata.is_file() {
        return Ok(vec![path.to_owned()]);
    }
    let mut files: Vec<PathBuf> = fs::read_dir(path)
        .into_diagnostic()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().and_then(|e| e.to_str()) == Some(ext))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(miette::miette!(
            "no *.{ext} files in {}\nhelp: add at least one .{ext} file to the directory",
            path.display()
        ));
    }
    Ok(files)
}

fn load_layouts(layouts_path: Option<&Path>) -> miette::Result<peitho_core::Layouts> {
    let Some(path) = layouts_path else {
        let layout = core(peitho_core::parse_layout(
            BUILTIN_LAYOUT_NAME,
            BUILTIN_LAYOUT_HTML,
        ))?;
        return core(peitho_core::Layouts::new(vec![layout]));
    };
    let mut layouts = Vec::new();
    for file in collect_asset_files(path, "html")? {
        let html = fs::read_to_string(&file).into_diagnostic()?;
        layouts.push(core(peitho_core::parse_layout(layout_name(&file), &html))?);
    }
    core(peitho_core::Layouts::new(layouts))
}

fn load_css(css_path: Option<&Path>) -> miette::Result<Vec<peitho_core::CssFile>> {
    let Some(path) = css_path else {
        return Ok(vec![peitho_core::CssFile {
            name: "base.css (built-in)".to_owned(),
            content: BUILTIN_BASE_CSS.to_owned(),
        }]);
    };
    let mut files = Vec::new();
    for file in collect_asset_files(path, "css")? {
        files.push(peitho_core::CssFile {
            name: file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.display().to_string()),
            content: fs::read_to_string(&file).into_diagnostic()?,
        });
    }
    Ok(files)
}

fn build_artifacts(input: &Path) -> miette::Result<BuildArtifacts> {
    let markdown = fs::read_to_string(input).into_diagnostic()?;
    let frontmatter = core(peitho_core::parse_frontmatter(&markdown))?;
    let assets = resolve_assets(input, &frontmatter)?;
    let highlighter = match assets.syntaxes.as_deref() {
        Some(path) => {
            let files = collect_asset_files(path, "sublime-syntax")?;
            core(peitho_core::highlight::Highlighter::with_user_files(&files))?
        }
        None => peitho_core::highlight::Highlighter::defaults(),
    };
    let layouts = load_layouts(assets.layouts.as_deref())?;
    let css_files = load_css(assets.css.as_deref())?;
    let parsed = core(peitho_core::parse_markdown(
        &markdown,
        frontmatter,
        &highlighter,
    ))?;
    let mapped = core(peitho_core::dispatch_by_convention(parsed, &layouts))?;
    let checked = core(peitho_core::check_deck(mapped))?;
    let slide_count = checked.slide_count();
    let css = core(peitho_core::build_theme_css(
        &css_files,
        &checked.slide_slot_classes(),
        &layouts.slot_classes(),
    ))?;
    let mut image_resolver = ImageResolver::new(input);
    let (resolved, image_assets) = core(peitho_core::resolve_image_paths(checked, |request| {
        image_resolver.resolve(request)
    }))?;
    let manifest = peitho_core::build_manifest(&resolved, &image_assets);
    let manifest_json = core(peitho_core::manifest_json(&manifest))?;
    let rendered = core(peitho_core::render_deck(resolved, &highlighter))?;

    Ok(BuildArtifacts {
        slide_count,
        rendered,
        manifest_json,
        css,
        image_assets,
    })
}

fn emit_distribution(out: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    write_shared_assets(out, artifacts)?;
    write_slide_fragments(out, &artifacts.rendered)?;
    fs::write(out.join("manifest.json"), &artifacts.manifest_json).into_diagnostic()?;
    fs::write(
        out.join("index.html"),
        peitho_core::render_distribution_index(artifacts.rendered.settings().aspect_ratio()),
    )
    .into_diagnostic()?;
    Ok(())
}

fn emit_pdf_workspace(workspace: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    write_shared_assets(workspace, artifacts)?;
    let pdf_html = peitho_core::render_pdf_document(&artifacts.rendered);
    fs::write(workspace.join("pdf.html"), pdf_html).into_diagnostic()?;
    Ok(())
}

fn emit_measure_workspace(workspace: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    write_shared_assets(workspace, artifacts)?;
    let measure_html = peitho_core::render_measure_document(&artifacts.rendered);
    fs::write(workspace.join("measure.html"), measure_html).into_diagnostic()?;
    Ok(())
}

fn write_shared_assets(dir: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    fs::create_dir_all(dir).into_diagnostic()?;
    fs::write(dir.join("peitho.css"), &artifacts.css).into_diagnostic()?;
    write_image_assets(dir, &artifacts.image_assets)
}

#[derive(Debug, Clone)]
struct ChromeLookupEnv {
    env_path: Option<PathBuf>,
    mac_chrome: PathBuf,
    path_dirs: Vec<PathBuf>,
}

fn locate_chrome() -> miette::Result<PathBuf> {
    let path_dirs = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect())
        .unwrap_or_default();
    locate_chrome_with_env(&ChromeLookupEnv {
        env_path: env::var_os("PEITHO_CHROME_PATH").map(PathBuf::from),
        mac_chrome: PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        path_dirs,
    })
}

fn locate_chrome_with_env(lookup: &ChromeLookupEnv) -> miette::Result<PathBuf> {
    if let Some(path) = &lookup.env_path {
        if path.is_file() {
            return Ok(path.clone());
        }
        return Err(miette::miette!(
            "Chrome not found at PEITHO_CHROME_PATH={}\nhelp: install Google Chrome or Chromium, or set PEITHO_CHROME_PATH=<absolute-path>",
            path.display()
        ));
    }

    if lookup.mac_chrome.is_file() {
        return Ok(lookup.mac_chrome.clone());
    }

    for program in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ] {
        if let Some(path) = find_chrome_in_path(program, &lookup.path_dirs) {
            return Ok(path);
        }
    }

    Err(miette::miette!(
        "Chrome not found\nhelp: install Google Chrome or Chromium, or set PEITHO_CHROME_PATH=<absolute-path>"
    ))
}

fn find_chrome_in_path(program: &str, path_dirs: &[PathBuf]) -> Option<PathBuf> {
    path_dirs.iter().find_map(|dir| {
        let candidate = dir.join(program);
        candidate.is_file().then_some(candidate)
    })
}

const CHROME_ONE_SHOT_TIMEOUT: Duration = Duration::from_secs(60);

enum ChromeCompletion {
    PdfWritten { output_path: PathBuf },
    DumpDom,
}

impl ChromeCompletion {
    fn description(&self) -> &'static str {
        match self {
            Self::PdfWritten { .. } => "PDF output",
            Self::DumpDom => "dumped DOM",
        }
    }

    fn is_ready(&self, stdout: &[u8], stderr: &[u8]) -> bool {
        match self {
            Self::PdfWritten { output_path } => {
                output_file_is_nonempty(output_path)
                    && String::from_utf8_lossy(stderr).contains("bytes written to file")
            }
            Self::DumpDom => String::from_utf8_lossy(stdout).contains("</html>"),
        }
    }
}

#[derive(Debug)]
enum ChromePipeEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

#[derive(Debug, Clone, Copy)]
enum ChromePipe {
    Stdout,
    Stderr,
}

fn run_one_shot_chrome(
    chrome: &Path,
    args: &[OsString],
    completion: ChromeCompletion,
    timeout: Duration,
) -> miette::Result<Vec<u8>> {
    let mut child = std::process::Command::new(chrome)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            miette::miette!(
                "failed to run Chrome at {}\nhelp: install Google Chrome or set PEITHO_CHROME_PATH=<absolute-path>\ncaused by: {err}",
                chrome.display()
            )
        })?;
    let stdout_pipe = child.stdout.take().ok_or_else(|| {
        miette::miette!(
            "failed to capture Chrome stdout\nhelp: retry export; this is an internal process setup error"
        )
    })?;
    let stderr_pipe = child.stderr.take().ok_or_else(|| {
        miette::miette!(
            "failed to capture Chrome stderr\nhelp: retry export; this is an internal process setup error"
        )
    })?;
    let (tx, rx) = mpsc::channel();
    let _stdout_reader = spawn_chrome_pipe_reader(stdout_pipe, ChromePipe::Stdout, tx.clone());
    let _stderr_reader = spawn_chrome_pipe_reader(stderr_pipe, ChromePipe::Stderr, tx);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let deadline = Instant::now() + timeout;

    loop {
        if completion.is_ready(&stdout, &stderr) {
            kill_and_reap_child(&mut child)?;
            drain_chrome_events(&rx, &mut stdout, &mut stderr);
            return Ok(stdout);
        }

        if let Some(status) = child.try_wait().into_diagnostic()? {
            drain_chrome_events_for(&rx, &mut stdout, &mut stderr, Duration::from_millis(100));
            if completion.is_ready(&stdout, &stderr) {
                return Ok(stdout);
            }
            if !status.success() {
                return Err(miette::miette!(
                    "Chrome failed during one-shot operation with status {}\nhelp: check that Chrome can run in headless mode\nstderr: {}",
                    status,
                    String::from_utf8_lossy(&stderr).trim()
                ));
            }
            return Err(miette::miette!(
                "Chrome completed before one-shot output was ready\nhelp: expected {} before Chrome exited\nstderr: {}",
                completion.description(),
                String::from_utf8_lossy(&stderr).trim()
            ));
        }

        let now = Instant::now();
        if now >= deadline {
            kill_and_reap_child(&mut child)?;
            drain_chrome_events(&rx, &mut stdout, &mut stderr);
            return Err(miette::miette!(
                "Chrome timed out after {}s waiting for {}\nhelp: retry export or check the generated HTML in the temporary workspace\nstderr: {}",
                timeout.as_secs(),
                completion.description(),
                String::from_utf8_lossy(&stderr).trim()
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        let poll = remaining.min(Duration::from_millis(25));
        match rx.recv_timeout(poll) {
            Ok(event) => append_chrome_event(event, &mut stdout, &mut stderr),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
    }
}

fn spawn_chrome_pipe_reader<R>(
    mut pipe: R,
    pipe_name: ChromePipe,
    tx: mpsc::Sender<ChromePipeEvent>,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let bytes = buffer[..n].to_vec();
                    let event = match pipe_name {
                        ChromePipe::Stdout => ChromePipeEvent::Stdout(bytes),
                        ChromePipe::Stderr => ChromePipeEvent::Stderr(bytes),
                    };
                    if tx.send(event).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

fn append_chrome_event(event: ChromePipeEvent, stdout: &mut Vec<u8>, stderr: &mut Vec<u8>) {
    match event {
        ChromePipeEvent::Stdout(bytes) => stdout.extend_from_slice(&bytes),
        ChromePipeEvent::Stderr(bytes) => stderr.extend_from_slice(&bytes),
    }
}

fn drain_chrome_events(
    rx: &mpsc::Receiver<ChromePipeEvent>,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) {
    while let Ok(event) = rx.try_recv() {
        append_chrome_event(event, stdout, stderr);
    }
}

fn drain_chrome_events_for(
    rx: &mpsc::Receiver<ChromePipeEvent>,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    duration: Duration,
) {
    let deadline = Instant::now() + duration;
    loop {
        drain_chrome_events(rx, stdout, stderr);
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline.saturating_duration_since(now);
        let poll = remaining.min(Duration::from_millis(10));
        match rx.recv_timeout(poll) {
            Ok(event) => append_chrome_event(event, stdout, stderr),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn kill_and_reap_child(child: &mut Child) -> miette::Result<()> {
    if child.try_wait().into_diagnostic()?.is_none() {
        child.kill().into_diagnostic()?;
    }
    child.wait().into_diagnostic()?;
    Ok(())
}

fn output_file_is_nonempty(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

fn run_chrome_print(chrome: &Path, workspace: &Path, out: &Path) -> miette::Result<()> {
    let abs_out = absolute_path_for_output(out)?;
    let profile = workspace.join("chrome-profile");
    fs::create_dir_all(&profile).into_diagnostic()?;
    let pdf_html = workspace.join("pdf.html");
    let url = file_url(&pdf_html)?;
    let args = vec![
        OsString::from("--headless=new"),
        OsString::from("--disable-gpu"),
        OsString::from("--no-sandbox"),
        OsString::from("--no-pdf-header-footer"),
        OsString::from(format!("--user-data-dir={}", profile.display())),
        OsString::from(format!("--print-to-pdf={}", abs_out.display())),
        OsString::from(url),
    ];
    run_one_shot_chrome(
        chrome,
        &args,
        ChromeCompletion::PdfWritten {
            output_path: abs_out.clone(),
        },
        CHROME_ONE_SHOT_TIMEOUT,
    )?;
    let metadata = fs::metadata(&abs_out).map_err(|err| {
        miette::miette!(
            "Chrome did not create PDF output at {}\nhelp: check output path permissions\ncaused by: {err}",
            abs_out.display()
        )
    })?;
    if metadata.len() == 0 {
        return Err(miette::miette!(
            "Chrome created an empty PDF at {}\nhelp: rerun export and check Chrome stderr",
            abs_out.display()
        ));
    }
    Ok(())
}

fn run_chrome_dump_dom(chrome: &Path, workspace: &Path) -> miette::Result<String> {
    let profile = workspace.join("chrome-profile");
    fs::create_dir_all(&profile).into_diagnostic()?;
    let measure_html = workspace.join("measure.html");
    let url = file_url(&measure_html)?;
    let args = vec![
        OsString::from("--headless=new"),
        OsString::from("--disable-gpu"),
        OsString::from("--no-sandbox"),
        OsString::from(format!("--user-data-dir={}", profile.display())),
        OsString::from("--dump-dom"),
        OsString::from("--virtual-time-budget=5000"),
        OsString::from(url),
    ];
    let stdout = run_one_shot_chrome(
        chrome,
        &args,
        ChromeCompletion::DumpDom,
        CHROME_ONE_SHOT_TIMEOUT,
    )?;
    String::from_utf8(stdout).map_err(|err| {
        miette::miette!(
            "Chrome dump-dom output was not UTF-8\nhelp: rerun export and inspect measure.html\ncaused by: {err}"
        )
    })
}

fn extract_measure_json(dumped_dom: &str) -> miette::Result<String> {
    let marker = r#"id="peitho-measure""#;
    let id_pos = dumped_dom.find(marker).ok_or_else(|| {
        miette::miette!(
            "measurement marker not found in Chrome dump\nhelp: ensure measure.js ran and appended <script id=\"peitho-measure\" type=\"application/json\">"
        )
    })?;
    let payload_start = dumped_dom[id_pos..]
        .find('>')
        .map(|offset| id_pos + offset + 1)
        .ok_or_else(|| {
            miette::miette!(
                "measurement marker opening tag was incomplete\nhelp: rerun export and inspect Chrome dump-dom output"
            )
        })?;
    let payload_end = dumped_dom[payload_start..]
        .find("</script>")
        .map(|offset| payload_start + offset)
        .ok_or_else(|| {
            miette::miette!(
                "measurement marker closing tag not found\nhelp: rerun export and inspect Chrome dump-dom output"
            )
        })?;
    Ok(dumped_dom[payload_start..payload_end].to_owned())
}

fn absolute_path_for_output(out: &Path) -> miette::Result<PathBuf> {
    if out.is_absolute() {
        return Ok(out.to_path_buf());
    }
    Ok(env::current_dir().into_diagnostic()?.join(out))
}

fn file_url(path: &Path) -> miette::Result<String> {
    let abs = absolute_path_for_output(path)?;
    Ok(format!("file://{}", abs.display()))
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

    validate_manifest_refs(dist, &manifest)?;
    Ok(manifest)
}

fn validate_manifest_refs(dist: &Path, manifest: &peitho_core::Manifest) -> miette::Result<()> {
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
        validate_manifest_dist_ref(dist, slide.src(), ManifestRefKind::Slide)?;
    }

    for image in manifest.images() {
        validate_manifest_dist_ref(dist, image.src(), ManifestRefKind::Image)?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum ManifestRefKind {
    Slide,
    Image,
}

impl ManifestRefKind {
    fn invalid_message(self, src: &str) -> String {
        match self {
            Self::Slide => format!("manifest contains invalid slide src: {src}"),
            Self::Image => format!("manifest contains invalid image src: {src}"),
        }
    }

    fn invalid_help(self) -> &'static str {
        match self {
            Self::Slide => "slide src must be a relative path inside dist/",
            Self::Image => "image src must be a relative path inside dist/",
        }
    }

    fn missing_message(self, src: &str) -> String {
        match self {
            Self::Slide => format!("manifest references missing slide fragment: {src}"),
            Self::Image => format!("manifest references missing image asset: {src}"),
        }
    }
}

fn validate_manifest_dist_ref(dist: &Path, src: &str, kind: ManifestRefKind) -> miette::Result<()> {
    let path = Path::new(src);
    let invalid_component = path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    });
    if src.is_empty() || path.is_absolute() || invalid_component {
        return Err(miette::miette!(
            "{}\nhelp: {}",
            kind.invalid_message(src),
            kind.invalid_help()
        ));
    }

    let canonical_dist = fs::canonicalize(dist).map_err(|err| {
        miette::miette!(
            "distribution is incomplete: failed to resolve dist directory\nhelp: run `peitho build` first\ncaused by: {err}"
        )
    })?;
    let target = dist.join(path);
    let canonical_target = match fs::canonicalize(&target) {
        Ok(path) => path,
        Err(_) => {
            return Err(miette::miette!(
                "{}\nhelp: run `peitho build` first",
                kind.missing_message(src)
            ));
        }
    };
    if !canonical_target.starts_with(&canonical_dist) {
        return Err(miette::miette!(
            "{}\nhelp: {}",
            kind.invalid_message(src),
            kind.invalid_help()
        ));
    }

    if !canonical_target.is_file() {
        return Err(miette::miette!(
            "{}\nhelp: run `peitho build` first",
            kind.missing_message(src)
        ));
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

    let artifacts = build_artifacts(&options.input)?;
    if options.no_serve {
        emit_present_cache(&cache, &artifacts, options.shell.as_deref(), false)?;
        println!("generated present cache at {}", cache.display());
        return Ok(());
    }

    let server = server::PresentServer::bind(cache.clone(), options.port)?;
    let url = server.url();
    let presenter_url = browser::presenter_url(&url);
    let browser_plan = if options.no_open {
        None
    } else {
        Some(browser::plan_browser_with_request(
            browser::BrowserOpenRequest {
                slides_url: &url,
                presenter_url: &presenter_url,
                no_presenter: options.no_presenter,
            },
            options.presenter_windowed,
        ))
    };
    let presenter_open = browser_plan
        .as_ref()
        .is_some_and(|plan| plan.opens_presenter);
    emit_present_cache(&cache, &artifacts, options.shell.as_deref(), presenter_open)?;
    println!("serving presentation at {url}");
    std::io::stdout().flush().into_diagnostic()?;
    if let Some(plan) = browser_plan {
        browser::open_browser_plan(plan);
    }
    let result = server.serve_forever();
    if !options.no_open {
        browser::quit_profile_instances();
    }
    result
}

fn emit_present_cache(
    cache: &Path,
    artifacts: &BuildArtifacts,
    shell: Option<&Path>,
    presenter_open: bool,
) -> miette::Result<()> {
    if let Some(shell) = shell {
        ensure_shell_bundle(shell)?;
    }
    fs::write(cache.join("peitho.css"), &artifacts.css).into_diagnostic()?;
    write_slide_fragments(cache, &artifacts.rendered)?;
    write_image_assets(cache, &artifacts.image_assets)?;
    fs::write(cache.join("manifest.json"), &artifacts.manifest_json).into_diagnostic()?;
    fs::write(
        cache.join("notes.json"),
        core(peitho_core::notes_json(&peitho_core::Notes::from_slides(
            artifacts.rendered.slides(),
        )))?,
    )
    .into_diagnostic()?;
    fs::write(
        cache.join("present.json"),
        core(peitho_core::present_config_json(
            &peitho_core::PresentConfig::new(presenter_open),
        ))?,
    )
    .into_diagnostic()?;
    fs::write(
        cache.join("present.html"),
        peitho_core::render_present_index(artifacts.rendered.settings().aspect_ratio()),
    )
    .into_diagnostic()?;
    fs::write(
        cache.join("presenter.html"),
        peitho_core::render_presenter_index(artifacts.rendered.settings().aspect_ratio()),
    )
    .into_diagnostic()?;
    match shell {
        Some(shell) => {
            fs::copy(shell, cache.join("shell.js")).into_diagnostic()?;
        }
        None => {
            fs::write(cache.join("shell.js"), BUILTIN_SHELL_JS).into_diagnostic()?;
        }
    }
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

fn write_image_assets(
    out: &Path,
    image_assets: &[peitho_core::ResolvedImageAsset],
) -> miette::Result<()> {
    let assets_dir = out.join("assets");
    if assets_dir.exists() {
        fs::remove_dir_all(&assets_dir).into_diagnostic()?;
    }
    fs::create_dir_all(&assets_dir).into_diagnostic()?;
    for asset in image_assets {
        fs::copy(&asset.source_abs, out.join(asset.dist_rel.as_str())).into_diagnostic()?;
    }
    Ok(())
}

struct ImageResolver {
    deck_dir: PathBuf,
    by_hash: BTreeMap<String, peitho_core::ResolvedImageAsset>,
}

impl ImageResolver {
    fn new(input: &Path) -> Self {
        let deck_dir = asset_resolution::deck_parent(input).to_path_buf();
        Self {
            deck_dir,
            by_hash: BTreeMap::new(),
        }
    }

    fn resolve(
        &mut self,
        request: peitho_core::ImageRequest<'_>,
    ) -> peitho_core::Result<peitho_core::ResolvedImageAsset> {
        let source = self.deck_dir.join(request.raw.as_str());
        let display_path = request.raw.as_str();
        let deck_abs =
            fs::canonicalize(&self.deck_dir).map_err(|err| image_read_error(display_path, err))?;
        let source_abs =
            fs::canonicalize(&source).map_err(|err| image_metadata_error(display_path, err))?;
        if !source_abs.starts_with(&deck_abs) {
            return Err(peitho_core::BuildError::new(
                peitho_core::error::ErrorKind::Asset,
                None,
                format!("image path escapes deck directory: {display_path}"),
                "keep image files inside the deck directory",
            ));
        }
        let metadata =
            fs::metadata(&source_abs).map_err(|err| image_metadata_error(display_path, err))?;
        if !metadata.is_file() {
            return Err(peitho_core::BuildError::new(
                peitho_core::error::ErrorKind::Asset,
                None,
                format!("image file not found: {display_path}"),
                "place the image at the deck-relative path or fix the path",
            ));
        }
        let bytes = fs::read(&source_abs).map_err(|err| image_read_error(display_path, err))?;
        let hash = short_sha256_hex(&bytes);
        if let Some(asset) = self.by_hash.get(&hash) {
            return Ok(asset.clone());
        }
        let basename = Path::new(request.raw.as_str())
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                peitho_core::BuildError::new(
                    peitho_core::error::ErrorKind::Asset,
                    None,
                    format!("image path has no file name: {}", request.raw.as_str()),
                    "write a deck-relative image path with a file name",
                )
            })?;
        let dist_rel = peitho_core::ResolvedImagePath::from_hashed_asset(&hash, basename).map_err(
            |message| {
                peitho_core::BuildError::new(
                    peitho_core::error::ErrorKind::Asset,
                    None,
                    message,
                    "keep generated image asset paths under assets/",
                )
            },
        )?;
        let asset = peitho_core::ResolvedImageAsset {
            source_abs,
            dist_rel,
        };
        self.by_hash.insert(hash, asset.clone());
        Ok(asset)
    }
}

fn image_metadata_error(path: &str, err: std::io::Error) -> peitho_core::BuildError {
    match err.kind() {
        std::io::ErrorKind::NotFound => peitho_core::BuildError::new(
            peitho_core::error::ErrorKind::Asset,
            None,
            format!("image file not found: {path}"),
            "place the image at the deck-relative path or fix the path",
        ),
        _ => peitho_core::BuildError::new(
            peitho_core::error::ErrorKind::Asset,
            None,
            format!("image file unreadable: {path}"),
            "make the image file readable",
        ),
    }
}

fn image_read_error(path: &str, err: std::io::Error) -> peitho_core::BuildError {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => peitho_core::BuildError::new(
            peitho_core::error::ErrorKind::Asset,
            None,
            format!("image file unreadable: {path}"),
            "make the image file readable",
        ),
        _ => peitho_core::BuildError::new(
            peitho_core::error::ErrorKind::Asset,
            None,
            format!("failed to read image: {err}"),
            "make sure the image exists and can be read",
        ),
    }
}

fn short_sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash: [u8; 32] = hasher.finalize().into();
    let mut hex = String::with_capacity(16);
    for byte in &hash[..8] {
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    hex
}

fn core<T>(result: peitho_core::Result<T>) -> miette::Result<T> {
    result.map_err(|err| miette::miette!("{err}"))
}

fn layout_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARINA_SUBLIME_SYNTAX: &str = r#"%YAML 1.2
---
name: Carina
file_extensions: [crn]
scope: source.carina
contexts:
  main:
    - match: '\b(resource|provider|module)\b'
      scope: keyword.control.carina
"#;
    const TEST_LAYOUT_HTML: &str = r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#;

    #[test]
    fn watch_dependency_types_are_available() {
        fn accepts_recursive_mode(_mode: notify::RecursiveMode) {}

        accepts_recursive_mode(notify::RecursiveMode::NonRecursive);
        let result: notify_debouncer_mini::DebounceEventResult = Ok(Vec::new());

        assert!(matches!(result, Ok(events) if events.is_empty()));
    }

    #[test]
    fn watch_covers_asset_dir_contents_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        let layouts = dir.path().join("layouts");
        let css = dir.path().join("css");
        let syntaxes = dir.path().join("syntaxes");
        fs::create_dir_all(&layouts).unwrap();
        fs::create_dir_all(&css).unwrap();
        fs::create_dir_all(&syntaxes).unwrap();
        let targets = WatchTargets::new(
            dir.path().join("deck.md"),
            ResolvedAssets {
                layouts: Some(layouts.clone()),
                css: Some(css.clone()),
                syntaxes: Some(syntaxes.clone()),
            },
        );

        assert!(targets.is_relevant_change(&dir.path().join("deck.md")));
        assert!(targets.is_relevant_change(&layouts.join("cover.html")));
        assert!(targets.is_relevant_change(&css.join("base.css")));
        assert!(targets.is_relevant_change(&syntaxes.join("foo.sublime-syntax")));
        assert!(!targets.is_relevant_change(&layouts.join("notes.txt")));
        assert!(!targets.is_relevant_change(&dir.path().join("other.md")));

        let dirs = targets.watch_dirs();
        assert!(dirs.iter().any(|d| d == &layouts));
        assert!(dirs.iter().any(|d| d == &css));
        assert!(dirs.iter().any(|d| d == &syntaxes));
        assert!(dirs.iter().any(|d| d == dir.path()));
    }

    #[test]
    fn build_options_with_builtin_assets_watch_only_the_deck() {
        let targets = WatchTargets::new(PathBuf::from("deck.md"), empty_assets());

        assert!(targets.is_relevant_change(Path::new("deck.md")));
        assert!(!targets.is_relevant_change(Path::new("layout.html")));
    }

    #[test]
    fn build_artifacts_uses_builtin_layout_and_theme_without_flags() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n\nBody\n\n```rust\nfn main() {}\n```\n").unwrap();

        let artifacts = build_artifacts(&deck).unwrap();

        assert_eq!(artifacts.slide_count, 1);
        assert!(artifacts
            .css
            .contains("width: var(--peitho-canvas-width, 1280px);"));
    }

    #[test]
    fn build_artifacts_uses_syntaxes_dir_next_to_the_deck() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let syntaxes = dir.path().join("syntaxes");
        fs::write(
            &deck,
            "# Infra\n\n```carina\nresource \"aws_s3_bucket\" \"site\" {}\n```\n",
        )
        .unwrap();
        fs::create_dir_all(&syntaxes).unwrap();
        fs::write(
            syntaxes.join("carina.sublime-syntax"),
            CARINA_SUBLIME_SYNTAX,
        )
        .unwrap();
        let artifacts = build_artifacts(&deck).unwrap();

        assert!(artifacts.rendered.slides()[0].html().contains("hl-"));
    }

    #[test]
    fn conventional_dirs_next_to_the_deck_win_over_builtins() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n").unwrap();
        fs::create_dir_all(dir.path().join("layouts")).unwrap();
        fs::create_dir_all(dir.path().join("css")).unwrap();
        let frontmatter = peitho_core::parse_frontmatter("# Intro\n").unwrap();
        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.layouts, Some(dir.path().join("layouts")));
        assert_eq!(assets.css, Some(dir.path().join("css")));
    }

    #[test]
    fn frontmatter_key_wins_over_conventional_dir() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n").unwrap();
        fs::create_dir_all(dir.path().join("layouts")).unwrap();
        let explicit = dir.path().join("other-layouts");
        fs::create_dir_all(&explicit).unwrap();
        let frontmatter =
            peitho_core::parse_frontmatter("---\nlayouts: ./other-layouts\n---\n# Intro\n")
                .unwrap();
        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.layouts, Some(explicit));
    }

    #[test]
    fn no_frontmatter_key_and_no_conventional_dir_resolves_to_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n").unwrap();
        let frontmatter = peitho_core::parse_frontmatter("# Intro\n").unwrap();
        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.layouts, None);
        assert_eq!(assets.css, None);
    }

    #[test]
    fn collect_asset_files_sorts_directory_entries_by_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.css"), "").unwrap();
        fs::write(dir.path().join("a.css"), "").unwrap();
        fs::write(dir.path().join("ignore.txt"), "").unwrap();

        let files = collect_asset_files(dir.path(), "css").unwrap();

        assert_eq!(
            files,
            vec![dir.path().join("a.css"), dir.path().join("b.css")]
        );
    }

    #[test]
    fn collect_asset_files_rejects_directory_without_matches() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ignore.txt"), "").unwrap();

        let err = collect_asset_files(dir.path(), "html").unwrap_err();

        assert!(err.to_string().contains("no *.html files"));
    }

    #[test]
    fn build_options_deduplicates_watch_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let targets = WatchTargets::new(
            dir.path().join("deck.md"),
            ResolvedAssets {
                layouts: Some(dir.path().join("title-body-code.html")),
                css: Some(dir.path().join("base.css")),
                syntaxes: None,
            },
        );

        assert_eq!(targets.watch_dirs(), vec![dir.path().to_path_buf()]);
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
        let mut fixture = WatchFixture::new("# Intro\n");
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
        fs::write(&fixture.options.input, "# Intro\n\n---\n# Details\n").unwrap();

        handle_watch_paths(
            &fixture.options,
            &mut fixture.targets,
            &mut watcher,
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
        let mut fixture = WatchFixture::new("# Intro\n");
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let unrelated = fixture._dir.path().join("outside").join("ignored.txt");

        handle_watch_paths(
            &fixture.options,
            &mut fixture.targets,
            &mut watcher,
            &[unrelated],
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        assert!(!fixture.options.out.join("manifest.json").exists());
    }

    #[test]
    fn watch_path_handler_ignores_output_directory_event_in_watched_parent() {
        let mut fixture = WatchFixture::new("# Intro\n");
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
        stdout.clear();
        stderr.clear();

        handle_watch_paths(
            &fixture.options,
            &mut fixture.targets,
            &mut watcher,
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
        let mut fixture = WatchFixture::new("# Intro\n");
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let temp = fixture._dir.path().join("deck-new.md");

        fs::write(&temp, "# Atomic one\n\n---\n# Atomic two\n").unwrap();
        fs::rename(&temp, &fixture.options.input).unwrap();

        handle_watch_paths(
            &fixture.options,
            &mut fixture.targets,
            &mut watcher,
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
    fn watch_path_handler_rewatches_when_frontmatter_asset_paths_change() {
        let mut fixture = WatchFixture::new("# Intro\n");
        let mut watcher = RecordingWatchController::default();
        let alternate_layouts = fixture._dir.path().join("other-layouts");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        fs::create_dir_all(&alternate_layouts).unwrap();
        fs::write(
            alternate_layouts.join("title-body-code.html"),
            TEST_LAYOUT_HTML,
        )
        .unwrap();
        fs::write(
            &fixture.options.input,
            "---\nlayouts: ./other-layouts\n---\n# Intro\n",
        )
        .unwrap();

        handle_watch_paths(
            &fixture.options,
            &mut fixture.targets,
            &mut watcher,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(
            fixture.targets.assets.layouts,
            Some(alternate_layouts.clone())
        );
        assert!(watcher
            .watched
            .iter()
            .any(|path| path == &alternate_layouts));
        assert!(watcher
            .unwatched
            .iter()
            .any(|path| path == &fixture._dir.path().join("layouts")));
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("note: watching new asset paths from frontmatter:"));
        assert!(!stderr.contains("restart --watch"));
    }

    #[test]
    fn watch_path_handler_reports_rebuild_error_when_asset_resolution_fails() {
        let mut fixture = WatchFixture::new("# Intro\n");
        let mut watcher = RecordingWatchController::default();
        let original_assets = fixture.targets.assets.clone();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        fs::write(
            &fixture.options.input,
            "---\nlayouts: ./missing-layouts\n---\n# Intro\n",
        )
        .unwrap();

        handle_watch_paths(
            &fixture.options,
            &mut fixture.targets,
            &mut watcher,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(fixture.targets.assets, original_assets);
        assert!(watcher.watched.is_empty());
        assert!(watcher.unwatched.is_empty());
        assert!(stdout.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("build failed:"));
        assert!(stderr.contains("layouts path does not exist"));
        assert!(!stderr.contains("restart --watch"));
        assert!(!stderr.contains("watching new asset paths"));
    }

    #[test]
    fn watch_build_function_is_available_for_cli_dispatch() {
        let _watch: fn(BuildOptions) -> miette::Result<()> = watch_build;
    }

    #[test]
    fn present_command_accepts_presenter_windowed_flag() {
        let cli = Cli::parse_from(["peitho", "present", "deck.md", "--presenter-windowed"]);

        match cli.command {
            Command::Present {
                input,
                presenter_windowed,
                ..
            } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert!(presenter_windowed);
            }
            Command::Build { .. } | Command::Publish { .. } | Command::Export { .. } => {
                panic!("expected present command");
            }
        }
    }

    #[test]
    fn export_pdf_command_accepts_output_flag() {
        let cli = Cli::parse_from(["peitho", "export", "pdf", "deck.md", "-o", "out.pdf"]);

        match cli.command {
            Command::Export {
                command: ExportCommand::Pdf { input, out },
            } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert_eq!(out, Some(PathBuf::from("out.pdf")));
            }
            Command::Build { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Export { .. } => {
                panic!("expected export pdf command");
            }
        }
    }

    #[test]
    fn export_pptx_command_accepts_output_flag() {
        let cli = Cli::parse_from(["peitho", "export", "pptx", "deck.md", "-o", "out.pptx"]);

        match cli.command {
            Command::Export {
                command: ExportCommand::Pptx { input, out },
            } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert_eq!(out, Some(PathBuf::from("out.pptx")));
            }
            Command::Build { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Export { .. } => {
                panic!("expected export pptx command");
            }
        }
    }

    #[test]
    fn emit_pdf_workspace_writes_static_pdf_entry_without_notes_or_manifest() {
        let fixture = WatchFixture::new(
            "---\nresolution: 1920x1080\n---\n# Export\n\n<!-- private note -->\n",
        );
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let workspace = fixture._dir.path().join("pdf-workspace");

        emit_pdf_workspace(&workspace, &artifacts).unwrap();

        let html = fs::read_to_string(workspace.join("pdf.html")).unwrap();
        assert!(html.contains("@page { size: 1920px 1080px; margin: 0; }"));
        assert!(html.contains(r#"data-slide-key="export""#));
        assert!(!html.contains("private note"));
        assert!(!workspace.join("manifest.json").exists());
        assert!(!workspace.join("slides").exists());
        assert!(workspace.join("peitho.css").is_file());
    }

    #[test]
    fn emit_pdf_workspace_allows_note_text_that_also_appears_in_slide_html() {
        let fixture = WatchFixture::new("# PDF Export\n\n<!-- PDF -->\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let workspace = fixture._dir.path().join("pdf-workspace");

        emit_pdf_workspace(&workspace, &artifacts).unwrap();

        assert!(workspace.join("pdf.html").is_file());
    }

    #[test]
    fn emit_measure_workspace_writes_static_measure_entry_and_assets() {
        let fixture = WatchFixture::new("# Export\n\n<!-- private note -->\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let workspace = fixture._dir.path().join("measure-workspace");

        emit_measure_workspace(&workspace, &artifacts).unwrap();

        let html = fs::read_to_string(workspace.join("measure.html")).unwrap();
        assert!(html.contains(r#"data-slide-key="export""#));
        assert!(html.contains("peitho-measure"));
        assert!(!html.contains("private note"));
        assert!(!workspace.join("manifest.json").exists());
        assert!(!workspace.join("slides").exists());
        assert!(workspace.join("peitho.css").is_file());
    }

    #[test]
    fn extract_measure_json_reads_marker_payload_from_dumped_dom() {
        let dumped = r#"<!doctype html><html><body><script id="peitho-measure" type="application/json">{"canvasWidth":1280,"canvasHeight":720,"slides":[]}</script></body></html>"#;

        let json = extract_measure_json(dumped).unwrap();

        assert_eq!(
            json,
            r#"{"canvasWidth":1280,"canvasHeight":720,"slides":[]}"#
        );
    }

    #[test]
    fn extract_measure_json_reports_missing_marker() {
        let err = extract_measure_json("<html></html>").unwrap_err();

        assert!(err.to_string().contains("measurement marker not found"));
    }

    #[cfg(unix)]
    #[test]
    fn chrome_dump_dom_runner_returns_dump_after_html_closing_tag_and_kills_child() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("measure.html"), "<html></html>").unwrap();
        write_executable_script(
            &fake_chrome,
            r#"#!/bin/sh
printf '%s\n' '<!doctype html><html><body><script id="peitho-measure" type="application/json">{"canvasWidth":1280,"canvasHeight":720,"slides":[]}</script></body></html>'
exec sleep 30
"#,
        );

        let started = std::time::Instant::now();
        let dumped = run_chrome_dump_dom(&fake_chrome, &workspace).unwrap();

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(dumped.contains("peitho-measure"));
    }

    #[test]
    fn locate_chrome_prefers_env_var_without_running_browser() {
        let dir = tempfile::tempdir().unwrap();
        let env_chrome = dir.path().join("env-chrome");
        let mac_chrome = dir.path().join("mac-chrome");
        let path_chrome_dir = dir.path().join("bin");
        fs::create_dir_all(&path_chrome_dir).unwrap();
        write_fake_browser(&env_chrome);
        write_fake_browser(&mac_chrome);
        write_fake_browser(&path_chrome_dir.join("google-chrome"));

        let chrome = locate_chrome_with_env(&ChromeLookupEnv {
            env_path: Some(env_chrome.clone()),
            mac_chrome,
            path_dirs: vec![path_chrome_dir],
        })
        .unwrap();

        assert_eq!(chrome, env_chrome);
    }

    #[test]
    fn locate_chrome_uses_mac_default_before_path() {
        let dir = tempfile::tempdir().unwrap();
        let mac_chrome = dir.path().join("mac-chrome");
        let path_chrome_dir = dir.path().join("bin");
        fs::create_dir_all(&path_chrome_dir).unwrap();
        write_fake_browser(&mac_chrome);
        write_fake_browser(&path_chrome_dir.join("google-chrome"));

        let chrome = locate_chrome_with_env(&ChromeLookupEnv {
            env_path: None,
            mac_chrome: mac_chrome.clone(),
            path_dirs: vec![path_chrome_dir],
        })
        .unwrap();

        assert_eq!(chrome, mac_chrome);
    }

    #[test]
    fn locate_chrome_searches_path_in_required_order() {
        let dir = tempfile::tempdir().unwrap();
        let path_chrome_dir = dir.path().join("bin");
        fs::create_dir_all(&path_chrome_dir).unwrap();
        write_fake_browser(&path_chrome_dir.join("chromium"));
        write_fake_browser(&path_chrome_dir.join("google-chrome-stable"));

        let chrome = locate_chrome_with_env(&ChromeLookupEnv {
            env_path: None,
            mac_chrome: dir.path().join("missing-mac-chrome"),
            path_dirs: vec![path_chrome_dir.clone()],
        })
        .unwrap();

        assert_eq!(chrome, path_chrome_dir.join("google-chrome-stable"));
    }

    #[test]
    fn locate_chrome_reports_help_when_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = locate_chrome_with_env(&ChromeLookupEnv {
            env_path: None,
            mac_chrome: dir.path().join("missing-mac-chrome"),
            path_dirs: Vec::new(),
        })
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("Chrome not found"));
        assert!(message.contains("PEITHO_CHROME_PATH=<absolute-path>"));
    }

    #[cfg(unix)]
    #[test]
    fn one_shot_chrome_runner_returns_after_pdf_completion_signal_and_kills_child() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        let out = dir.path().join("out.pdf");
        write_executable_script(
            &fake_chrome,
            r#"#!/bin/sh
out="$1"
printf '%s' '%PDF-test' > "$out"
printf '9 bytes written to file %s\n' "$out" >&2
exec sleep 30
"#,
        );

        let started = std::time::Instant::now();
        let stdout = run_one_shot_chrome(
            &fake_chrome,
            &[out.clone().into_os_string()],
            ChromeCompletion::PdfWritten {
                output_path: out.clone(),
            },
            Duration::from_secs(2),
        )
        .unwrap();

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(out.is_file());
        assert!(stdout.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn one_shot_chrome_runner_times_out_and_reaps_child_without_completion() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        write_executable_script(
            &fake_chrome,
            r#"#!/bin/sh
exec sleep 30
"#,
        );

        let started = std::time::Instant::now();
        let err = run_one_shot_chrome(
            &fake_chrome,
            &[],
            ChromeCompletion::DumpDom,
            Duration::from_millis(100),
        )
        .unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(err.to_string().contains("timed out"));
    }

    #[cfg(unix)]
    #[test]
    fn pdf_completion_requires_nonempty_output_file() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        let out = dir.path().join("out.pdf");
        write_executable_script(
            &fake_chrome,
            r#"#!/bin/sh
out="$1"
: > "$out"
printf '0 bytes written to file %s\n' "$out" >&2
"#,
        );

        let err = run_one_shot_chrome(
            &fake_chrome,
            &[out.clone().into_os_string()],
            ChromeCompletion::PdfWritten { output_path: out },
            Duration::from_secs(2),
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("completed before one-shot output was ready"));
    }

    #[test]
    fn emit_present_cache_writes_present_json() {
        let fixture = WatchFixture::new("# Intro\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();

        fs::create_dir_all(&fixture.options.out).unwrap();
        emit_present_cache(&fixture.options.out, &artifacts, None, true).unwrap();

        let json = fs::read_to_string(fixture.options.out.join("present.json")).unwrap();
        assert!(json.contains(r#""presenterOpen": true"#));
    }

    #[test]
    fn present_cache_copies_markdown_images() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("layouts");
        let css = dir.path().join("css");
        let cache = dir.path().join("present-cache");

        fs::write(&deck, "# Visual\n\n![Architecture](img/arch.png)").unwrap();
        fs::create_dir_all(&layouts).unwrap();
        fs::create_dir_all(&css).unwrap();
        fs::create_dir_all(dir.path().join("img")).unwrap();
        fs::write(
            layouts.join("visual.html"),
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="hero" accepts="image" arity="1"></slot></section>"#,
        )
        .unwrap();
        fs::write(
            css.join("base.css"),
            ".slot-hero img { max-width: 100%; }\n",
        )
        .unwrap();
        fs::write(dir.path().join("img/arch.png"), b"test png bytes").unwrap();

        let artifacts = build_artifacts(&deck).unwrap();
        fs::create_dir_all(&cache).unwrap();
        emit_present_cache(&cache, &artifacts, None, false).unwrap();

        let mut assets = fs::read_dir(cache.join("assets"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assets.sort();
        assert_eq!(assets.len(), 1);
        let asset_name = assets[0].to_string_lossy();
        assert!(asset_name.ends_with("-arch.png"));
        let slide = fs::read_to_string(cache.join("slides/000-visual.html")).unwrap();
        assert!(slide.contains(&format!(r#"<img src="assets/{asset_name}""#)));
    }

    #[test]
    fn image_resolver_handles_bare_deck_filename() {
        let resolver = ImageResolver::new(Path::new("deck.md"));

        assert_eq!(resolver.deck_dir, PathBuf::from("."));
    }

    fn write_fake_browser(path: &Path) {
        fs::write(path, "").unwrap();
    }

    #[cfg(unix)]
    fn write_executable_script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, body).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn build_command_accepts_watch_flag() {
        let cli = Cli::parse_from(["peitho", "build", "deck.md", "--watch"]);

        match cli.command {
            Command::Build { input, watch, .. } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert!(watch);
            }
            Command::Present { .. } | Command::Publish { .. } | Command::Export { .. } => {
                panic!("expected build command");
            }
        }
    }

    #[test]
    fn build_command_defaults_to_builtin_assets() {
        let cli = Cli::parse_from(["peitho", "build", "deck.md"]);

        match cli.command {
            Command::Build { input, out, watch } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert_eq!(out, PathBuf::from("dist"));
                assert!(!watch);
            }
            Command::Present { .. } | Command::Publish { .. } | Command::Export { .. } => {
                panic!("expected build command");
            }
        }
    }

    #[test]
    fn build_command_rejects_removed_asset_flags() {
        for args in [
            ["peitho", "build", "deck.md", "--layouts", "layouts"],
            ["peitho", "build", "deck.md", "--css", "layouts"],
            ["peitho", "present", "deck.md", "--layouts", "layouts"],
            ["peitho", "present", "deck.md", "--css", "css"],
        ] {
            let err = Cli::try_parse_from(args).unwrap_err();

            assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }

    #[derive(Default)]
    struct RecordingWatchController {
        watched: Vec<PathBuf>,
        unwatched: Vec<PathBuf>,
    }

    impl WatchController for RecordingWatchController {
        fn watch_dir(&mut self, dir: &Path) -> miette::Result<()> {
            self.watched.push(dir.to_path_buf());
            Ok(())
        }

        fn unwatch_dir(&mut self, dir: &Path) -> miette::Result<()> {
            self.unwatched.push(dir.to_path_buf());
            Ok(())
        }
    }

    struct WatchFixture {
        _dir: tempfile::TempDir,
        options: BuildOptions,
        targets: WatchTargets,
    }

    impl WatchFixture {
        fn new(markdown: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let deck = dir.path().join("deck.md");
            let layouts = dir.path().join("layouts");
            let css = dir.path().join("css");
            let out = dir.path().join("dist");

            fs::write(&deck, markdown).unwrap();
            fs::create_dir_all(&layouts).unwrap();
            fs::create_dir_all(&css).unwrap();
            fs::write(layouts.join("title-body-code.html"), TEST_LAYOUT_HTML).unwrap();
            fs::write(css.join("base.css"), ".slot-title { font-weight: 700; }\n").unwrap();
            let targets = resolve_watch_targets(&deck).unwrap();

            Self {
                _dir: dir,
                options: BuildOptions { input: deck, out },
                targets,
            }
        }
    }

    fn empty_assets() -> ResolvedAssets {
        ResolvedAssets {
            layouts: None,
            css: None,
            syntaxes: None,
        }
    }
}
