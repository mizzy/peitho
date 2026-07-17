use std::{
    collections::{BTreeMap, HashSet},
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, IsTerminal, Read, Write},
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::{Component, Path, PathBuf},
    process::{Child, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use miette::IntoDiagnostic;
use notify::{PollWatcher, RecursiveMode};
use notify_debouncer_mini::{
    new_debouncer_opt, Config as DebounceConfig, DebounceEventResult, Debouncer,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

mod asset_resolution;
mod doctor;
mod lint;
mod new_cmd;

use asset_resolution::{resolve_assets, Provenance, ResolvedAssets};
use peitho::{browser, server};

struct BuildArtifacts {
    slide_count: usize,
    rendered: peitho_core::Deck<peitho_core::Rendered>,
    manifest_json: String,
    css: String,
    image_assets: Vec<peitho_core::ResolvedImageAsset>,
    fonts_source: Option<PathBuf>,
}

struct CliSvgRunner {
    cwd: PathBuf,
    timeout: Duration,
}

impl Default for CliSvgRunner {
    fn default() -> Self {
        Self {
            cwd: PathBuf::from("."),
            timeout: Duration::from_secs(30),
        }
    }
}

impl CliSvgRunner {
    fn for_deck(input: &Path) -> Self {
        Self {
            cwd: asset_resolution::deck_parent(input).to_path_buf(),
            timeout: Duration::from_secs(30),
        }
    }
}

impl peitho_core::code_images::SvgRunner for CliSvgRunner {
    fn run(
        &self,
        command: &peitho_core::domain::CodeImageCommand,
        stdin: &str,
    ) -> peitho_core::Result<Vec<u8>> {
        run_code_image_command(command, stdin, self.timeout, &self.cwd)
    }
}

#[derive(Debug, Clone)]
struct BuildOptions {
    input: PathBuf,
    out: PathBuf,
}

#[derive(Debug, Clone)]
struct WatchRoot {
    path: PathBuf,
    ext: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct WatchTargets {
    roots: Vec<WatchRoot>,
    assets: ResolvedAssets,
}

struct WatchState {
    input: PathBuf,
    targets: WatchTargets,
    watched_dirs: Vec<PathBuf>,
    emitted_watch_error_notes: HashSet<String>,
}

impl WatchState {
    fn new(input: PathBuf, targets: WatchTargets, watched_dirs: Vec<PathBuf>) -> Self {
        Self {
            input,
            targets,
            watched_dirs,
            emitted_watch_error_notes: HashSet::new(),
        }
    }

    fn reconcile_after_events(
        &mut self,
        watcher: &mut dyn WatchController,
        stderr: &mut dyn Write,
    ) -> miette::Result<bool> {
        let desired_dirs = self.targets.watch_dirs();
        let result = reconcile_watched_dirs(
            watcher,
            &mut self.watched_dirs,
            &desired_dirs,
            stderr,
            &mut self.emitted_watch_error_notes,
        )?;
        if !result.had_failures {
            self.emitted_watch_error_notes.clear();
        }
        Ok(result.changed)
    }

    fn reconcile_after_error(
        &mut self,
        watcher: &mut dyn WatchController,
        stderr: &mut dyn Write,
    ) -> miette::Result<bool> {
        let desired_dirs = self.targets.watch_dirs();
        let result = reconcile_watched_dirs(
            watcher,
            &mut self.watched_dirs,
            &desired_dirs,
            stderr,
            &mut self.emitted_watch_error_notes,
        )?;
        Ok(result.changed)
    }

    fn write_watch_error_note(
        &mut self,
        err: &notify::Error,
        stderr: &mut dyn Write,
    ) -> miette::Result<()> {
        let key = err.to_string();
        let note = format!(
            "note: watch error: {err}\nhelp: missing watch targets are dropped and re-watched automatically when they reappear or the deck frontmatter changes; if this error persists, check file watcher permissions"
        );
        write_suppressed_watch_note(&key, &note, stderr, &mut self.emitted_watch_error_notes)?;
        Ok(())
    }
}

struct WatchRuntime {
    state: WatchState,
    debouncer: Debouncer<PollWatcher>,
    rx: mpsc::Receiver<DebounceEventResult>,
}

impl WatchTargets {
    /// The deck file plus the resolved asset paths. Each asset path may be a
    /// single file or a directory whose matching extension files are watched.
    fn new(input: PathBuf, assets: ResolvedAssets) -> Self {
        let mut roots = vec![WatchRoot {
            path: input,
            ext: Some("md"),
        }];
        if let Some(path) = assets.layouts.path() {
            roots.push(WatchRoot {
                path: path.to_path_buf(),
                ext: Some("html"),
            });
        }
        if let Some(path) = assets.css.path() {
            roots.push(WatchRoot {
                path: path.to_path_buf(),
                ext: Some("css"),
            });
        }
        if let Some(path) = assets.syntaxes.path() {
            roots.push(WatchRoot {
                path: path.to_path_buf(),
                ext: Some("sublime-syntax"),
            });
        }
        if let Some(path) = assets.fonts.path() {
            roots.push(WatchRoot {
                path: path.to_path_buf(),
                ext: None,
            });
        }
        Self { roots, assets }
    }

    fn is_relevant_change(&self, changed: &Path) -> bool {
        self.roots.iter().any(|root| {
            if same_watch_path(&root.path, changed) {
                return true;
            }
            let matches_root_filter = match root.ext {
                Some(ext) => {
                    changed.extension().and_then(|e| e.to_str()) == Some(ext)
                        && changed
                            .parent()
                            .is_some_and(|parent| same_watch_path(&root.path, parent))
                }
                None => {
                    changed.starts_with(&root.path)
                        && !has_hidden_relative_component(&root.path, changed)
                        && changed
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_none_or(|name| !name.starts_with('.'))
                }
            };
            root.path.is_dir() && matches_root_filter
        })
    }

    fn watch_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for root in &self.roots {
            if root.path.is_dir() {
                if root.ext.is_none() {
                    collect_watch_tree(&root.path, &mut dirs);
                } else {
                    push_watch_dir(&mut dirs, root.path.clone());
                }
            } else {
                push_watch_dir(&mut dirs, parent_dir_for_watch(&root.path));
            }
        }
        dirs
    }
}

fn collect_watch_tree(root: &Path, dirs: &mut Vec<PathBuf>) {
    push_watch_dir(dirs, root.to_path_buf());
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut child_dirs = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            if name.to_str().is_some_and(|name| name.starts_with('.')) {
                return None;
            }
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    child_dirs.sort();
    for child in child_dirs {
        collect_watch_tree(&child, dirs);
    }
}

fn has_hidden_relative_component(root: &Path, changed: &Path) -> bool {
    changed.strip_prefix(root).ok().is_some_and(|relative| {
        relative.components().any(|component| {
            matches!(
                component,
                Component::Normal(name)
                    if name.to_str().is_some_and(|name| name.starts_with('.'))
            )
        })
    })
}

fn push_watch_dir(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dirs.iter().any(|existing| same_watch_path(existing, &dir)) {
        dirs.push(dir);
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
        let metadata = fs::metadata(dir)
            .map_err(|err| miette::miette!("directory does not exist or cannot be read: {err}"))?;
        if !metadata.is_dir() {
            return Err(miette::miette!("path is not a directory"));
        }
        self.watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|err| miette::miette!("{err}"))
    }

    fn unwatch_dir(&mut self, dir: &Path) -> miette::Result<()> {
        self.watcher
            .unwatch(dir)
            .map_err(|err| miette::miette!("{err}"))
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
    host: Option<IpAddr>,
}

struct PreviewOptions {
    input: PathBuf,
    port: u16,
    no_open: bool,
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
    New {
        #[arg(default_value = ".")]
        dir: PathBuf,
        #[arg(long, value_enum, default_value_t = new_cmd::LayoutVariant::Default)]
        layouts: new_cmd::LayoutVariant,
        #[arg(long, value_enum, default_value_t = new_cmd::ThemeVariant::Light)]
        theme: new_cmd::ThemeVariant,
        #[arg(
            long,
            help = "overwrite the scaffold-owned files (deck.md, layouts/, css/base.css, .gitignore) in a non-empty directory"
        )]
        force: bool,
    },
    Build {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(long, default_value = "dist")]
        out: PathBuf,
        #[arg(long)]
        watch: bool,
    },
    Lint {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
    },
    Layouts {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(long)]
        explain: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Preview {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(long, default_value_t = 0)]
        port: u16,
        #[arg(long)]
        no_open: bool,
    },
    Present {
        #[arg(default_value = "deck.md")]
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
        #[arg(long, value_name = "IP")]
        host: Option<IpAddr>,
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
    Completions {
        shell: Shell,
    },
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    Pdf {
        #[arg(default_value = "deck.md")]
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
const BUILTIN_PREVIEW_JS: &str = include_str!("../../../packages/peitho-present/dist/preview.js");
const BUILTIN_REMOTE_JS: &str = include_str!("../../../packages/peitho-present/dist/remote.js");

const PRESENT_CACHE: &str = ".peitho/present-cache";
const PREVIEW_CACHE: &str = ".peitho/preview-cache";
const PRESENTATION_ONLY_DIST_FILES: &[&str] = &[
    "present.html",
    "presenter.html",
    "remote.html",
    "notes.json",
    "shell.js",
    "remote.js",
];

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New {
            dir,
            layouts,
            theme,
            force,
        } => new_cmd::run(
            new_cmd::NewOptions {
                target: dir,
                layouts,
                theme,
                force,
            },
            &mut std::io::stdout(),
        ),
        Command::Build { input, out, watch } => {
            let options = BuildOptions { input, out };
            if watch {
                watch_build(options)
            } else {
                build(&options)
            }
        }
        Command::Lint { input } => {
            let mut stdout = std::io::stdout();
            let code = lint::run(input, &mut stdout)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Command::Layouts {
            input,
            explain,
            json,
        } => cmd_layouts(input, explain, json),
        Command::Doctor { input, json } => {
            let env = doctor::DoctorEnv::from_process_env();
            let mut stdout = std::io::stdout();
            let is_terminal = stdout.is_terminal();
            let code = doctor::dispatch(input, json, &env, &mut stdout, is_terminal)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Command::Preview {
            input,
            port,
            no_open,
        } => preview(PreviewOptions {
            input,
            port,
            no_open,
        }),
        Command::Present {
            input,
            shell,
            port,
            no_open,
            no_serve,
            no_presenter,
            presenter_windowed,
            host,
        } => present(PresentOptions {
            input,
            shell,
            port,
            no_open,
            no_serve,
            no_presenter,
            presenter_windowed,
            host,
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
        },
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
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
    if let Err(err) = run_chrome_print(&chrome, tmp.path(), &out) {
        return Err(keep_workspace_for_error(tmp, err));
    }
    println!(
        "exported {} slide(s) to {}",
        artifacts.slide_count,
        out.display()
    );
    Ok(())
}

fn cmd_layouts(input: PathBuf, explain: Option<String>, json: bool) -> miette::Result<()> {
    let markdown = fs::read_to_string(&input).map_err(|err| {
        miette::miette!(
            "failed to read {}\nhelp: the deck argument defaults to deck.md in the current directory when omitted; pass the deck path explicitly if it lives elsewhere\ncaused by: {err}",
            input.display()
        )
    })?;
    let frontmatter = core(peitho_core::parse_frontmatter(&markdown))?;
    let assets = resolve_assets(&input, &frontmatter)?;
    let layouts = load_layouts(assets.layouts.path())?;

    let Some(key) = explain else {
        if json {
            print_layouts_json(&assets.layouts, &layouts)?;
        } else {
            print_layouts_human(&assets.layouts, &layouts);
        }
        return Ok(());
    };

    let highlighter = load_highlighter(assets.syntaxes.path())?;
    let parsed = core(peitho_core::code_images::parse_deck_and_transform(
        &markdown,
        frontmatter,
        &highlighter,
        &CliSvgRunner::for_deck(&input),
        &code_images_cache_dir(&input),
    ))?;
    let Some(slide) = parsed
        .parsed_slides()
        .iter()
        .find(|slide| slide.key.as_str() == key)
    else {
        let known_keys = parsed
            .parsed_slides()
            .iter()
            .map(|slide| slide.key.as_str())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let message = format!("slide key '{key}' not found in {}", input.display());
        if json {
            let payload = UnknownSlideKeyJson {
                error: "slide-key-not-found",
                key: key.clone(),
                known_keys,
                message,
            };
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&payload).into_diagnostic()?
            );
            std::process::exit(2);
        }
        let err = peitho_core::BuildError::new(
            peitho_core::error::ErrorKind::Parse,
            None,
            message,
            format!("known keys: {}", known_keys.join(", ")),
        );
        eprintln!("{err}");
        std::process::exit(2);
    };
    let trace = peitho_core::explain_dispatch(slide, &layouts);
    if json {
        print_explain_json(&assets.layouts, slide, &trace)?;
    } else {
        print_explain_human(&assets.layouts, slide, &trace);
    }
    if matches!(trace.result(), peitho_core::DispatchResult::Matched(_)) {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

#[derive(Serialize)]
struct SourceJson {
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

#[derive(Serialize)]
struct LayoutsJson {
    source: SourceJson,
    layouts: Vec<LayoutJson>,
}

#[derive(Serialize)]
struct LayoutJson {
    name: String,
    slots: Vec<SlotJson>,
}

#[derive(Serialize)]
struct SlotJson {
    name: String,
    accepts: String,
    arity: String,
}

#[derive(Serialize)]
struct ExplainJson {
    source: SourceJson,
    slide: SlideJson,
    dispatch: DispatchJson,
}

#[derive(Serialize)]
struct SlideJson {
    key: String,
    index: usize,
}

#[derive(Serialize)]
struct UnknownSlideKeyJson {
    error: &'static str,
    key: String,
    known_keys: Vec<String>,
    message: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum DispatchJson {
    Explicit {
        layout: String,
        line: usize,
        result: DispatchResultJson,
    },
    SoleLayout {
        layout: String,
        result: DispatchResultJson,
    },
    StructuralMatch {
        candidates: Vec<CandidateJson>,
        result: DispatchResultJson,
    },
}

#[derive(Serialize)]
struct CandidateJson {
    layout: String,
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum DispatchResultJson {
    Matched(String),
    Failure(DispatchFailureJson),
}

#[derive(Serialize)]
struct DispatchFailureJson {
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layouts: Option<Vec<String>>,
}

fn print_layouts_human(source: &Provenance, layouts: &peitho_core::Layouts) {
    println!("layouts source: {}", provenance_human(source));
    for summary in peitho_core::describe_layouts(layouts) {
        println!();
        println!("{}", summary.name);
        println!("  slots:");
        let width = summary
            .slots
            .iter()
            .map(|slot| slot.name.len())
            .max()
            .unwrap_or(0);
        for slot in summary.slots {
            println!(
                "    - {:width$}  accepts={} arity={}",
                slot.name,
                slot.accepts,
                slot.arity,
                width = width
            );
        }
    }
}

fn print_layouts_json(source: &Provenance, layouts: &peitho_core::Layouts) -> miette::Result<()> {
    let payload = LayoutsJson {
        source: source_json(source),
        layouts: peitho_core::describe_layouts(layouts)
            .into_iter()
            .map(layout_json)
            .collect(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).into_diagnostic()?
    );
    Ok(())
}

fn print_explain_human(
    source: &Provenance,
    slide: &peitho_core::phase::ParsedSlide,
    trace: &peitho_core::DispatchTrace,
) {
    println!("layouts source: {}", provenance_human(source));
    println!("slide: {} (index {})", slide.key.as_str(), slide.index);
    println!();
    match trace {
        peitho_core::DispatchTrace::Explicit {
            layout,
            line,
            result,
        } => {
            println!("dispatch: explicit layout request");
            println!("  requested: {layout} (line {line})");
            println!("  result: {}", dispatch_result_human(result));
            print_no_match_reason(result);
        }
        peitho_core::DispatchTrace::SoleLayout { layout, result } => {
            println!("dispatch: sole layout");
            println!("  layout: {layout}");
            println!("  result: {}", dispatch_result_human(result));
            print_no_match_reason(result);
        }
        peitho_core::DispatchTrace::StructuralMatch { candidates, result } => {
            println!("dispatch: structural match");
            println!("  candidates:");
            let width = candidates
                .iter()
                .map(|candidate| candidate.layout.len())
                .max()
                .unwrap_or(0);
            for candidate in candidates {
                match &candidate.outcome {
                    peitho_core::CandidateOutcome::Matched => {
                        println!("    - {:width$}  matched", candidate.layout, width = width);
                    }
                    peitho_core::CandidateOutcome::Rejected { reason } => {
                        println!(
                            "    - {:width$}  rejected: {reason}",
                            candidate.layout,
                            width = width
                        );
                    }
                }
            }
            println!("  result: {}", dispatch_result_human(result));
        }
    }
}

fn print_explain_json(
    source: &Provenance,
    slide: &peitho_core::phase::ParsedSlide,
    trace: &peitho_core::DispatchTrace,
) -> miette::Result<()> {
    let payload = ExplainJson {
        source: source_json(source),
        slide: SlideJson {
            key: slide.key.as_str().to_owned(),
            index: slide.index,
        },
        dispatch: dispatch_json(trace),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).into_diagnostic()?
    );
    Ok(())
}

fn source_json(source: &Provenance) -> SourceJson {
    SourceJson {
        kind: source.kind().to_owned(),
        path: source.path().map(|path| path.display().to_string()),
    }
}

fn layout_json(summary: peitho_core::LayoutSummary) -> LayoutJson {
    LayoutJson {
        name: summary.name,
        slots: summary
            .slots
            .into_iter()
            .map(|slot| SlotJson {
                name: slot.name,
                accepts: slot.accepts,
                arity: slot.arity,
            })
            .collect(),
    }
}

fn dispatch_json(trace: &peitho_core::DispatchTrace) -> DispatchJson {
    match trace {
        peitho_core::DispatchTrace::Explicit {
            layout,
            line,
            result,
        } => DispatchJson::Explicit {
            layout: layout.clone(),
            line: *line,
            result: dispatch_result_json(result),
        },
        peitho_core::DispatchTrace::SoleLayout { layout, result } => DispatchJson::SoleLayout {
            layout: layout.clone(),
            result: dispatch_result_json(result),
        },
        peitho_core::DispatchTrace::StructuralMatch { candidates, result } => {
            DispatchJson::StructuralMatch {
                candidates: candidates
                    .iter()
                    .map(|candidate| match &candidate.outcome {
                        peitho_core::CandidateOutcome::Matched => CandidateJson {
                            layout: candidate.layout.clone(),
                            outcome: "matched".to_owned(),
                            reason: None,
                        },
                        peitho_core::CandidateOutcome::Rejected { reason } => CandidateJson {
                            layout: candidate.layout.clone(),
                            outcome: "rejected".to_owned(),
                            reason: Some(reason.clone()),
                        },
                    })
                    .collect(),
                result: dispatch_result_json(result),
            }
        }
    }
}

fn dispatch_result_json(result: &peitho_core::DispatchResult) -> DispatchResultJson {
    match result {
        peitho_core::DispatchResult::Matched(layout) => DispatchResultJson::Matched(layout.clone()),
        peitho_core::DispatchResult::NoMatch { reason } => {
            DispatchResultJson::Failure(DispatchFailureJson {
                kind: "no-match".to_owned(),
                reason: reason.clone(),
                layout: None,
                layouts: None,
            })
        }
        peitho_core::DispatchResult::Ambiguous(layouts) => {
            DispatchResultJson::Failure(DispatchFailureJson {
                kind: "ambiguous".to_owned(),
                reason: None,
                layout: None,
                layouts: Some(layouts.clone()),
            })
        }
        peitho_core::DispatchResult::UnknownLayout(layout) => {
            DispatchResultJson::Failure(DispatchFailureJson {
                kind: "unknown-layout".to_owned(),
                reason: None,
                layout: Some(layout.clone()),
                layouts: None,
            })
        }
    }
}

fn dispatch_result_human(result: &peitho_core::DispatchResult) -> String {
    match result {
        peitho_core::DispatchResult::Matched(layout) => layout.clone(),
        peitho_core::DispatchResult::NoMatch { .. } => "no match".to_owned(),
        peitho_core::DispatchResult::Ambiguous(layouts) => {
            format!("ambiguous: {}", layouts.join(", "))
        }
        peitho_core::DispatchResult::UnknownLayout(layout) => {
            format!("unknown layout: {layout}")
        }
    }
}

fn print_no_match_reason(result: &peitho_core::DispatchResult) {
    if let peitho_core::DispatchResult::NoMatch {
        reason: Some(reason),
    } = result
    {
        println!("  reason: {reason}");
    }
}

fn provenance_human(source: &Provenance) -> String {
    match source {
        Provenance::Explicit(path) => format!("explicit ({})", path.display()),
        Provenance::DeckAdjacent(path) => format!("deck-adjacent ({})", path.display()),
        Provenance::Builtin => "built-in".to_owned(),
    }
}

fn keep_workspace_for_error(tmp: tempfile::TempDir, err: impl std::fmt::Display) -> miette::Report {
    let kept = tmp.keep();
    miette::miette!("{err}\nhelp: workspace kept at {}", kept.display())
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

fn handle_watch_paths_with_rebuild<F>(
    state: &mut WatchState,
    watcher: &mut dyn WatchController,
    changed_paths: &[PathBuf],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    mut rebuild: F,
) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    let relevant = changed_paths
        .iter()
        .any(|changed| state.targets.is_relevant_change(changed));

    if relevant
        && changed_paths
            .iter()
            .any(|changed| same_watch_path(&state.input, changed))
    {
        refresh_watch_targets_after_deck_change(state, stderr)?;
    }

    let watch_set_changed = state.reconcile_after_events(watcher, stderr)?;
    if relevant || watch_set_changed {
        rebuild(stdout, stderr)?;
    }
    Ok(())
}

fn watch_build(options: BuildOptions) -> miette::Result<()> {
    let (runtime, ()) = run_after_watch_registration(&options.input, prepare_watch_loop, || {
        println!("watching deck and resolved asset paths");
        rebuild_once_for_watch(&options, &mut std::io::stdout(), &mut std::io::stderr())
    })?;
    watch_paths_loop(runtime, move |stdout, stderr| {
        rebuild_once_for_watch(&options, stdout, stderr)
    })
}

fn run_after_watch_registration<W, T, P, A>(
    input: &Path,
    prepare_watch: P,
    action: A,
) -> miette::Result<(W, T)>
where
    P: FnOnce(PathBuf) -> miette::Result<W>,
    A: FnOnce() -> miette::Result<T>,
{
    let watch = prepare_watch(input.to_path_buf())?;
    let value = action()?;
    Ok((watch, value))
}

fn watch_paths_loop<F>(mut runtime: WatchRuntime, mut rebuild: F) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    while let Ok(result) = runtime.rx.recv() {
        let mut watcher = NotifyWatchController::new(runtime.debouncer.watcher());
        handle_watch_event_result(
            result,
            &mut runtime.state,
            &mut watcher,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
            &mut rebuild,
        )?;
    }

    Ok(())
}

fn prepare_watch_loop(input: PathBuf) -> miette::Result<WatchRuntime> {
    let targets = resolve_watch_targets_or_deck_only(&input);
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
        let watched_dirs = register_watch_target_dirs(&targets, &mut watcher)?;
        let state = WatchState::new(input, targets, watched_dirs);

        Ok(WatchRuntime {
            state,
            debouncer,
            rx,
        })
    }
}

fn register_watch_target_dirs(
    targets: &WatchTargets,
    watcher: &mut dyn WatchController,
) -> miette::Result<Vec<PathBuf>> {
    let dirs = targets.watch_dirs();
    watch_all_dirs(watcher, &dirs)?;
    Ok(dirs)
}

fn handle_watch_event_result<F>(
    result: DebounceEventResult,
    state: &mut WatchState,
    watcher: &mut dyn WatchController,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    mut rebuild: F,
) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    match result {
        Ok(events) => {
            let paths = events
                .into_iter()
                .map(|event| event.path)
                .collect::<Vec<_>>();
            handle_watch_paths_with_rebuild(state, watcher, &paths, stdout, stderr, rebuild)
        }
        Err(err) => {
            let watch_set_changed = state.reconcile_after_error(watcher, stderr)?;
            state.write_watch_error_note(&err, stderr)?;
            if watch_set_changed {
                rebuild(stdout, stderr)?;
            }
            Ok(())
        }
    }
}

fn resolve_watch_targets(input: &Path) -> miette::Result<WatchTargets> {
    let assets = resolve_deck_assets(input)?;
    Ok(WatchTargets::new(input.to_path_buf(), assets))
}

fn resolve_watch_targets_or_deck_only(input: &Path) -> WatchTargets {
    resolve_watch_targets(input).unwrap_or_else(|_| deck_only_watch_targets(input))
}

fn deck_only_watch_targets(input: &Path) -> WatchTargets {
    WatchTargets::new(
        input.to_path_buf(),
        ResolvedAssets {
            layouts: Provenance::Builtin,
            css: Provenance::Builtin,
            syntaxes: Provenance::Builtin,
            fonts: Provenance::Builtin,
        },
    )
}

fn resolve_deck_assets(input: &Path) -> miette::Result<ResolvedAssets> {
    let markdown = fs::read_to_string(input).into_diagnostic()?;
    let frontmatter = core(peitho_core::parse_frontmatter(&markdown))?;
    resolve_assets(input, &frontmatter)
}

fn refresh_watch_targets_after_deck_change(
    state: &mut WatchState,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    let current_assets = match resolve_deck_assets(&state.input) {
        Ok(assets) => assets,
        Err(_) => {
            return Ok(());
        }
    };
    let paths_changed = resolved_asset_paths_changed(&state.targets.assets, &current_assets);
    let next_targets = WatchTargets::new(state.input.clone(), current_assets);
    state.targets = next_targets;
    if !paths_changed {
        return Ok(());
    }
    writeln!(
        stderr,
        "note: watching new asset paths from frontmatter: {}",
        describe_resolved_assets(&state.targets.assets)
    )
    .into_diagnostic()?;
    stderr.flush().into_diagnostic()?;
    Ok(())
}

fn resolved_asset_paths_changed(old: &ResolvedAssets, new: &ResolvedAssets) -> bool {
    old.layouts.path() != new.layouts.path()
        || old.css.path() != new.css.path()
        || old.syntaxes.path() != new.syntaxes.path()
        || old.fonts.path() != new.fonts.path()
}

fn watch_all_dirs(watcher: &mut dyn WatchController, dirs: &[PathBuf]) -> miette::Result<()> {
    for dir in dirs {
        watcher.watch_dir(dir).map_err(|err| {
            miette::miette!(
                "failed to watch {}\nhelp: verify the watched directories exist and are readable before starting --watch\ncaused by: {err}",
                dir.display()
            )
        })?;
    }
    Ok(())
}

fn watch_path_key(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

struct ReconcileResult {
    changed: bool,
    had_failures: bool,
}

fn reconcile_watched_dirs(
    watcher: &mut dyn WatchController,
    watched_dirs: &mut Vec<PathBuf>,
    desired_dirs: &[PathBuf],
    stderr: &mut dyn Write,
    emitted_watch_error_notes: &mut HashSet<String>,
) -> miette::Result<ReconcileResult> {
    let previous_dirs = watched_dirs
        .iter()
        .map(|path| (watch_path_key(path), path.clone()))
        .collect::<Vec<_>>();
    let desired_dirs = desired_dirs
        .iter()
        .map(|path| (watch_path_key(path), path.clone()))
        .collect::<Vec<_>>();
    let previous_keys = previous_dirs
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<HashSet<_>>();
    let desired_keys = desired_dirs
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<HashSet<_>>();
    let mut next_watched_dirs = Vec::new();
    let mut next_keys = HashSet::new();
    let mut had_failures = false;

    for (key, old) in previous_dirs {
        if !desired_keys.contains(&key) {
            if let Err(err) = watcher.unwatch_dir(&old) {
                let note = format!("note: failed to stop watching {}: {err}", old.display());
                write_suppressed_watch_note(&note, &note, stderr, emitted_watch_error_notes)?;
                had_failures = true;
            }
        } else {
            next_keys.insert(key);
            next_watched_dirs.push(old);
        }
    }
    for (key, new) in desired_dirs {
        if !previous_keys.contains(&key) {
            if let Err(err) = watcher.watch_dir(&new) {
                let note = format!("note: failed to watch {}: {err}", new.display());
                write_suppressed_watch_note(&note, &note, stderr, emitted_watch_error_notes)?;
                had_failures = true;
            } else {
                next_keys.insert(key);
                next_watched_dirs.push(new);
            }
        }
    }

    let changed = previous_keys != next_keys;
    *watched_dirs = next_watched_dirs;
    Ok(ReconcileResult {
        changed,
        had_failures,
    })
}

fn write_suppressed_watch_note(
    key: &str,
    note: &str,
    stderr: &mut dyn Write,
    emitted_watch_error_notes: &mut HashSet<String>,
) -> miette::Result<()> {
    if !emitted_watch_error_notes.insert(key.to_owned()) {
        return Ok(());
    }
    writeln!(stderr, "{note}").into_diagnostic()?;
    stderr.flush().into_diagnostic()?;
    Ok(())
}

fn describe_resolved_assets(assets: &ResolvedAssets) -> String {
    let mut parts = Vec::new();
    if let Some(path) = assets.layouts.path() {
        parts.push(format!(
            "layouts={}({})",
            assets.layouts.kind(),
            path.display()
        ));
    }
    if let Some(path) = assets.css.path() {
        parts.push(format!("css={}({})", assets.css.kind(), path.display()));
    }
    if let Some(path) = assets.syntaxes.path() {
        parts.push(format!(
            "syntaxes={}({})",
            assets.syntaxes.kind(),
            path.display()
        ));
    }
    if let Some(path) = assets.fonts.path() {
        parts.push(format!("fonts={}({})", assets.fonts.kind(), path.display()));
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join(", ")
    }
}

fn parent_dir_for_watch(path: &Path) -> PathBuf {
    let mut candidate = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    while let Some(dir) = candidate {
        if dir.is_dir() {
            return dir.to_path_buf();
        }
        candidate = dir.parent().filter(|parent| !parent.as_os_str().is_empty());
    }
    Path::new(".").to_path_buf()
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

fn load_highlighter(
    syntaxes_path: Option<&Path>,
) -> miette::Result<peitho_core::highlight::Highlighter> {
    match syntaxes_path {
        Some(path) => {
            let files = collect_asset_files(path, "sublime-syntax")?;
            core(peitho_core::highlight::Highlighter::with_user_files(&files))
        }
        None => Ok(peitho_core::highlight::Highlighter::defaults()),
    }
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
    let markdown = fs::read_to_string(input).map_err(|err| {
        miette::miette!(
            "failed to read {}\nhelp: the deck argument defaults to deck.md in the current directory when omitted; pass the deck path explicitly if it lives elsewhere\ncaused by: {err}",
            input.display()
        )
    })?;
    let frontmatter = core(peitho_core::parse_frontmatter(&markdown))?;
    let assets = resolve_assets(input, &frontmatter)?;
    let highlighter = load_highlighter(assets.syntaxes.path())?;
    let layouts = load_layouts(assets.layouts.path())?;
    let css_files = load_css(assets.css.path())?;
    let parsed = core(peitho_core::code_images::parse_deck_and_transform(
        &markdown,
        frontmatter,
        &highlighter,
        &CliSvgRunner::for_deck(input),
        &code_images_cache_dir(input),
    ))?;
    let mapped = core(peitho_core::dispatch_by_convention(parsed, &layouts))?;
    let checked = core(peitho_core::check_deck(mapped))?;
    let slide_count = checked.slide_count();
    let css = core(peitho_core::build_theme_css(
        &css_files,
        &checked.slide_slot_classes(),
        &layouts.slot_classes(),
        &layouts.root_classes(),
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
        fonts_source: assets.fonts.path().map(Path::to_path_buf),
    })
}

fn code_images_cache_dir(input: &Path) -> PathBuf {
    asset_resolution::deck_parent(input).join(peitho_core::CODE_IMAGES_CACHE_DIR)
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

fn write_shared_assets(dir: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    fs::create_dir_all(dir).into_diagnostic()?;
    fs::write(dir.join("peitho.css"), &artifacts.css).into_diagnostic()?;
    write_image_assets(dir, &artifacts.image_assets)?;
    write_fonts_assets(dir, artifacts.fonts_source.as_deref())
}

fn write_fonts_assets(out: &Path, fonts_source: Option<&Path>) -> miette::Result<()> {
    let fonts_dir = out.join("fonts");
    if fonts_dir.exists() {
        fs::remove_dir_all(&fonts_dir).into_diagnostic()?;
    }
    let Some(fonts_source) = fonts_source else {
        return Ok(());
    };
    let file_type = fs::symlink_metadata(fonts_source)
        .into_diagnostic()?
        .file_type();
    if file_type.is_symlink() {
        return Err(miette::miette!(
            "unsupported fonts: source {} (symlink)\nhelp: point fonts: at a regular file or a directory",
            fonts_source.display()
        ));
    }

    fs::create_dir_all(&fonts_dir).into_diagnostic()?;

    if file_type.is_dir() {
        copy_dir_contents(fonts_source, &fonts_dir)
    } else if file_type.is_file() {
        let file_name = fonts_source.file_name().ok_or_else(|| {
            miette::miette!(
                "cannot copy font file without a file name: {}",
                fonts_source.display()
            )
        })?;
        fs::copy(fonts_source, fonts_dir.join(file_name)).into_diagnostic()?;
        Ok(())
    } else {
        Err(miette::miette!(
            "unsupported fonts: source {} (special file)\nhelp: point fonts: at a regular file or a directory",
            fonts_source.display()
        ))
    }
}

fn copy_dir_contents(source: &Path, destination: &Path) -> miette::Result<()> {
    fs::create_dir_all(destination).into_diagnostic()?;
    let mut entries = fs::read_dir(source)
        .into_diagnostic()?
        .collect::<std::io::Result<Vec<_>>>()
        .into_diagnostic()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().into_diagnostic()?;
        if file_type.is_dir() {
            copy_dir_contents(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).into_diagnostic()?;
        } else {
            let entry_type = if file_type.is_symlink() {
                "symlink"
            } else {
                "special file"
            };
            return Err(miette::miette!(
                "unsupported entry in fonts directory: {} ({entry_type})\nhelp: only regular files and subdirectories are supported inside fonts/",
                source_path.display()
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct ChromeLookupEnv {
    pub(crate) env_path: Option<PathBuf>,
    pub(crate) mac_chrome: PathBuf,
    pub(crate) path_dirs: Vec<PathBuf>,
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

pub(crate) fn locate_chrome_with_env(lookup: &ChromeLookupEnv) -> miette::Result<PathBuf> {
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
    LintResultLogged,
}

#[derive(Default)]
struct ChromeCompletionState {
    stderr_scanned: usize,
    signal_seen: bool,
}

impl ChromeCompletion {
    fn description(&self) -> &'static str {
        match self {
            Self::PdfWritten { .. } => "PDF output",
            Self::LintResultLogged => "lint measurement payload",
        }
    }

    fn retry_help(&self) -> &'static str {
        match self {
            Self::PdfWritten { .. } => "retry export",
            Self::LintResultLogged => {
                "retry lint; if a lint workspace was kept, inspect lint.html there"
            }
        }
    }

    fn timeout_help(&self) -> &'static str {
        match self {
            Self::PdfWritten { .. } => {
                "retry export or check the generated HTML in the workspace path reported by the export command"
            }
            Self::LintResultLogged => {
                "retry lint; if a lint workspace was kept, inspect lint.html there"
            }
        }
    }

    fn process_setup_help(&self) -> String {
        format!(
            "{}; this is an internal process setup error",
            self.retry_help()
        )
    }

    fn is_ready(&self, _stdout: &[u8], stderr: &[u8], state: &mut ChromeCompletionState) -> bool {
        match self {
            Self::PdfWritten { output_path } => {
                if !state.signal_seen {
                    state.signal_seen = scan_for_needle(
                        stderr,
                        &mut state.stderr_scanned,
                        b"bytes written to file",
                    );
                }
                output_file_is_nonempty(output_path) && state.signal_seen
            }
            Self::LintResultLogged => {
                if !state.signal_seen {
                    state.signal_seen = scan_for_needle(
                        stderr,
                        &mut state.stderr_scanned,
                        lint::PEITHO_LINT_DONE.as_bytes(),
                    );
                }
                state.signal_seen
            }
        }
    }

    fn is_ready_after_successful_exit(
        &self,
        stdout: &[u8],
        stderr: &[u8],
        state: &mut ChromeCompletionState,
    ) -> bool {
        match self {
            Self::PdfWritten { output_path } => output_file_is_nonempty(output_path),
            Self::LintResultLogged => self.is_ready(stdout, stderr, state),
        }
    }
}

fn scan_for_needle(buffer: &[u8], scanned: &mut usize, needle: &[u8]) -> bool {
    let overlap = needle.len().saturating_sub(1);
    let start = (*scanned).saturating_sub(overlap).min(buffer.len());
    let found = buffer[start..]
        .windows(needle.len())
        .any(|window| window == needle);
    *scanned = buffer.len();
    found
}

fn run_one_shot_chrome(
    chrome: &Path,
    args: &[OsString],
    completion: ChromeCompletion,
    timeout: Duration,
) -> miette::Result<ChromeOutput> {
    let mut completion_state = ChromeCompletionState::default();
    let outcome = run_child_with_timeout(chrome, args, None, timeout, |stdout, stderr| {
        completion.is_ready(stdout, stderr, &mut completion_state)
    })
    .map_err(|err| chrome_process_error(chrome, err, &completion))?;

    match outcome {
        ProcessOutcome::Ready { stdout, stderr } => Ok(chrome_output(stdout, stderr)),
        ProcessOutcome::Exited {
            status,
            stdout,
            stderr,
        } => {
            if status.success()
                && completion.is_ready_after_successful_exit(
                    &stdout,
                    &stderr,
                    &mut completion_state,
                )
            {
                return Ok(chrome_output(stdout, stderr));
            }
            if !status.success() {
                return Err(miette::miette!(
                    "Chrome failed during one-shot operation with status {}\nhelp: check that Chrome can run in headless mode\nstderr: {}",
                    status,
                    String::from_utf8_lossy(&stderr).trim()
                ));
            }
            Err(miette::miette!(
                "Chrome completed before one-shot output was ready\nhelp: expected {} before Chrome exited\nstderr: {}",
                completion.description(),
                String::from_utf8_lossy(&stderr).trim()
            ))
        }
        ProcessOutcome::TimedOut { stderr } => Err(miette::miette!(
            "Chrome timed out after {}s waiting for {}\nhelp: {}\nstderr: {}",
            timeout.as_secs(),
            completion.description(),
            completion.timeout_help(),
            String::from_utf8_lossy(&stderr).trim(),
        )),
    }
}

#[derive(Debug)]
pub(crate) struct ChromeOutput {
    #[cfg(test)]
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

fn chrome_output(stdout: Vec<u8>, stderr: Vec<u8>) -> ChromeOutput {
    #[cfg(test)]
    {
        ChromeOutput { stdout, stderr }
    }
    #[cfg(not(test))]
    {
        let _ = stdout;
        ChromeOutput { stderr }
    }
}

#[derive(Debug)]
enum ProcessOutcome {
    Ready {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Exited {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    TimedOut {
        stderr: Vec<u8>,
    },
}

#[derive(Debug)]
enum ProcessPipeEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

#[derive(Debug, Clone, Copy)]
enum ProcessPipe {
    Stdout,
    Stderr,
}

#[derive(Debug)]
enum ProcessRunError {
    Spawn(io::Error),
    CaptureStdout,
    CaptureStderr,
    CaptureStdin,
    Wait(io::Error),
    Kill(io::Error),
}

fn run_child_with_timeout<P, A, F>(
    program: P,
    args: &[A],
    stdin: Option<&[u8]>,
    timeout: Duration,
    is_complete: F,
) -> Result<ProcessOutcome, ProcessRunError>
where
    P: AsRef<OsStr>,
    A: AsRef<OsStr>,
    F: FnMut(&[u8], &[u8]) -> bool,
{
    run_child_with_timeout_in_dir(program, args, stdin, None, timeout, is_complete)
}

fn run_child_with_timeout_in_dir<P, A, F>(
    program: P,
    args: &[A],
    stdin: Option<&[u8]>,
    cwd: Option<&Path>,
    timeout: Duration,
    mut is_complete: F,
) -> Result<ProcessOutcome, ProcessRunError>
where
    P: AsRef<OsStr>,
    A: AsRef<OsStr>,
    F: FnMut(&[u8], &[u8]) -> bool,
{
    let mut command = std::process::Command::new(program.as_ref());
    for arg in args {
        command.arg(arg.as_ref());
    }
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().map_err(ProcessRunError::Spawn)?;
    let stdout_pipe = child.stdout.take().ok_or(ProcessRunError::CaptureStdout)?;
    let stderr_pipe = child.stderr.take().ok_or(ProcessRunError::CaptureStderr)?;
    let (tx, rx) = mpsc::channel();
    // Intentionally do not join these readers: lingering grandchildren can hold pipes open forever; threads exit with this process.
    let _stdout_reader = spawn_process_pipe_reader(stdout_pipe, ProcessPipe::Stdout, tx.clone());
    let _stderr_reader = spawn_process_pipe_reader(stderr_pipe, ProcessPipe::Stderr, tx);

    if let Some(input) = stdin {
        let mut stdin_pipe = child.stdin.take().ok_or(ProcessRunError::CaptureStdin)?;
        let input = input.to_vec();
        let _stdin_writer = thread::spawn(move || {
            let _ = stdin_pipe.write_all(&input);
        });
    }

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let deadline = Instant::now() + timeout;

    loop {
        if is_complete(&stdout, &stderr) {
            kill_and_reap_process_child(&mut child).map_err(ProcessRunError::Kill)?;
            drain_process_events(&rx, &mut stdout, &mut stderr);
            return Ok(ProcessOutcome::Ready { stdout, stderr });
        }

        if let Some(status) = child.try_wait().map_err(ProcessRunError::Wait)? {
            drain_process_events_for(&rx, &mut stdout, &mut stderr, Duration::from_millis(100));
            if is_complete(&stdout, &stderr) {
                return Ok(ProcessOutcome::Ready { stdout, stderr });
            }
            return Ok(ProcessOutcome::Exited {
                status,
                stdout,
                stderr,
            });
        }

        let now = Instant::now();
        if now >= deadline {
            kill_and_reap_process_child(&mut child).map_err(ProcessRunError::Kill)?;
            drain_process_events(&rx, &mut stdout, &mut stderr);
            return Ok(ProcessOutcome::TimedOut { stderr });
        }

        let remaining = deadline.saturating_duration_since(now);
        let poll = remaining.min(Duration::from_millis(25));
        let _ = receive_process_event_until(&rx, &mut stdout, &mut stderr, poll);
    }
}

fn spawn_process_pipe_reader<R>(
    mut pipe: R,
    pipe_name: ProcessPipe,
    tx: mpsc::Sender<ProcessPipeEvent>,
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
                        ProcessPipe::Stdout => ProcessPipeEvent::Stdout(bytes),
                        ProcessPipe::Stderr => ProcessPipeEvent::Stderr(bytes),
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

fn append_process_event(event: ProcessPipeEvent, stdout: &mut Vec<u8>, stderr: &mut Vec<u8>) {
    match event {
        ProcessPipeEvent::Stdout(bytes) => stdout.extend_from_slice(&bytes),
        ProcessPipeEvent::Stderr(bytes) => stderr.extend_from_slice(&bytes),
    }
}

fn drain_process_events(
    rx: &mpsc::Receiver<ProcessPipeEvent>,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) {
    while let Ok(event) = rx.try_recv() {
        append_process_event(event, stdout, stderr);
    }
}

fn drain_process_events_for(
    rx: &mpsc::Receiver<ProcessPipeEvent>,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    duration: Duration,
) {
    let deadline = Instant::now() + duration;
    loop {
        drain_process_events(rx, stdout, stderr);
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline.saturating_duration_since(now);
        let poll = remaining.min(Duration::from_millis(10));
        match receive_process_event_until(rx, stdout, stderr, poll) {
            ProcessReceive::Event | ProcessReceive::Timeout => {}
            ProcessReceive::Disconnected => return,
        }
    }
}

enum ProcessReceive {
    Event,
    Timeout,
    Disconnected,
}

fn receive_process_event_until(
    rx: &mpsc::Receiver<ProcessPipeEvent>,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    poll: Duration,
) -> ProcessReceive {
    match rx.recv_timeout(poll) {
        Ok(event) => {
            append_process_event(event, stdout, stderr);
            ProcessReceive::Event
        }
        Err(mpsc::RecvTimeoutError::Timeout) => ProcessReceive::Timeout,
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            thread::sleep(poll);
            ProcessReceive::Disconnected
        }
    }
}

fn kill_and_reap_process_child(child: &mut Child) -> io::Result<()> {
    if child.try_wait()?.is_none() {
        child.kill()?;
    }
    child.wait()?;
    Ok(())
}

fn chrome_process_error(
    chrome: &Path,
    err: ProcessRunError,
    completion: &ChromeCompletion,
) -> miette::Error {
    match err {
        ProcessRunError::CaptureStdout => miette::miette!(
            "failed to capture Chrome stdout\nhelp: {}",
            completion.process_setup_help()
        ),
        ProcessRunError::CaptureStderr => miette::miette!(
            "failed to capture Chrome stderr\nhelp: {}",
            completion.process_setup_help()
        ),
        ProcessRunError::CaptureStdin => miette::miette!(
            "failed to capture Chrome stdin\nhelp: {}",
            completion.process_setup_help()
        ),
        ProcessRunError::Spawn(err) => miette::miette!(
            "failed to run Chrome at {}\nhelp: install Google Chrome or set PEITHO_CHROME_PATH=<absolute-path>\ncaused by: {err}",
            chrome.display()
        ),
        ProcessRunError::Wait(err) => miette::miette!(
            "failed to wait on Chrome: {err}\nhelp: {}",
            completion.retry_help()
        ),
        ProcessRunError::Kill(err) => miette::miette!(
            "failed to terminate Chrome: {err}\nhelp: report the underlying io error"
        ),
    }
}

fn output_file_is_nonempty(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

fn run_code_image_command(
    command: &peitho_core::domain::CodeImageCommand,
    stdin: &str,
    timeout: Duration,
    cwd: &Path,
) -> peitho_core::Result<Vec<u8>> {
    let Some(program) = command.argv.first() else {
        return Err(code_image_runner_error(
            "code_images command has empty argv",
            "set the code_images entry to a command",
        ));
    };

    let outcome = run_child_with_timeout_in_dir(
        program,
        &command.argv[1..],
        Some(stdin.as_bytes()),
        Some(cwd),
        timeout,
        |_, _| false,
    )
    .map_err(|err| code_image_process_error(program, err))?;

    match outcome {
        ProcessOutcome::Ready { stdout, .. } => Ok(stdout),
        ProcessOutcome::Exited {
            status,
            stdout,
            stderr,
        } => {
            if status.success() {
                return Ok(stdout);
            }
            Err(code_image_runner_error(
                format!(
                    "command exited with status {status}; stderr: {}",
                    stderr_excerpt(&stderr)
                ),
                "fix the code_images command or the fenced code block input",
            ))
        }
        ProcessOutcome::TimedOut { stderr } => Err(code_image_runner_error(
            format!(
                "command timed out after {}s; stderr: {}",
                timeout.as_secs(),
                stderr_excerpt(&stderr)
            ),
            "make the code_images command finish within 30 seconds",
        )),
    }
}

fn stderr_excerpt(stderr: &[u8]) -> String {
    let excerpt = String::from_utf8_lossy(stderr)
        .trim()
        .chars()
        .take(200)
        .collect::<String>();
    if excerpt.is_empty() {
        "(empty)".to_owned()
    } else {
        excerpt
    }
}

fn code_image_process_error(program: &str, err: ProcessRunError) -> peitho_core::BuildError {
    match err {
        ProcessRunError::Spawn(err) => code_image_runner_error(
            format!("failed to start command '{program}': {err}"),
            "install the command or fix the code_images frontmatter",
        ),
        ProcessRunError::CaptureStdout => code_image_runner_error(
            "failed to capture command stdout",
            "retry the build; this is an internal process setup error",
        ),
        ProcessRunError::CaptureStderr => code_image_runner_error(
            "failed to capture command stderr",
            "retry the build; this is an internal process setup error",
        ),
        ProcessRunError::CaptureStdin => code_image_runner_error(
            "failed to capture command stdin",
            "retry the build; this is an internal process setup error",
        ),
        ProcessRunError::Wait(err) => code_image_runner_error(
            format!("failed to wait for command '{program}': {err}"),
            "retry the build or check the code_images command",
        ),
        ProcessRunError::Kill(err) => code_image_runner_error(
            format!("failed to kill timed-out command '{program}': {err}"),
            "stop the hung code_images command and retry the build",
        ),
    }
}

fn code_image_runner_error(
    message: impl Into<String>,
    help: impl Into<String>,
) -> peitho_core::BuildError {
    peitho_core::BuildError::new(peitho_core::error::ErrorKind::Asset, None, message, help)
}

fn chrome_print_args(profile: &Path, out: &Path, url: &str) -> Vec<OsString> {
    vec![
        OsString::from("--headless=new"),
        OsString::from("--disable-gpu"),
        OsString::from("--no-sandbox"),
        OsString::from("--no-pdf-header-footer"),
        OsString::from("--virtual-time-budget=10000"),
        OsString::from(format!("--user-data-dir={}", profile.display())),
        OsString::from(format!("--print-to-pdf={}", out.display())),
        OsString::from(url),
    ]
}

fn run_chrome_print(chrome: &Path, workspace: &Path, out: &Path) -> miette::Result<()> {
    let abs_out = absolute_path_for_output(out)?;
    let profile = workspace.join("chrome-profile");
    fs::create_dir_all(&profile).into_diagnostic()?;
    let pdf_html = workspace.join("pdf.html");
    let url = file_url(&pdf_html)?;
    let args = chrome_print_args(&profile, &abs_out, &url);
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
    validate_present_options(&options)?;

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

    let server = server::PresentServer::bind_with_host(
        cache.clone(),
        options.port,
        "present.html",
        options.host,
    )?;
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
    if let Some(host) = options.host {
        let port = server.addr().port();
        let target = remote_control_target_for_host(host, port);
        let (lines, qr_url) = remote_control_output(&target);
        for line in lines {
            println!("{line}");
        }
        if let Some(url) = qr_url {
            match peitho::qr::qr_unicode_lines(url.as_str()) {
                Ok(lines) => {
                    println!();
                    for line in lines {
                        println!("{line}");
                    }
                }
                Err(err) => {
                    eprintln!("warning: failed to render remote control QR for {url}: {err}");
                }
            }
        }
    }
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

fn validate_present_options(options: &PresentOptions) -> miette::Result<()> {
    let Some(host) = options.host else {
        return Ok(());
    };
    if options.no_serve {
        return Err(miette::miette!(
            "--host requires the present server\nhelp: remove --no-serve or omit --host"
        ));
    }
    if host.is_loopback() {
        return Err(miette::miette!(
            "--host must be non-loopback\nhelp: use the default loopback server without --host, or pass a reachable Tailscale/LAN IP address"
        ));
    }
    Ok(())
}

enum RemoteControlTarget {
    Specific(peitho::remote_url::RemoteUrl),
    Candidates(Vec<peitho::remote_url::RemoteUrlCandidate>),
}

fn remote_control_target_for_host(host: IpAddr, port: u16) -> RemoteControlTarget {
    if host.is_unspecified() {
        RemoteControlTarget::Candidates(remote_url_candidates_from_interfaces(port, host))
    } else {
        RemoteControlTarget::Specific(peitho::remote_url::remote_url_for_addr(host, port))
    }
}

fn remote_control_output(
    target: &RemoteControlTarget,
) -> (Vec<String>, Option<&peitho::remote_url::RemoteUrl>) {
    match target {
        RemoteControlTarget::Specific(url) => (vec![format!("remote control: {url}")], Some(url)),
        RemoteControlTarget::Candidates(candidates) if candidates.is_empty() => (
            vec!["remote control: no non-loopback network addresses found".to_owned()],
            None,
        ),
        RemoteControlTarget::Candidates(candidates) => {
            let mut lines = Vec::with_capacity(candidates.len());
            let mut qr_url = None;
            for candidate in candidates {
                if qr_url.is_none() {
                    qr_url = Some(&candidate.url);
                }
                lines.push(match candidate.label {
                    Some(label) => {
                        format!("remote control ({}): {}", label.as_str(), candidate.url)
                    }
                    None => format!("remote control: {}", candidate.url),
                });
            }
            (lines, qr_url)
        }
    }
}

fn remote_url_candidates_from_interfaces(
    port: u16,
    bound_wildcard: IpAddr,
) -> Vec<peitho::remote_url::RemoteUrlCandidate> {
    let addrs = match if_addrs::get_if_addrs() {
        Ok(addrs) => addrs.into_iter().map(|addr| addr.ip()).collect::<Vec<_>>(),
        Err(err) => {
            eprintln!("warning: failed to enumerate network interfaces for remote URL: {err}");
            Vec::new()
        }
    };
    peitho::remote_url::remote_url_candidates(
        &addrs,
        default_route_ipv4(),
        port,
        Some(bound_wildcard),
    )
}

fn default_route_ipv4() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

fn preview(options: PreviewOptions) -> miette::Result<()> {
    let cache = PathBuf::from(PREVIEW_CACHE);
    let (watch, root) = run_after_watch_registration(&options.input, prepare_watch_loop, || {
        emit_initial_preview_root(&options.input, &cache, &mut std::io::stderr())
    })?;

    let server = server::PresentServer::bind(root, options.port, "index.html")?;
    let url = server.preview_url();
    let _watch = spawn_preview_watch(watch, cache, server.clone());
    println!("serving preview at {url}");
    std::io::stdout().flush().into_diagnostic()?;
    if !options.no_open {
        open_preview_browser_or_warn(&url, &mut std::io::stderr(), open_default_browser)?;
    }
    server.serve_forever()
}

fn spawn_preview_watch(
    watch: WatchRuntime,
    cache: PathBuf,
    server: server::PresentServer,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if let Err(err) = preview_watch_thread_result(|| watch_preview(watch, cache, server)) {
            eprintln!("preview watch error: {err}");
            std::process::exit(1);
        }
    })
}

fn preview_watch_thread_result<F>(run_watch: F) -> std::result::Result<(), String>
where
    F: FnOnce() -> miette::Result<()>,
{
    // AssertUnwindSafe is acceptable here because the process exits immediately after a caught panic, so no state is observed post-panic.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run_watch)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(payload) => Err(format!(
            "preview watch panicked: {}",
            panic_payload_message(payload.as_ref())
        )),
    }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_owned()
    }
}

fn watch_preview(
    watch: WatchRuntime,
    cache: PathBuf,
    server: server::PresentServer,
) -> miette::Result<()> {
    let rebuild_input = watch.state.input.clone();
    watch_paths_loop(watch, move |stdout, stderr| {
        rebuild_preview_once_for_watch(&rebuild_input, &cache, &server, stdout, stderr)
    })
}

trait PreviewReloadTarget {
    fn generation(&self) -> u64;
    fn swap_root(&self, root: PathBuf);
    fn broadcast_reload(&self) -> u64;
}

impl PreviewReloadTarget for server::PresentServer {
    fn generation(&self) -> u64 {
        server::PresentServer::generation(self)
    }

    fn swap_root(&self, root: PathBuf) {
        server::PresentServer::swap_root(self, root);
    }

    fn broadcast_reload(&self) -> u64 {
        server::PresentServer::broadcast_reload(self)
    }
}

fn emit_initial_preview_root(
    input: &Path,
    cache: &Path,
    stderr: &mut dyn Write,
) -> miette::Result<PathBuf> {
    match build_artifacts(input) {
        Ok(artifacts) => {
            let root = emit_preview_cache_generation(cache, 0, &artifacts)?;
            prune_preview_cache_generations(cache, 0)?;
            Ok(root)
        }
        Err(err) => {
            writeln!(stderr, "build failed: {err}").into_diagnostic()?;
            stderr.flush().into_diagnostic()?;
            let root = emit_preview_error_page(cache, 0, &err.to_string())?;
            prune_preview_cache_generations(cache, 0)?;
            Ok(root)
        }
    }
}

fn rebuild_preview_once_for_watch(
    input: &Path,
    cache: &Path,
    server: &impl PreviewReloadTarget,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    match build_artifacts(input).and_then(|artifacts| {
        let generation = server.generation() + 1;
        let slide_count = artifacts.slide_count;
        let root = emit_preview_cache_generation(cache, generation, &artifacts)?;
        server.swap_root(root);
        server.broadcast_reload();
        prune_preview_cache_generations(cache, generation)?;
        Ok(slide_count)
    }) {
        Ok(slide_count) => {
            writeln!(
                stdout,
                "rebuilt {slide_count} slide(s) into {}",
                cache.display()
            )
            .into_diagnostic()?;
            stdout.flush().into_diagnostic()?;
        }
        Err(err) => {
            writeln!(stderr, "build failed: {err}").into_diagnostic()?;
            stderr.flush().into_diagnostic()?;
        }
    }

    Ok(())
}

fn open_default_browser(url: &str) -> miette::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "linux") {
        "xdg-open"
    } else {
        return Err(miette::miette!(
            "cannot open preview browser on this platform\nhelp: pass --no-open and open {url} manually"
        ));
    };
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|err| {
            miette::miette!(
                "failed to open preview browser with {program}\nhelp: pass --no-open and open {url} manually\ncaused by: {err}"
            )
        })
}

fn open_preview_browser_or_warn<F>(url: &str, stderr: &mut dyn Write, open: F) -> miette::Result<()>
where
    F: FnOnce(&str) -> miette::Result<()>,
{
    if let Err(err) = open(url) {
        writeln!(stderr, "warning: failed to open preview browser: {err}").into_diagnostic()?;
        writeln!(stderr, "help: open {url} manually").into_diagnostic()?;
        stderr.flush().into_diagnostic()?;
    }
    Ok(())
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
    write_fonts_assets(cache, artifacts.fonts_source.as_deref())?;
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
    fs::write(
        cache.join("remote.html"),
        peitho_core::render_remote_index(artifacts.rendered.settings().aspect_ratio()),
    )
    .into_diagnostic()?;
    fs::write(cache.join("remote.js"), BUILTIN_REMOTE_JS).into_diagnostic()?;
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

fn emit_preview_cache_generation(
    cache: &Path,
    generation: u64,
    artifacts: &BuildArtifacts,
) -> miette::Result<PathBuf> {
    fs::create_dir_all(cache).into_diagnostic()?;
    let generation_dir = preview_generation_dir(cache, generation);
    if generation_dir.exists() {
        fs::remove_dir_all(&generation_dir).into_diagnostic()?;
    }
    fs::create_dir_all(&generation_dir).into_diagnostic()?;
    write_shared_assets(&generation_dir, artifacts)?;
    write_slide_fragments(&generation_dir, &artifacts.rendered)?;
    fs::write(
        generation_dir.join("manifest.json"),
        &artifacts.manifest_json,
    )
    .into_diagnostic()?;
    fs::write(
        generation_dir.join("index.html"),
        peitho_core::render_preview_index(artifacts.rendered.settings().aspect_ratio()),
    )
    .into_diagnostic()?;
    fs::write(generation_dir.join("preview.js"), BUILTIN_PREVIEW_JS).into_diagnostic()?;
    Ok(generation_dir)
}

fn emit_preview_error_page(cache: &Path, generation: u64, error: &str) -> miette::Result<PathBuf> {
    fs::create_dir_all(cache).into_diagnostic()?;
    let generation_dir = preview_generation_dir(cache, generation);
    if generation_dir.exists() {
        fs::remove_dir_all(&generation_dir).into_diagnostic()?;
    }
    fs::create_dir_all(&generation_dir).into_diagnostic()?;
    fs::write(
        generation_dir.join("index.html"),
        peitho_core::render_preview_error_index(generation, error),
    )
    .into_diagnostic()?;
    Ok(generation_dir)
}

fn preview_generation_dir(cache: &Path, generation: u64) -> PathBuf {
    cache.join(format!("build-{generation}"))
}

fn prune_preview_cache_generations(cache: &Path, current_generation: u64) -> miette::Result<()> {
    let Ok(entries) = fs::read_dir(cache) else {
        return Ok(());
    };
    let keep_previous = current_generation.saturating_sub(1);
    for entry in entries {
        let entry = entry.into_diagnostic()?;
        let path = entry.path();
        if !entry.file_type().into_diagnostic()?.is_dir() {
            continue;
        }
        let Some(generation) = parse_preview_generation_dir(&path) else {
            continue;
        };
        if generation != current_generation && generation != keep_previous {
            fs::remove_dir_all(path).into_diagnostic()?;
        }
    }
    Ok(())
}

fn parse_preview_generation_dir(path: &Path) -> Option<u64> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("build-"))
        .and_then(|generation| generation.parse().ok())
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
        let hash = short_sha256_hex(&bytes, 16);
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

pub(crate) fn short_sha256_hex(bytes: &[u8], hex_chars: usize) -> String {
    use std::fmt::Write as _;

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash: [u8; 32] = hasher.finalize().into();
    let byte_count = hex_chars.div_ceil(2).min(hash.len());
    let mut hex = String::with_capacity(byte_count * 2);
    for byte in &hash[..byte_count] {
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    hex.truncate(hex_chars);
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
    use assert_cmd::Command as AssertCommand;
    use std::cell::{Cell, RefCell};

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
                layouts: Provenance::Explicit(layouts.clone()),
                css: Provenance::Explicit(css.clone()),
                syntaxes: Provenance::Explicit(syntaxes.clone()),
                fonts: Provenance::Builtin,
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
    fn watch_covers_fonts_dir_contents_without_extension_filter() {
        let dir = tempfile::tempdir().unwrap();
        let fonts = dir.path().join("fonts");
        fs::create_dir_all(&fonts).unwrap();
        let targets = WatchTargets::new(
            dir.path().join("deck.md"),
            ResolvedAssets {
                layouts: Provenance::Builtin,
                css: Provenance::Builtin,
                syntaxes: Provenance::Builtin,
                fonts: Provenance::Explicit(fonts.clone()),
            },
        );

        assert!(targets.is_relevant_change(&fonts.join("deck-font.woff2")));
        assert!(targets.is_relevant_change(&fonts.join("font-face.css")));
        assert!(!targets.is_relevant_change(&dir.path().join("other.woff2")));

        let dirs = targets.watch_dirs();
        assert!(dirs.iter().any(|d| d == &fonts));
        assert!(dirs.iter().any(|d| d == dir.path()));
    }

    #[test]
    fn watch_ignores_dotfiles_in_fonts_dir() {
        let dir = tempfile::tempdir().unwrap();
        let fonts = dir.path().join("fonts");
        fs::create_dir_all(&fonts).unwrap();
        let targets = WatchTargets::new(
            dir.path().join("deck.md"),
            ResolvedAssets {
                layouts: Provenance::Builtin,
                css: Provenance::Builtin,
                syntaxes: Provenance::Builtin,
                fonts: Provenance::Explicit(fonts.clone()),
            },
        );

        assert!(!targets.is_relevant_change(&fonts.join(".DS_Store")));
        assert!(!targets.is_relevant_change(&fonts.join(".swp")));
        assert!(targets.is_relevant_change(&fonts.join("deck-font.woff2")));
    }

    #[test]
    fn watch_covers_nested_font_files() {
        let dir = tempfile::tempdir().unwrap();
        let fonts = dir.path().join("fonts");
        let nested = fonts.join("inter");
        fs::create_dir_all(&nested).unwrap();
        let targets = WatchTargets::new(
            dir.path().join("deck.md"),
            ResolvedAssets {
                layouts: Provenance::Builtin,
                css: Provenance::Builtin,
                syntaxes: Provenance::Builtin,
                fonts: Provenance::Explicit(fonts.clone()),
            },
        );

        assert!(targets.is_relevant_change(&nested.join("400.woff2")));
        assert!(!targets.is_relevant_change(&nested.join(".DS_Store")));
        assert!(!targets.is_relevant_change(&dir.path().join("other/400.woff2")));

        let dirs = targets.watch_dirs();
        assert!(dirs.iter().any(|dir| dir == &fonts));
        assert!(dirs.iter().any(|dir| dir == &nested));
    }

    #[test]
    fn watch_dirs_falls_back_to_nearest_existing_ancestor_for_missing_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let deck = root.join("deck.md");
        fs::write(&deck, "# Intro\n").unwrap();
        let missing_fonts = root.join("gone").join("fonts").join("noto");
        let targets = WatchTargets::new(
            deck,
            ResolvedAssets {
                layouts: Provenance::Builtin,
                css: Provenance::Builtin,
                syntaxes: Provenance::Builtin,
                fonts: Provenance::Explicit(missing_fonts.clone()),
            },
        );

        let dirs = targets.watch_dirs();

        assert!(
            dirs.iter().all(|path| path.is_dir()),
            "actual dirs: {dirs:?}"
        );
        assert!(
            dirs.iter().any(|path| path == &root),
            "actual dirs: {dirs:?}"
        );
        assert!(
            !dirs
                .iter()
                .any(|path| path == missing_fonts.parent().unwrap()),
            "actual dirs: {dirs:?}"
        );
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

        assert_eq!(
            assets.layouts,
            Provenance::DeckAdjacent(dir.path().join("layouts"))
        );
        assert_eq!(assets.css, Provenance::DeckAdjacent(dir.path().join("css")));
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

        assert_eq!(assets.layouts, Provenance::Explicit(explicit));
    }

    #[test]
    fn no_frontmatter_key_and_no_conventional_dir_resolves_to_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n").unwrap();
        let frontmatter = peitho_core::parse_frontmatter("# Intro\n").unwrap();
        let assets = resolve_assets(&deck, &frontmatter).unwrap();

        assert_eq!(assets.layouts, Provenance::Builtin);
        assert_eq!(assets.css, Provenance::Builtin);
    }

    #[test]
    fn write_shared_assets_copies_fonts_directory() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let fonts = dir.path().join("fonts");
        let out = dir.path().join("dist");

        fs::write(&deck, "# Intro\n").unwrap();
        fs::create_dir_all(&fonts).unwrap();
        fs::write(fonts.join("deck-font.woff2"), b"font bytes").unwrap();
        fs::write(
            fonts.join("font-face.css"),
            r#"@font-face { src: url("deck-font.woff2"); }"#,
        )
        .unwrap();

        let artifacts = build_artifacts(&deck).unwrap();
        write_shared_assets(&out, &artifacts).unwrap();

        assert_eq!(
            fs::read(out.join("fonts/deck-font.woff2")).unwrap(),
            b"font bytes"
        );
        assert!(fs::read_to_string(out.join("fonts/font-face.css"))
            .unwrap()
            .contains("@font-face"));
    }

    #[test]
    fn write_shared_assets_copies_single_font_file() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let font = dir.path().join("deck-font.woff2");
        let out = dir.path().join("dist");

        fs::write(&deck, "---\nfonts: ./deck-font.woff2\n---\n# Intro\n").unwrap();
        fs::write(&font, b"single font bytes").unwrap();

        let artifacts = build_artifacts(&deck).unwrap();
        write_shared_assets(&out, &artifacts).unwrap();

        assert_eq!(
            fs::read(out.join("fonts/deck-font.woff2")).unwrap(),
            b"single font bytes"
        );
    }

    #[test]
    fn write_fonts_assets_clears_stale_fonts_when_source_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("dist");
        let existing_fonts = out.join("fonts");
        fs::create_dir_all(&existing_fonts).unwrap();
        fs::write(existing_fonts.join("stale.woff2"), b"stale font").unwrap();

        write_fonts_assets(&out, None).unwrap();

        assert!(!existing_fonts.join("stale.woff2").exists());
        assert!(!existing_fonts.exists());
    }

    #[cfg(unix)]
    #[test]
    fn write_fonts_assets_rejects_symlink_entries() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("fonts");
        let target = dir.path().join("font.woff2");
        let out = dir.path().join("dist");
        fs::create_dir_all(&source).unwrap();
        fs::write(&target, b"font bytes").unwrap();
        symlink(&target, source.join("linked.woff2")).unwrap();

        let err = write_fonts_assets(&out, Some(&source)).unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("unsupported entry in fonts directory"),
            "actual error: {message}"
        );
        assert!(message.contains("linked.woff2"), "actual error: {message}");
        assert!(
            message.contains("only regular files and subdirectories"),
            "actual error: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_fonts_assets_rejects_symlink_source() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("font.woff2");
        let linked = dir.path().join("linked.woff2");
        let out = dir.path().join("dist");
        fs::write(&target, b"font bytes").unwrap();
        symlink(&target, &linked).unwrap();

        let err = write_fonts_assets(&out, Some(&linked)).unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("unsupported fonts: source"),
            "actual error: {message}"
        );
        assert!(message.contains("linked.woff2"), "actual error: {message}");
        assert!(message.contains("symlink"), "actual error: {message}");
        assert!(
            message.contains("point fonts: at a regular file or a directory"),
            "actual error: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_fonts_assets_rejects_symlink_to_directory_source() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let real_fonts = dir.path().join("real-fonts");
        let linked = dir.path().join("theme-fonts");
        let out = dir.path().join("dist");

        fs::write(&deck, "---\nfonts: ./theme-fonts\n---\n# Intro\n").unwrap();
        fs::create_dir_all(&real_fonts).unwrap();
        fs::write(real_fonts.join("deck-font.woff2"), b"font bytes").unwrap();
        symlink(&real_fonts, &linked).unwrap();

        let artifacts = build_artifacts(&deck).unwrap();
        let err = write_shared_assets(&out, &artifacts).unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("unsupported fonts: source"),
            "actual error: {message}"
        );
        assert!(message.contains("theme-fonts"), "actual error: {message}");
        assert!(message.contains("symlink"), "actual error: {message}");
        assert!(!out.join("fonts/deck-font.woff2").exists());
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

        let message = err.to_string();
        assert!(
            message.contains("no *.html files"),
            "actual error: {message}"
        );
    }

    #[test]
    fn build_options_deduplicates_watch_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let targets = WatchTargets::new(
            dir.path().join("deck.md"),
            ResolvedAssets {
                layouts: Provenance::Explicit(dir.path().join("title-body-code.html")),
                css: Provenance::Explicit(dir.path().join("base.css")),
                syntaxes: Provenance::Builtin,
                fonts: Provenance::Builtin,
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
        assert!(stderr.contains("build failed:"), "actual stderr: {stderr}");
        assert!(
            stderr.contains("slot 'code' got 2 item(s)"),
            "actual stderr: {stderr}"
        );
        assert!(
            stderr.contains("help: use a layout with more code capacity or remove one code block"),
            "actual stderr: {stderr}"
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
        assert!(stderr.contains("build failed:"), "actual stderr: {stderr}");
    }

    #[test]
    fn preview_watch_thread_result_reports_panics_as_errors() {
        let err = preview_watch_thread_result(|| -> miette::Result<()> {
            panic!("boom");
        })
        .unwrap_err();

        assert_eq!(err, "preview watch panicked: boom");
    }

    #[test]
    fn watch_path_handler_rebuilds_after_markdown_change() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
        fs::write(&fixture.options.input, "# Intro\n\n---\n# Details\n").unwrap();

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
            |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
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
        let mut state = watch_state_for_fixture(&fixture);
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let unrelated = fixture._dir.path().join("outside").join("ignored.txt");

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            &[unrelated],
            &mut stdout,
            &mut stderr,
            |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        )
        .unwrap();

        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        assert!(!fixture.options.out.join("manifest.json").exists());
    }

    #[test]
    fn watch_path_handler_ignores_output_directory_event_in_watched_parent() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
        stdout.clear();
        stderr.clear();

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&fixture.options.out),
            &mut stdout,
            &mut stderr,
            |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        )
        .unwrap();

        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    #[test]
    fn watch_path_handler_rebuilds_after_atomic_save_final_path() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let temp = fixture._dir.path().join("deck-new.md");

        fs::write(&temp, "# Atomic one\n\n---\n# Atomic two\n").unwrap();
        fs::rename(&temp, &fixture.options.input).unwrap();

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
            |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
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
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
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

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
            |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        )
        .unwrap();

        assert_eq!(
            state.targets.assets.layouts,
            Provenance::Explicit(alternate_layouts.clone())
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
        assert!(
            stderr.contains("note: watching new asset paths from frontmatter:"),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains("restart --watch"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn deck_refresh_updates_targets_without_reconciling_watched_dirs() {
        let fixture = WatchFixture::new("# Intro\n");
        let old_layouts = fixture._dir.path().join("layouts");
        let alternate_layouts = fixture._dir.path().join("other-layouts");
        fs::create_dir_all(&alternate_layouts).unwrap();
        fs::write(
            alternate_layouts.join("title-body-code.html"),
            TEST_LAYOUT_HTML,
        )
        .unwrap();
        let mut state = WatchState::new(
            fixture.options.input.clone(),
            fixture.targets.clone(),
            fixture.targets.watch_dirs(),
        );
        fs::remove_dir_all(&old_layouts).unwrap();
        fs::write(
            &state.input,
            "---\nlayouts: ./other-layouts\n---\n# Intro\n",
        )
        .unwrap();
        let mut stderr = Vec::new();

        refresh_watch_targets_after_deck_change(&mut state, &mut stderr).unwrap();

        assert!(state.watched_dirs.iter().any(|path| path == &old_layouts));
        assert_eq!(
            state.targets.assets.layouts,
            Provenance::Explicit(alternate_layouts)
        );
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("note: watching new asset paths from frontmatter:"));
    }

    #[test]
    fn refresh_watch_targets_does_not_note_when_only_provenance_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let deck = root.join("deck.md");
        let layouts = root.join("layouts");
        fs::create_dir_all(&layouts).unwrap();
        fs::write(layouts.join("title-body-code.html"), TEST_LAYOUT_HTML).unwrap();
        fs::write(&deck, "# Intro\n").unwrap();
        let targets = resolve_watch_targets(&deck).unwrap();
        assert_eq!(
            targets.assets.layouts,
            Provenance::DeckAdjacent(layouts.clone())
        );
        let watched_dirs = targets.watch_dirs();
        let mut state = WatchState::new(deck.clone(), targets, watched_dirs);
        fs::write(&deck, "---\nlayouts: ./layouts\n---\n# Intro\n").unwrap();
        let mut stderr = Vec::new();

        refresh_watch_targets_after_deck_change(&mut state, &mut stderr).unwrap();

        assert!(
            stderr.is_empty(),
            "actual stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert_eq!(state.targets.assets.layouts, Provenance::Explicit(layouts));
    }

    #[test]
    fn deck_refresh_updates_targets_when_frontmatter_asset_removed_and_no_deck_adjacent_dir_exists()
    {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let deck = root.join("deck.md");
        let explicit_layouts = root.join("x").join("layouts");
        fs::create_dir_all(&explicit_layouts).unwrap();
        fs::write(
            explicit_layouts.join("title-body-code.html"),
            TEST_LAYOUT_HTML,
        )
        .unwrap();
        fs::write(&deck, "---\nlayouts: ./x/layouts\n---\n# Intro\n").unwrap();
        let targets = resolve_watch_targets(&deck).unwrap();
        assert_eq!(
            targets.assets.layouts,
            Provenance::Explicit(explicit_layouts.clone())
        );
        let watched_dirs = targets.watch_dirs();
        assert!(watched_dirs.iter().any(|path| path == &explicit_layouts));
        let mut state = WatchState::new(deck.clone(), targets, watched_dirs);
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        fs::write(&deck, "# Intro\n").unwrap();

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&deck),
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| Ok(()),
        )
        .unwrap();

        assert_eq!(state.targets.assets.layouts, Provenance::Builtin);
        assert!(!state
            .watched_dirs
            .iter()
            .any(|path| path == &explicit_layouts));
    }

    #[test]
    fn watch_path_handler_reports_rebuild_error_when_asset_resolution_fails() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut watcher = RecordingWatchController::default();
        let original_assets = state.targets.assets.clone();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        fs::write(
            &fixture.options.input,
            "---\nlayouts: ./missing-layouts\n---\n# Intro\n",
        )
        .unwrap();

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
            |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        )
        .unwrap();

        assert_eq!(state.targets.assets, original_assets);
        assert!(watcher.watched.is_empty());
        assert!(watcher.unwatched.is_empty());
        assert!(stdout.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("build failed:"), "actual stderr: {stderr}");
        assert!(
            stderr.contains("layouts path does not exist"),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains("restart --watch"),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains("watching new asset paths"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn watch_path_handler_reconciles_after_new_fonts_subdir() {
        let (_dir, mut state, fonts) = watch_state_with_fonts();
        let nested = fonts.join("noto");
        fs::create_dir_all(&nested).unwrap();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&nested),
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 1);
        assert!(watcher.watched.iter().any(|path| path == &nested));
        assert_eq!(state.watched_dirs, state.targets.watch_dirs());
        assert!(stderr.is_empty());
    }

    #[test]
    fn watch_path_handler_reconciles_after_removed_fonts_subdir() {
        let (_dir, mut state, fonts) = watch_state_with_fonts();
        let nested = fonts.join("noto");
        fs::create_dir_all(&nested).unwrap();
        state.watched_dirs = state.targets.watch_dirs();
        fs::remove_dir_all(&nested).unwrap();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&nested),
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 1);
        assert!(watcher.unwatched.iter().any(|path| path == &nested));
        assert!(!state.watched_dirs.iter().any(|path| path == &nested));
    }

    #[test]
    fn watch_path_handler_rebuilds_after_irrelevant_ancestor_event_that_changes_watch_set() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let deck = root.join("deck.md");
        let sub = root.join("sub");
        let fonts = sub.join("fonts");
        fs::create_dir_all(&fonts).unwrap();
        fs::write(&deck, "---\nfonts: ./sub/fonts\n---\n# Intro\n").unwrap();
        let targets = resolve_watch_targets(&deck).unwrap();
        let mut state = WatchState::new(deck, targets, vec![root.clone()]);
        fs::remove_dir_all(&sub).unwrap();
        fs::create_dir_all(&fonts).unwrap();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&sub),
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 1);
        assert!(watcher.watched.iter().any(|path| path == &fonts));
        assert!(state.watched_dirs.iter().any(|path| path == &fonts));
    }

    #[test]
    fn watch_path_handler_ignores_hidden_font_directory_creation() {
        let (_dir, mut state, fonts) = watch_state_with_fonts();
        let hidden = fonts.join(".hidden");
        fs::create_dir_all(&hidden).unwrap();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&hidden),
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 0);
        assert!(!watcher.watched.iter().any(|path| path == &hidden));
        assert!(!state
            .targets
            .watch_dirs()
            .iter()
            .any(|path| path == &hidden));
        assert!(!state.watched_dirs.iter().any(|path| path == &hidden));
    }

    #[test]
    fn watch_path_handler_ignores_hidden_font_descendant_file_change() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let deck = root.join("deck.md");
        let fonts = root.join("fonts");
        let normal = fonts.join("normal");
        fs::create_dir_all(&normal).unwrap();
        fs::write(&deck, "---\nfonts: ./fonts\n---\n# Intro\n").unwrap();
        let targets = resolve_watch_targets(&deck).unwrap();
        let watched_dirs = targets.watch_dirs();
        let mut state = WatchState::new(deck, targets, watched_dirs);
        let hidden = fonts.join(".hidden");
        let hidden_file = hidden.join("a.woff2");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(&hidden_file, b"font").unwrap();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&hidden_file),
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 0);

        let normal_file = normal.join("a.woff2");
        fs::write(&normal_file, b"font").unwrap();
        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&normal_file),
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 1);
    }

    #[test]
    fn watch_path_handler_ignores_irrelevant_event_when_watch_set_is_unchanged() {
        let (_dir, mut state, fonts) = watch_state_with_fonts();
        let unrelated = fonts.parent().unwrap().join("ignored.txt");
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&unrelated),
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 0);
        assert!(watcher.watched.is_empty());
        assert!(watcher.unwatched.is_empty());
    }

    #[test]
    fn shared_watch_path_handler_invokes_injected_rebuild_action() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut calls = 0;

        handle_watch_paths_with_rebuild(
            &mut state,
            &mut watcher,
            std::slice::from_ref(&fixture.options.input),
            &mut stdout,
            &mut stderr,
            |stdout, _stderr| {
                calls += 1;
                writeln!(stdout, "custom rebuild").into_diagnostic()
            },
        )
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(String::from_utf8(stdout).unwrap(), "custom rebuild\n");
        assert!(stderr.is_empty());
    }

    #[test]
    fn watch_target_registration_runs_before_initial_rebuild() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        register_watch_target_dirs(&fixture.targets, &mut watcher).unwrap();
        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();

        assert!(watcher
            .watched
            .iter()
            .any(|path| same_watch_path(path, fixture._dir.path())));
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn watch_state_owns_registered_dirs_after_registration() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut watcher = RecordingWatchController::default();

        let watched_dirs = register_watch_target_dirs(&fixture.targets, &mut watcher).unwrap();
        let state = WatchState::new(
            fixture.options.input.clone(),
            fixture.targets.clone(),
            watched_dirs.clone(),
        );

        assert_eq!(state.input, fixture.options.input);
        assert_eq!(state.watched_dirs, watched_dirs);
        assert_eq!(watcher.watched, state.watched_dirs);
        assert!(state.emitted_watch_error_notes.is_empty());
    }

    #[test]
    fn notify_watch_controller_rejects_missing_dir_before_poll_watcher_can_skip_it() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let (tx, _rx) = mpsc::channel::<DebounceEventResult>();
        let notify_config = notify::Config::default().with_poll_interval(Duration::from_millis(50));
        let debounce_config = DebounceConfig::default()
            .with_timeout(Duration::from_millis(50))
            .with_notify_config(notify_config);
        let mut debouncer = new_debouncer_opt::<_, PollWatcher>(debounce_config, tx).unwrap();
        let mut controller = NotifyWatchController::new(debouncer.watcher());

        let err = controller.watch_dir(&missing).unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("does not exist"),
            "actual error: {message}"
        );
        assert!(
            !message.contains("failed to watch"),
            "actual error: {message}"
        );
        assert!(!message.contains("help:"), "actual error: {message}");
    }

    #[test]
    fn watch_all_dirs_wraps_startup_watch_failure_with_help() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let (tx, _rx) = mpsc::channel::<DebounceEventResult>();
        let notify_config = notify::Config::default().with_poll_interval(Duration::from_millis(50));
        let debounce_config = DebounceConfig::default()
            .with_timeout(Duration::from_millis(50))
            .with_notify_config(notify_config);
        let mut debouncer = new_debouncer_opt::<_, PollWatcher>(debounce_config, tx).unwrap();
        let mut controller = NotifyWatchController::new(debouncer.watcher());

        let err = watch_all_dirs(&mut controller, std::slice::from_ref(&missing)).unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains(&format!("failed to watch {}", missing.display())),
            "actual error: {message}"
        );
        assert!(
            message.contains(
                "verify the watched directories exist and are readable before starting --watch"
            ),
            "actual error: {message}"
        );
        assert!(message.contains("caused by:"), "actual error: {message}");
    }

    #[test]
    fn reconcile_watch_failure_note_uses_single_line_bare_controller_cause() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let (tx, _rx) = mpsc::channel::<DebounceEventResult>();
        let notify_config = notify::Config::default().with_poll_interval(Duration::from_millis(50));
        let debounce_config = DebounceConfig::default()
            .with_timeout(Duration::from_millis(50))
            .with_notify_config(notify_config);
        let mut debouncer = new_debouncer_opt::<_, PollWatcher>(debounce_config, tx).unwrap();
        let mut controller = NotifyWatchController::new(debouncer.watcher());
        let mut watched_dirs = Vec::new();
        let desired_dirs = vec![missing.clone()];
        let mut stderr = Vec::new();
        let mut emitted_notes = HashSet::new();

        let result = reconcile_watched_dirs(
            &mut controller,
            &mut watched_dirs,
            &desired_dirs,
            &mut stderr,
            &mut emitted_notes,
        )
        .unwrap();

        assert!(!result.changed);
        assert!(result.had_failures);
        let stderr = String::from_utf8(stderr).unwrap();
        let lines = stderr.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "actual stderr: {stderr}");
        assert!(
            lines[0].starts_with(&format!("note: failed to watch {}: ", missing.display())),
            "actual stderr: {stderr}"
        );
        assert!(!stderr.contains("help:"), "actual stderr: {stderr}");
        assert!(
            !stderr.contains("restart --watch"),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains(&format!(
                "note: failed to watch {}: failed to watch {}",
                missing.display(),
                missing.display()
            )),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn reconcile_watched_dirs_applies_diff_and_updates_owned_set() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let old = root.join("old");
        let keep = root.join("keep");
        let new = root.join("new");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&keep).unwrap();
        fs::create_dir_all(&new).unwrap();
        let mut watched_dirs = vec![old.clone(), keep.clone()];
        let desired_dirs = vec![keep.clone(), new.clone()];
        let mut watcher = RecordingWatchController::default();
        let mut stderr = Vec::new();
        let mut emitted_notes = HashSet::new();

        let result = reconcile_watched_dirs(
            &mut watcher,
            &mut watched_dirs,
            &desired_dirs,
            &mut stderr,
            &mut emitted_notes,
        )
        .unwrap();

        assert!(result.changed);
        assert!(!result.had_failures);
        assert_eq!(watcher.unwatched, vec![old]);
        assert_eq!(watcher.watched, vec![new]);
        assert_eq!(watched_dirs, desired_dirs);
        assert!(stderr.is_empty());
    }

    #[test]
    fn reconcile_watched_dirs_excludes_failed_watch_and_retries_later() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let desired = root.join("desired");
        fs::create_dir_all(&desired).unwrap();
        let mut watched_dirs = Vec::new();
        let desired_dirs = vec![desired.clone()];
        let mut watcher = RecordingWatchController {
            fail_watch: vec![desired.clone()],
            ..RecordingWatchController::default()
        };
        let mut stderr = Vec::new();
        let mut emitted_notes = HashSet::new();

        let result = reconcile_watched_dirs(
            &mut watcher,
            &mut watched_dirs,
            &desired_dirs,
            &mut stderr,
            &mut emitted_notes,
        )
        .unwrap();

        assert!(!result.changed);
        assert!(result.had_failures);
        assert_eq!(watcher.watched, vec![desired.clone()]);
        assert!(watched_dirs.is_empty());

        watcher.fail_watch.clear();
        let result = reconcile_watched_dirs(
            &mut watcher,
            &mut watched_dirs,
            &desired_dirs,
            &mut stderr,
            &mut emitted_notes,
        )
        .unwrap();

        assert!(result.changed);
        assert!(!result.had_failures);
        assert_eq!(watcher.watched, vec![desired.clone(), desired.clone()]);
        assert_eq!(watched_dirs, desired_dirs);
    }

    #[test]
    fn reconcile_watched_dirs_notes_failures_and_converges() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let stale = root.join("stale");
        let desired = root.join("desired");
        let mut watched_dirs = vec![stale.clone()];
        let desired_dirs = vec![desired.clone()];
        let mut watcher = RecordingWatchController {
            fail_unwatch: vec![stale.clone()],
            fail_watch: vec![desired.clone()],
            ..RecordingWatchController::default()
        };
        let mut stderr = Vec::new();
        let mut emitted_notes = HashSet::new();

        let result = reconcile_watched_dirs(
            &mut watcher,
            &mut watched_dirs,
            &desired_dirs,
            &mut stderr,
            &mut emitted_notes,
        )
        .unwrap();

        assert!(result.changed);
        assert!(result.had_failures);
        assert_eq!(watcher.unwatched, vec![stale]);
        assert_eq!(watcher.watched, vec![desired]);
        assert!(watched_dirs.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(
            stderr.contains("note: failed to stop watching"),
            "actual stderr: {stderr}"
        );
        assert!(
            stderr.contains("note: failed to watch"),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains("restart --watch"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn watch_startup_runs_registration_before_initial_action() {
        let events = RefCell::new(Vec::new());
        let input = PathBuf::from("deck.md");

        let (watch, value) = run_after_watch_registration(
            &input,
            |path| {
                events
                    .borrow_mut()
                    .push(format!("watch:{}", path.display()));
                Ok("watch-runtime")
            },
            || {
                events.borrow_mut().push("initial-build".to_owned());
                Ok("initial-root")
            },
        )
        .unwrap();

        assert_eq!(watch, "watch-runtime");
        assert_eq!(value, "initial-root");
        assert_eq!(
            &*events.borrow(),
            &vec!["watch:deck.md".to_owned(), "initial-build".to_owned()]
        );
    }

    #[test]
    fn watch_target_resolution_falls_back_to_deck_parent_when_initial_assets_are_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "---\nlayouts: ./missing-layouts\n---\n# Intro\n").unwrap();

        let targets = resolve_watch_targets_or_deck_only(&deck);

        assert_eq!(targets.roots.len(), 1);
        assert_eq!(targets.roots[0].path, deck);
        assert_eq!(targets.watch_dirs(), vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn watch_event_handler_notes_error_reconciles_and_continues() {
        let (_dir, mut state, fonts) = watch_state_with_fonts();
        fs::remove_dir_all(&fonts).unwrap();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;
        let result: DebounceEventResult =
            Err(notify::Error::path_not_found().add_path(fonts.clone()));

        handle_watch_event_result(
            result,
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 1);
        assert!(watcher.unwatched.iter().any(|path| path == &fonts));
        assert!(!state.watched_dirs.iter().any(|path| path == &fonts));
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(
            stderr.contains("note: watch error:"),
            "actual stderr: {stderr}"
        );
        assert!(
            stderr.contains("missing watch targets are dropped and re-watched automatically"),
            "actual stderr: {stderr}"
        );
        assert!(
            stderr.contains(&fonts.display().to_string()),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains("stopped watching"),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains("removed missing paths"),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains("restart the command"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn watch_event_handler_suppresses_same_error_after_reconcile_changes_state() {
        let (_dir, mut state, fonts) = watch_state_with_fonts();
        fs::remove_dir_all(&fonts).unwrap();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        for _ in 0..2 {
            handle_watch_event_result(
                Err(notify::Error::path_not_found().add_path(fonts.clone())),
                &mut state,
                &mut watcher,
                &mut stdout,
                &mut stderr,
                |_stdout, _stderr| {
                    rebuilds += 1;
                    Ok(())
                },
            )
            .unwrap();
        }

        assert_eq!(rebuilds, 1);
        let stderr = String::from_utf8(stderr).unwrap();
        assert_eq!(stderr.matches("note: watch error:").count(), 1);
        assert_eq!(
            stderr
                .matches("missing watch targets are dropped and re-watched automatically")
                .count(),
            1
        );
        assert!(
            !stderr.contains("stopped watching"),
            "actual stderr: {stderr}"
        );
        assert_eq!(
            stderr
                .matches("if this error persists, check file watcher permissions")
                .count(),
            1
        );
    }

    #[test]
    fn watch_event_handler_returns_ok_when_watcher_reports_error() {
        let (_dir, mut state, _fonts) = watch_state_with_fonts();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        handle_watch_event_result(
            Err(notify::Error::generic("backend stopped")),
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| Ok(()),
        )
        .unwrap();

        assert!(stdout.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("note: watch error: backend stopped"));
    }

    #[test]
    fn watch_event_handler_does_not_rebuild_when_error_does_not_change_watch_set() {
        let (_dir, mut state, _fonts) = watch_state_with_fonts();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;
        let result: DebounceEventResult =
            Err(notify::Error::generic("permission denied while scanning")
                .add_path(state.input.clone()));

        handle_watch_event_result(
            result,
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 0);
        assert_eq!(state.watched_dirs, state.targets.watch_dirs());
        assert!(watcher.watched.is_empty());
        assert!(watcher.unwatched.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(
            stderr.contains("missing watch targets are dropped and re-watched automatically"),
            "actual stderr: {stderr}"
        );
        assert!(
            stderr.contains("if this error persists, check file watcher permissions"),
            "actual stderr: {stderr}"
        );
        assert!(
            !stderr.contains("removed missing paths"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn watch_event_handler_rebuilds_once_when_error_changes_watch_set_even_if_path_irrelevant() {
        let (_dir, mut state, fonts) = watch_state_with_fonts();
        let stale = fonts.parent().unwrap().join("stale-watch-root");
        state.watched_dirs.push(stale.clone());
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;
        let result: DebounceEventResult =
            Err(notify::Error::path_not_found().add_path(stale.clone()));

        handle_watch_event_result(
            result,
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 1);
        assert!(watcher.unwatched.iter().any(|path| path == &stale));
        assert!(!state.watched_dirs.iter().any(|path| path == &stale));
    }

    #[test]
    fn watch_event_handler_suppresses_reconcile_failure_until_clean_ok_batch() {
        let (_dir, mut state, fonts) = watch_state_with_fonts();
        state.watched_dirs.retain(|path| path != &fonts);
        let mut watcher = RecordingWatchController {
            fail_watch: vec![fonts.clone()],
            ..RecordingWatchController::default()
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let event_path = state
            .input
            .parent()
            .unwrap()
            .join("dist")
            .join("index.html");

        for _ in 0..2 {
            handle_watch_event_result(
                Ok(vec![notify_debouncer_mini::DebouncedEvent::new(
                    event_path.clone(),
                    notify_debouncer_mini::DebouncedEventKind::Any,
                )]),
                &mut state,
                &mut watcher,
                &mut stdout,
                &mut stderr,
                |_stdout, _stderr| Ok(()),
            )
            .unwrap();
        }

        let stderr_after_repeated_failure = String::from_utf8(stderr.clone()).unwrap();
        assert_eq!(
            stderr_after_repeated_failure
                .matches("note: failed to watch")
                .count(),
            1
        );

        watcher.fail_watch.clear();
        handle_watch_event_result(
            Ok(vec![notify_debouncer_mini::DebouncedEvent::new(
                event_path.clone(),
                notify_debouncer_mini::DebouncedEventKind::Any,
            )]),
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| Ok(()),
        )
        .unwrap();

        state.watched_dirs.retain(|path| path != &fonts);
        watcher.fail_watch.push(fonts.clone());
        handle_watch_event_result(
            Ok(vec![notify_debouncer_mini::DebouncedEvent::new(
                event_path,
                notify_debouncer_mini::DebouncedEventKind::Any,
            )]),
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| Ok(()),
        )
        .unwrap();

        let stderr = String::from_utf8(stderr).unwrap();
        assert_eq!(stderr.matches("note: failed to watch").count(), 2);
    }

    #[test]
    fn watch_event_handler_suppresses_alternating_duplicate_error_notes() {
        let (_dir, mut state, _fonts) = watch_state_with_fonts();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        for message in [
            "backend noisy a",
            "backend noisy b",
            "backend noisy a",
            "backend noisy b",
        ] {
            handle_watch_event_result(
                Err(notify::Error::generic(message)),
                &mut state,
                &mut watcher,
                &mut stdout,
                &mut stderr,
                |_stdout, _stderr| Ok(()),
            )
            .unwrap();
        }

        let stderr = String::from_utf8(stderr).unwrap();
        assert_eq!(
            stderr.matches("note: watch error: backend noisy a").count(),
            1
        );
        assert_eq!(
            stderr.matches("note: watch error: backend noisy b").count(),
            1
        );
    }

    #[test]
    fn watch_event_handler_suppresses_consecutive_duplicate_error_notes() {
        let (_dir, mut state, _fonts) = watch_state_with_fonts();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        for _ in 0..2 {
            handle_watch_event_result(
                Err(notify::Error::generic("backend noisy")),
                &mut state,
                &mut watcher,
                &mut stdout,
                &mut stderr,
                |_stdout, _stderr| Ok(()),
            )
            .unwrap();
        }

        handle_watch_event_result(
            Ok(vec![notify_debouncer_mini::DebouncedEvent::new(
                state.input.clone(),
                notify_debouncer_mini::DebouncedEventKind::Any,
            )]),
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| Ok(()),
        )
        .unwrap();

        handle_watch_event_result(
            Err(notify::Error::generic("backend noisy")),
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| Ok(()),
        )
        .unwrap();

        let stderr = String::from_utf8(stderr).unwrap();
        assert_eq!(
            stderr.matches("note: watch error: backend noisy").count(),
            2
        );
    }

    #[test]
    fn watch_event_handler_keeps_stderr_write_failure_fatal() {
        let (_dir, mut state, _fonts) = watch_state_with_fonts();
        let mut watcher = RecordingWatchController::default();
        let mut stdout = Vec::new();
        let mut stderr = FailingWriter;

        let err = handle_watch_event_result(
            Err(notify::Error::generic("backend stopped")),
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| Ok(()),
        )
        .unwrap_err();

        assert!(err.to_string().contains("closed"), "actual error: {err}");
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
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected present command");
            }
        }
    }

    #[test]
    fn present_command_accepts_host_flag() {
        let cli = Cli::parse_from(["peitho", "present", "deck.md", "--host", "100.64.0.5"]);

        match cli.command {
            Command::Present { input, host, .. } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert_eq!(host, Some("100.64.0.5".parse().unwrap()));
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected present command");
            }
        }
    }

    #[test]
    fn present_command_rejects_invalid_host_ip() {
        let err = Cli::try_parse_from(["peitho", "present", "deck.md", "--host", "not-an-ip"])
            .unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn present_options_reject_host_with_no_serve() {
        let options = PresentOptions {
            input: PathBuf::from("deck.md"),
            shell: None,
            port: 0,
            no_open: true,
            no_serve: true,
            no_presenter: false,
            presenter_windowed: false,
            host: Some("100.64.0.5".parse().unwrap()),
        };

        let err = validate_present_options(&options).unwrap_err();

        assert!(err
            .to_string()
            .contains("--host requires the present server"));
        assert!(err.to_string().contains("--no-serve"));
    }

    #[test]
    fn present_options_reject_loopback_host() {
        let options = PresentOptions {
            input: PathBuf::from("deck.md"),
            shell: None,
            port: 0,
            no_open: true,
            no_serve: false,
            no_presenter: false,
            presenter_windowed: false,
            host: Some("127.0.0.1".parse().unwrap()),
        };

        let err = validate_present_options(&options).unwrap_err();

        assert!(err.to_string().contains("--host must be non-loopback"));
        assert!(err.to_string().contains("use the default loopback server"));
    }

    #[test]
    fn remote_control_lines_format_specific_host() {
        let target = remote_control_target_for_host("100.64.0.5".parse().unwrap(), 3000);
        let (lines, qr_url) = remote_control_output(&target);

        assert_eq!(lines, vec!["remote control: http://100.64.0.5:3000/remote"]);
        assert_eq!(
            qr_url.unwrap().as_str(),
            remote_url_from_control_line(&lines[0])
        );
    }

    #[test]
    fn remote_control_lines_bracket_specific_ipv6_host() {
        let target = remote_control_target_for_host("2001:db8::5".parse().unwrap(), 3000);
        let (lines, qr_url) = remote_control_output(&target);

        assert_eq!(
            lines,
            vec!["remote control: http://[2001:db8::5]:3000/remote"]
        );
        assert_eq!(qr_url.unwrap().as_str(), "http://[2001:db8::5]:3000/remote");
        assert_eq!(
            qr_url.unwrap().as_str(),
            remote_url_from_control_line(&lines[0])
        );
    }

    #[test]
    fn remote_control_lines_format_unspecified_host_candidates_with_labels() {
        let candidates = peitho::remote_url::remote_url_candidates(
            &[
                "192.168.1.20".parse().unwrap(),
                "100.100.10.5".parse().unwrap(),
                "10.0.0.15".parse().unwrap(),
            ],
            Some("10.0.0.15".parse().unwrap()),
            3000,
            None,
        );

        let target = RemoteControlTarget::Candidates(candidates);
        let (lines, qr_url) = remote_control_output(&target);

        assert_eq!(
            lines,
            vec![
                "remote control: http://10.0.0.15:3000/remote",
                "remote control (Tailscale): http://100.100.10.5:3000/remote",
                "remote control: http://192.168.1.20:3000/remote"
            ]
        );
        assert_eq!(
            qr_url.unwrap().as_str(),
            remote_url_from_control_line(&lines[0])
        );
    }

    #[test]
    fn remote_control_lines_show_when_unspecified_host_has_no_candidates() {
        let target = RemoteControlTarget::Candidates(Vec::new());
        let (lines, qr_url) = remote_control_output(&target);

        assert_eq!(
            lines,
            vec!["remote control: no non-loopback network addresses found"]
        );
        assert!(qr_url.is_none());
    }

    fn remote_url_from_control_line(line: &str) -> &str {
        let start = line
            .find("http://")
            .unwrap_or_else(|| panic!("remote control line did not contain a URL: {line}"));
        &line[start..]
    }

    #[test]
    fn lint_command_defaults_input_to_deck_md() {
        let cli = Cli::parse_from(["peitho", "lint"]);

        match cli.command {
            Command::Lint { input } => {
                assert_eq!(input, PathBuf::from("deck.md"));
            }
            Command::Build { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected lint command");
            }
        }
    }

    #[test]
    fn preview_command_defaults_input_and_accepts_port_and_no_open() {
        let cli = Cli::parse_from(["peitho", "preview", "--port", "4321", "--no-open"]);

        match cli.command {
            Command::Preview {
                input,
                port,
                no_open,
            } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert_eq!(port, 4321);
                assert!(no_open);
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected preview command");
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
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Completions { .. } => {
                panic!("expected export pdf command");
            }
        }
    }

    #[test]
    fn completions_command_accepts_shell_argument() {
        let cli = Cli::parse_from(["peitho", "completions", "bash"]);

        match cli.command {
            Command::Completions { shell } => {
                assert_eq!(shell, Shell::Bash);
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Export { .. } => {
                panic!("expected completions command");
            }
        }
    }

    #[test]
    fn layouts_command_defaults_input_and_accepts_explain_and_json() {
        let cli = Cli::parse_from(["peitho", "layouts", "--explain", "intro", "--json"]);

        match cli.command {
            Command::Layouts {
                input,
                explain,
                json,
            } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert_eq!(explain, Some("intro".to_owned()));
                assert!(json);
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected layouts command");
            }
        }
    }

    #[test]
    fn chrome_print_args_include_virtual_time_budget_and_url_last() {
        let profile = Path::new("/tmp/peitho-profile");
        let out = Path::new("/tmp/out.pdf");
        let url = "file:///tmp/pdf.html";

        let args = chrome_print_args(profile, out, url);

        let expected = vec![
            OsString::from("--headless=new"),
            OsString::from("--disable-gpu"),
            OsString::from("--no-sandbox"),
            OsString::from("--no-pdf-header-footer"),
            OsString::from("--virtual-time-budget=10000"),
            OsString::from(format!("--user-data-dir={}", profile.display())),
            OsString::from(format!("--print-to-pdf={}", out.display())),
            OsString::from(url),
        ];
        assert_eq!(args, expected);
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
    fn kept_workspace_error_mentions_path_and_preserves_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        fs::write(workspace.join("pdf.html"), "<html></html>").unwrap();

        let err = keep_workspace_for_error(tmp, miette::miette!("export failed"));
        let message = err.to_string();

        assert!(message.contains("export failed"), "actual error: {message}");
        assert!(
            message.contains("workspace kept at"),
            "actual error: {message}"
        );
        assert!(
            message.contains(&workspace.display().to_string()),
            "actual error: {message}"
        );
        assert!(workspace.join("pdf.html").is_file());
        fs::remove_dir_all(workspace).unwrap();
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
        assert!(
            message.contains("Chrome not found"),
            "actual error: {message}"
        );
        assert!(
            message.contains("PEITHO_CHROME_PATH=<absolute-path>"),
            "actual error: {message}"
        );
    }

    #[test]
    fn chrome_process_wait_error_does_not_use_install_hint() {
        let err = chrome_process_error(
            Path::new("/tmp/chrome"),
            ProcessRunError::Wait(std::io::Error::other("wait failed")),
            &ChromeCompletion::PdfWritten {
                output_path: PathBuf::from("out.pdf"),
            },
        );
        let message = err.to_string();

        assert!(
            message.contains("failed to wait on Chrome: wait failed"),
            "actual error: {message}"
        );
        assert!(
            message.contains("help: retry export"),
            "actual error: {message}"
        );
        assert!(
            !message.contains("install Google Chrome"),
            "actual error: {message}"
        );
    }

    #[test]
    fn chrome_process_wait_error_uses_lint_retry_for_lint_completion() {
        let err = chrome_process_error(
            Path::new("/tmp/chrome"),
            ProcessRunError::Wait(std::io::Error::other("wait failed")),
            &ChromeCompletion::LintResultLogged,
        );
        let message = err.to_string();

        assert!(
            message.contains("failed to wait on Chrome: wait failed"),
            "actual error: {message}"
        );
        assert!(
            message.contains("help: retry lint"),
            "actual error: {message}"
        );
        assert!(
            message.contains("lint workspace"),
            "actual error: {message}"
        );
        assert!(message.contains("lint.html"), "actual error: {message}");
        assert!(
            !message.contains("chrome-stderr.log"),
            "actual error: {message}"
        );
        assert!(!message.contains("retry export"), "actual error: {message}");
    }

    #[test]
    fn chrome_process_kill_error_does_not_use_install_hint() {
        let err = chrome_process_error(
            Path::new("/tmp/chrome"),
            ProcessRunError::Kill(std::io::Error::other("kill failed")),
            &ChromeCompletion::PdfWritten {
                output_path: PathBuf::from("out.pdf"),
            },
        );
        let message = err.to_string();

        assert!(
            message.contains("failed to terminate Chrome: kill failed"),
            "actual error: {message}"
        );
        assert!(
            message.contains("help: report the underlying io error"),
            "actual error: {message}"
        );
        assert!(
            !message.contains("install Google Chrome"),
            "actual error: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_runner_success_writes_stdin_and_captures_stdout() {
        let args = [
            OsString::from("-c"),
            OsString::from("read value; printf 'seen:%s' \"$value\""),
        ];
        let outcome = run_child_with_timeout(
            Path::new("/bin/sh"),
            &args,
            Some(b"input\n"),
            Duration::from_secs(2),
            |_, _| false,
        )
        .unwrap();

        match outcome {
            ProcessOutcome::Exited {
                status,
                stdout,
                stderr,
            } => {
                assert!(status.success());
                assert_eq!(stdout, b"seen:input");
                assert!(stderr.is_empty());
            }
            other => panic!("expected exited process, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn process_runner_nonzero_exit_captures_stderr() {
        let args = [
            OsString::from("-c"),
            OsString::from("printf 'boom\\n' >&2; exit 1"),
        ];
        let outcome = run_child_with_timeout(
            Path::new("/bin/sh"),
            &args,
            None,
            Duration::from_secs(2),
            |_, _| false,
        )
        .unwrap();

        match outcome {
            ProcessOutcome::Exited {
                status,
                stdout,
                stderr,
            } => {
                assert!(!status.success());
                assert!(stdout.is_empty());
                assert_eq!(stderr, b"boom\n");
            }
            other => panic!("expected exited process, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn process_runner_timeout_kills_child() {
        let args = [OsString::from("-c"), OsString::from("sleep 5")];
        let started = std::time::Instant::now();
        let outcome = run_child_with_timeout(
            Path::new("/bin/sh"),
            &args,
            None,
            Duration::from_millis(100),
            |_, _| false,
        )
        .unwrap();

        assert!(started.elapsed() < Duration::from_secs(2));
        match outcome {
            ProcessOutcome::TimedOut { stderr } => assert!(stderr.is_empty()),
            other => panic!("expected timed-out process, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn process_runner_passes_stdin_to_child() {
        let args = [OsString::from("-c"), OsString::from("cat")];
        let outcome = run_child_with_timeout(
            Path::new("/bin/sh"),
            &args,
            Some(b"echoed input"),
            Duration::from_secs(2),
            |_, _| false,
        )
        .unwrap();

        match outcome {
            ProcessOutcome::Exited { status, stdout, .. } => {
                assert!(status.success());
                assert_eq!(stdout, b"echoed input");
            }
            other => panic!("expected exited process, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn code_image_runner_resolves_relative_paths_from_deck_parent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("puppeteer-config.json"), "{}").unwrap();
        let command = peitho_core::domain::CodeImageCommand {
            argv: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "test -f ./puppeteer-config.json && printf '<svg viewBox=\"0 0 10 10\"></svg>'"
                    .to_owned(),
            ],
        };
        let runner = CliSvgRunner::for_deck(&dir.path().join("deck.md"));

        let stdout = peitho_core::code_images::SvgRunner::run(&runner, &command, "").unwrap();

        assert_eq!(stdout, br#"<svg viewBox="0 0 10 10"></svg>"#);
    }

    #[cfg(unix)]
    #[test]
    fn one_shot_chrome_runner_returns_after_pdf_completion_signal_and_kills_child() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        let out = dir.path().join("out.pdf");
        write_script(
            &fake_chrome,
            r#"#!/bin/sh
out="$1"
printf '%s' '%PDF-test' > "$out"
printf '9 bytes written to file %s\n' "$out" >&2
exec sleep 30
"#,
        );

        let started = std::time::Instant::now();
        let output = run_one_shot_chrome(
            Path::new("/bin/sh"),
            &[
                fake_chrome.clone().into_os_string(),
                out.clone().into_os_string(),
            ],
            ChromeCompletion::PdfWritten {
                output_path: out.clone(),
            },
            CHROME_ONE_SHOT_TIMEOUT,
        )
        .unwrap();

        // Well below the child's `sleep 30`: proves the completion signal
        // triggered the early return, with headroom for loaded CI runners.
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(out.is_file());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("bytes written to file"),
            "actual stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    #[test]
    fn one_shot_chrome_runner_accepts_successful_pdf_exit_without_stderr_signal() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        let out = dir.path().join("out.pdf");
        write_script(
            &fake_chrome,
            r#"#!/bin/sh
out="$1"
printf '%s' '%PDF-test' > "$out"
"#,
        );

        let output = run_one_shot_chrome(
            Path::new("/bin/sh"),
            &[
                fake_chrome.clone().into_os_string(),
                out.clone().into_os_string(),
            ],
            ChromeCompletion::PdfWritten {
                output_path: out.clone(),
            },
            CHROME_ONE_SHOT_TIMEOUT,
        )
        .unwrap();

        assert!(out.is_file());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn pdf_completion_scan_detects_needles_across_buffer_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.pdf");
        fs::write(&out, "%PDF-test").unwrap();
        let mut state = ChromeCompletionState::default();
        let mut stderr = b"9 bytes written to fi".to_vec();

        assert!(!ChromeCompletion::PdfWritten {
            output_path: out.clone()
        }
        .is_ready(&[], &stderr, &mut state));

        stderr.extend_from_slice(b"le /tmp/out.pdf\n");

        assert!(ChromeCompletion::PdfWritten { output_path: out }.is_ready(
            &[],
            &stderr,
            &mut state
        ));
    }

    #[test]
    fn lint_completion_scans_stderr_for_done_sentinel_only() {
        let mut state = ChromeCompletionState::default();
        let stdout = b"PEITHO_LINT_DONE".to_vec();
        let mut stderr = b"[1:2:INFO:CONSOLE(59)] \"PEITHO_LINT_CHUNK 1/1 abc".to_vec();

        assert!(!ChromeCompletion::LintResultLogged.is_ready(&stdout, &stderr, &mut state));

        stderr.extend_from_slice(
            b"\n[1:2:INFO:CONSOLE(60)] \"PEITHO_LINT_DONE\", source: file:///tmp/lint.html (60)",
        );

        assert!(ChromeCompletion::LintResultLogged.is_ready(&stdout, &stderr, &mut state));
        assert!(ChromeCompletion::LintResultLogged
            .is_ready_after_successful_exit(&stdout, &stderr, &mut state));
    }

    #[cfg(unix)]
    #[test]
    fn one_shot_chrome_runner_rejects_successful_lint_exit_without_done_signal() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        write_script(
            &fake_chrome,
            r#"#!/bin/sh
printf 'JavaScript exploded before lint payload\n' >&2
"#,
        );

        let err = run_one_shot_chrome(
            Path::new("/bin/sh"),
            &[fake_chrome.clone().into_os_string()],
            ChromeCompletion::LintResultLogged,
            CHROME_ONE_SHOT_TIMEOUT,
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("completed before one-shot output was ready"),
            "actual error: {message}"
        );
        assert!(
            message.contains("lint measurement payload"),
            "actual error: {message}"
        );
        assert!(
            message.contains("JavaScript exploded"),
            "actual error: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn one_shot_chrome_runner_times_out_and_reaps_child_without_completion() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        write_script(
            &fake_chrome,
            r#"#!/bin/sh
exec sleep 30
"#,
        );

        let started = std::time::Instant::now();
        let out = dir.path().join("out.pdf");
        let err = run_one_shot_chrome(
            Path::new("/bin/sh"),
            &[fake_chrome.clone().into_os_string()],
            ChromeCompletion::PdfWritten { output_path: out },
            Duration::from_millis(100),
        )
        .unwrap_err();

        // Well below the child's `sleep 30`: proves the deadline killed the
        // child instead of waiting it out, with headroom for loaded CI runners.
        assert!(started.elapsed() < Duration::from_secs(10));
        let message = err.to_string();
        assert!(message.contains("timed out"), "actual error: {message}");
    }

    #[cfg(unix)]
    #[test]
    fn one_shot_chrome_runner_lint_timeout_uses_lint_help() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        write_script(
            &fake_chrome,
            r#"#!/bin/sh
exec sleep 30
"#,
        );

        let err = run_one_shot_chrome(
            Path::new("/bin/sh"),
            &[fake_chrome.clone().into_os_string()],
            ChromeCompletion::LintResultLogged,
            Duration::from_millis(100),
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("timed out"), "actual error: {message}");
        assert!(message.contains("retry lint"), "actual error: {message}");
        assert!(
            message.contains("lint workspace"),
            "actual error: {message}"
        );
        assert!(message.contains("lint.html"), "actual error: {message}");
        assert!(
            !message.contains("chrome-stderr.log"),
            "actual error: {message}"
        );
        assert!(!message.contains("retry export"), "actual error: {message}");
        assert!(
            !message.contains("export command"),
            "actual error: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pdf_completion_requires_nonempty_output_file() {
        let dir = tempfile::tempdir().unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        let out = dir.path().join("out.pdf");
        write_script(
            &fake_chrome,
            r#"#!/bin/sh
out="$1"
: > "$out"
printf '0 bytes written to file %s\n' "$out" >&2
"#,
        );

        let err = run_one_shot_chrome(
            Path::new("/bin/sh"),
            &[
                fake_chrome.clone().into_os_string(),
                out.clone().into_os_string(),
            ],
            ChromeCompletion::PdfWritten { output_path: out },
            CHROME_ONE_SHOT_TIMEOUT,
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("completed before one-shot output was ready"),
            "actual error: {message}"
        );
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
    fn present_cache_copies_fonts() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let fonts = dir.path().join("fonts");
        let cache = dir.path().join("present-cache");

        fs::write(&deck, "# Intro\n").unwrap();
        fs::create_dir_all(&fonts).unwrap();
        fs::write(fonts.join("deck-font.woff2"), b"font bytes").unwrap();

        let artifacts = build_artifacts(&deck).unwrap();
        fs::create_dir_all(&cache).unwrap();
        emit_present_cache(&cache, &artifacts, None, false).unwrap();

        assert_eq!(
            fs::read(cache.join("fonts/deck-font.woff2")).unwrap(),
            b"font bytes"
        );
    }

    #[test]
    fn emit_preview_cache_writes_preview_only_files_in_generation_dir() {
        let fixture = WatchFixture::new("# Intro\n\n<!-- speaker note -->\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let cache = fixture._dir.path().join(".peitho/preview-cache");

        let generation_dir = emit_preview_cache_generation(&cache, 0, &artifacts).unwrap();

        assert_eq!(generation_dir, cache.join("build-0"));
        assert!(generation_dir.join("index.html").is_file());
        assert!(generation_dir.join("preview.js").is_file());
        assert!(generation_dir.join("peitho.css").is_file());
        assert!(generation_dir.join("manifest.json").is_file());
        assert!(generation_dir.join("slides/000-intro.html").is_file());
        assert!(!generation_dir.join("notes.json").exists());
        assert!(!generation_dir.join("present.html").exists());
        assert!(!generation_dir.join("presenter.html").exists());
        assert!(!generation_dir.join("present.json").exists());
        assert!(!generation_dir.join("shell.js").exists());
        assert!(!generation_dir.join("remote.html").exists());
        assert!(!generation_dir.join("remote.js").exists());

        let index = fs::read_to_string(generation_dir.join("index.html")).unwrap();
        assert!(index.contains("./preview.js"));
        assert!(index.contains("mountPreviewShell"));
        assert!(index.contains("installPreviewKeyboard"));
        assert!(index.contains("installPreviewReload"));
    }

    #[test]
    fn preview_cache_prune_keeps_current_and_previous_generation() {
        let fixture = WatchFixture::new("# Intro\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let cache = fixture._dir.path().join(".peitho/preview-cache");

        emit_preview_cache_generation(&cache, 0, &artifacts).unwrap();
        emit_preview_cache_generation(&cache, 1, &artifacts).unwrap();
        emit_preview_cache_generation(&cache, 2, &artifacts).unwrap();
        prune_preview_cache_generations(&cache, 2).unwrap();

        assert!(!cache.join("build-0").exists());
        assert!(cache.join("build-1").is_dir());
        assert!(cache.join("build-2").is_dir());
    }

    #[test]
    fn preview_watch_rebuild_success_swaps_broadcasts_and_prunes() {
        let fixture = WatchFixture::new("# Intro\n");
        let cache = fixture._dir.path().join(".peitho/preview-cache");
        let old_root = cache.join("build-0");
        let previous_root = cache.join("build-1");
        fs::create_dir_all(&old_root).unwrap();
        fs::create_dir_all(&previous_root).unwrap();
        let server = RecordingPreviewReloadTarget::new(1);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_preview_once_for_watch(
            &fixture.options.input,
            &cache,
            &server,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        let current_root = cache.join("build-2");
        assert!(current_root.join("index.html").is_file());
        assert!(current_root.join("manifest.json").is_file());
        assert_eq!(server.generation(), 2);
        assert_eq!(
            &*server.events.borrow(),
            &vec![
                PreviewReloadEvent::Swap(current_root.clone()),
                PreviewReloadEvent::Broadcast
            ]
        );
        assert!(!old_root.exists());
        assert!(previous_root.is_dir());
        assert!(current_root.is_dir());
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("rebuilt 1 slide(s)"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn preview_watch_rebuild_failure_keeps_existing_root_and_generation() {
        let fixture =
            WatchFixture::new("# Intro\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```");
        let cache = fixture._dir.path().join(".peitho/preview-cache");
        let existing_root = cache.join("build-4");
        fs::create_dir_all(&existing_root).unwrap();
        let server = RecordingPreviewReloadTarget::new(4);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_preview_once_for_watch(
            &fixture.options.input,
            &cache,
            &server,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(server.generation(), 4);
        assert!(server.events.borrow().is_empty());
        assert!(existing_root.is_dir());
        assert!(!cache.join("build-5").exists());
        assert!(stdout.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("build failed:"), "actual stderr: {stderr}");
        assert!(
            stderr.contains("slot 'code' got 2 item(s)"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn emit_preview_error_page_escapes_error_and_polls_generation() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join(".peitho/preview-cache");

        let generation_dir = emit_preview_error_page(&cache, 0, "bad <deck> & broken").unwrap();

        let html = fs::read_to_string(generation_dir.join("index.html")).unwrap();
        assert!(html.contains("bad &lt;deck&gt; &amp; broken"));
        assert!(html.contains(r#"let seq = null;"#));
        assert!(html.contains("fetch('/sync')"));
        assert!(html.contains("fetch(`/sync?seq=${seq}`)"));
        assert!(html.contains("body.generation !== baselineGeneration"));
        assert!(!html.contains("seq=now"));
        assert!(!generation_dir.join("manifest.json").exists());
    }

    #[test]
    fn initial_preview_root_uses_error_page_when_first_build_fails() {
        let fixture =
            WatchFixture::new("# Intro\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```");
        let cache = fixture._dir.path().join(".peitho/preview-cache");
        let mut stderr = Vec::new();

        let root = emit_initial_preview_root(&fixture.options.input, &cache, &mut stderr).unwrap();

        assert_eq!(root, cache.join("build-0"));
        let html = fs::read_to_string(root.join("index.html")).unwrap();
        assert!(html.contains("slot 'code' got 2 item(s)"));
        assert!(!root.join("manifest.json").exists());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("build failed:"), "actual stderr: {stderr}");
        assert!(
            stderr.contains("slot 'code' got 2 item(s)"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn preview_browser_open_failure_is_reported_without_error() {
        let mut stderr = Vec::new();

        open_preview_browser_or_warn("http://127.0.0.1:4321/", &mut stderr, |_url| {
            Err(miette::miette!("open failed"))
        })
        .unwrap();

        let stderr = String::from_utf8(stderr).unwrap();
        assert!(
            stderr.contains("warning: failed to open preview browser"),
            "actual stderr: {stderr}"
        );
        assert!(stderr.contains("open failed"), "actual stderr: {stderr}");
        assert!(
            stderr.contains("help: open http://127.0.0.1:4321/ manually"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn builtin_preview_shell_matches_committed_bundle() {
        let committed = fs::read_to_string(
            workspace_root_for_tests().join("packages/peitho-present/dist/preview.js"),
        )
        .unwrap();

        assert_eq!(BUILTIN_PREVIEW_JS, committed);
    }

    #[test]
    fn builtin_remote_shell_matches_committed_bundle() {
        let committed = fs::read_to_string(
            workspace_root_for_tests().join("packages/peitho-present/dist/remote.js"),
        )
        .unwrap();

        assert_eq!(BUILTIN_REMOTE_JS, committed);
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
    fn write_script(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
    }

    #[test]
    fn new_command_defaults_and_accepts_options() {
        let cli = Cli::parse_from([
            "peitho",
            "new",
            "starter",
            "--layouts",
            "cover",
            "--theme",
            "dark",
            "--force",
        ]);

        match cli.command {
            Command::New {
                dir,
                layouts,
                theme,
                force,
            } => {
                assert_eq!(dir, PathBuf::from("starter"));
                assert_eq!(layouts, new_cmd::LayoutVariant::Cover);
                assert_eq!(theme, new_cmd::ThemeVariant::Dark);
                assert!(force);
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected new command");
            }
        }

        let cli = Cli::parse_from(["peitho", "new"]);
        match cli.command {
            Command::New {
                dir,
                layouts,
                theme,
                force,
            } => {
                assert_eq!(dir, PathBuf::from("."));
                assert_eq!(layouts, new_cmd::LayoutVariant::Default);
                assert_eq!(theme, new_cmd::ThemeVariant::Light);
                assert!(!force);
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected new command");
            }
        }
    }

    #[test]
    fn new_command_is_listed_in_help() {
        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("--help")
            .assert()
            .success();

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        let lists_new_subcommand = stdout.lines().any(|line| {
            line.strip_prefix("  new")
                .and_then(|rest| rest.chars().next())
                .is_some_and(char::is_whitespace)
        });
        assert!(lists_new_subcommand, "actual stdout: {stdout}");
    }

    #[test]
    fn new_command_scaffolds_from_cli() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("starter");

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("new")
            .arg(&target)
            .arg("--layouts")
            .arg("split")
            .arg("--theme")
            .arg("dark")
            .assert()
            .success();

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert!(stdout.contains("deck.md"), "actual stdout: {stdout}");
        assert!(stdout.contains("peitho preview"), "actual stdout: {stdout}");
        assert!(target.join("deck.md").is_file());
        assert!(target.join("layouts/title-body-code.html").is_file());
        assert!(target.join("layouts/two-column.html").is_file());
        assert!(target.join("css/base.css").is_file());
        assert!(target.join(".gitignore").is_file());
    }

    #[test]
    fn generated_scaffolds_build_for_all_layout_and_theme_combinations() {
        let layouts = [
            new_cmd::LayoutVariant::Default,
            new_cmd::LayoutVariant::Split,
            new_cmd::LayoutVariant::Cover,
        ];
        let themes = [new_cmd::ThemeVariant::Light, new_cmd::ThemeVariant::Dark];

        for layout in layouts {
            for theme in themes {
                let dir = tempfile::tempdir().unwrap();
                let target = dir
                    .path()
                    .join(format!("{layout:?}-{theme:?}").to_lowercase());
                let mut stdout = Vec::new();

                new_cmd::run(
                    new_cmd::NewOptions {
                        target: target.clone(),
                        layouts: layout,
                        theme,
                        force: false,
                    },
                    &mut stdout,
                )
                .unwrap();
                build(&BuildOptions {
                    input: target.join("deck.md"),
                    out: target.join("dist"),
                })
                .unwrap();

                assert!(
                    target.join("dist/manifest.json").is_file(),
                    "missing manifest for {layout:?}/{theme:?}"
                );
            }
        }
    }

    #[test]
    fn build_command_defaults_input_to_deck_md() {
        let cli = Cli::parse_from(["peitho", "build"]);

        match cli.command {
            Command::Build { input, .. } => {
                assert_eq!(input, PathBuf::from("deck.md"));
            }
            Command::Present { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected build command");
            }
        }
    }

    #[test]
    fn present_command_defaults_input_to_deck_md() {
        let cli = Cli::parse_from(["peitho", "present"]);

        match cli.command {
            Command::Present { input, .. } => {
                assert_eq!(input, PathBuf::from("deck.md"));
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
                panic!("expected present command");
            }
        }
    }

    #[test]
    fn export_pdf_command_defaults_input_to_deck_md() {
        let cli = Cli::parse_from(["peitho", "export", "pdf"]);

        match cli.command {
            Command::Export {
                command: ExportCommand::Pdf { input, .. },
            } => {
                assert_eq!(input, PathBuf::from("deck.md"));
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Present { .. }
            | Command::Publish { .. }
            | Command::Completions { .. } => {
                panic!("expected export pdf command");
            }
        }
    }

    #[test]
    fn build_artifacts_missing_input_error_names_path_and_default() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-deck.md");

        let err = match build_artifacts(&missing) {
            Ok(_) => panic!("expected missing input error"),
            Err(err) => err,
        };

        let message = format!("{err:?}");
        assert!(
            message.contains("no-such-deck.md"),
            "actual error: {message}"
        );
        assert!(
            message.contains("defaults to deck.md"),
            "actual error: {message}"
        );
    }

    #[test]
    fn build_command_accepts_watch_flag() {
        let cli = Cli::parse_from(["peitho", "build", "deck.md", "--watch"]);

        match cli.command {
            Command::Build { input, watch, .. } => {
                assert_eq!(input, PathBuf::from("deck.md"));
                assert!(watch);
            }
            Command::Present { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
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
            Command::Present { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Completions { .. } => {
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

    #[test]
    fn layouts_command_prints_builtin_layout_summary() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n\nBody\n").unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .assert()
            .success();

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert!(
            stdout.contains("layouts source: built-in"),
            "actual stdout: {stdout}"
        );
        assert!(
            stdout.contains("title-body-code"),
            "actual stdout: {stdout}"
        );
        assert!(
            stdout.contains("- title") && stdout.contains("accepts=inline arity=1"),
            "actual stdout: {stdout}"
        );
    }

    #[test]
    fn layouts_command_json_prints_expected_shape() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n\nBody\n").unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--json")
            .assert()
            .success();

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["source"]["kind"], "built-in");
        assert!(json["source"].get("path").is_none());
        assert_eq!(json["layouts"][0]["name"], "title-body-code");
        assert_eq!(json["layouts"][0]["slots"][0]["name"], "body");
    }

    #[test]
    fn layouts_explain_matching_slide_exits_zero() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "<!-- {\"key\":\"intro\"} -->\n# Intro\n\nBody\n").unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("intro")
            .assert()
            .success();

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert!(
            stdout.contains("slide: intro (index 0)"),
            "actual stdout: {stdout}"
        );
        assert!(
            stdout.contains("dispatch: sole layout"),
            "actual stdout: {stdout}"
        );
        assert!(
            stdout.contains("result: title-body-code"),
            "actual stdout: {stdout}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn layouts_explain_applies_code_images_transform() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("layouts");
        let command = dir.path().join("svg-command.sh");
        fs::create_dir_all(&layouts).unwrap();
        write_script(
            &command,
            "#!/bin/sh\ncat >/dev/null\nprintf '<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"></svg>'\n",
        );
        fs::write(
            layouts.join("code.html"),
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="code" accepts="code" arity="1"></slot></section>"#,
        )
        .unwrap();
        fs::write(
            layouts.join("image.html"),
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="image" accepts="image" arity="1"></slot></section>"#,
        )
        .unwrap();
        fs::write(
            &deck,
            format!(
                "---\ncode_images:\n  mermaid: /bin/sh {}\n---\n<!-- {{\"key\":\"diagram\"}} -->\n# Diagram\n\n```mermaid\ngraph TD\n```",
                command.display()
            ),
        )
        .unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("diagram")
            .arg("--json")
            .assert()
            .success();

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["dispatch"]["result"], "image");
    }

    #[test]
    fn layouts_explain_unknown_slide_key_exits_two_with_help() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "<!-- {\"key\":\"intro\"} -->\n# Intro\n").unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("missing")
            .assert()
            .code(2);

        let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
        assert!(
            stderr.contains("slide key 'missing' not found"),
            "actual stderr: {stderr}"
        );
        assert!(
            stderr.contains("help: known keys: intro"),
            "actual stderr: {stderr}"
        );
    }

    #[test]
    fn layouts_explain_unknown_slide_key_with_json_emits_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "<!-- {\"key\":\"intro\"} -->\n# Intro\n\nBody\n").unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("missing")
            .arg("--json")
            .assert()
            .code(2);

        let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
        assert_eq!(json["error"], "slide-key-not-found");
        assert_eq!(json["key"], "missing");
        assert!(json["known_keys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|key| key.as_str() == Some("intro")));
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("slide key 'missing' not found"));
    }

    #[test]
    fn layouts_explain_unknown_explicit_layout_prints_trace_and_exits_one() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            &deck,
            "<!-- {\"key\":\"bad\",\"layout\":\"missing\"} -->\n# Hi\n",
        )
        .unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("bad")
            .assert()
            .code(1);
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert!(
            stdout.contains("result: unknown layout: missing"),
            "actual stdout: {stdout}"
        );

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("bad")
            .arg("--json")
            .assert()
            .code(1);
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["dispatch"]["kind"], "explicit");
        assert_eq!(json["dispatch"]["result"]["kind"], "unknown-layout");
        assert_eq!(json["dispatch"]["result"]["layout"], "missing");
    }

    #[test]
    fn layouts_explain_dispatch_failure_exits_one_and_prints_trace() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("layouts");
        fs::create_dir_all(&layouts).unwrap();
        fs::write(
            layouts.join("cover.html"),
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        fs::write(
            layouts.join("statement.html"),
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="1..*"></slot></section>"#,
        )
        .unwrap();
        fs::write(
            &deck,
            "<!-- {\"key\":\"bad\"} -->\n# Intro\n\n```rust\nfn main() {}\n```\n",
        )
        .unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("bad")
            .assert()
            .code(1);

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert!(
            stdout.contains("layouts source: deck-adjacent"),
            "actual stdout: {stdout}"
        );
        assert!(
            stdout.contains("dispatch: structural match"),
            "actual stdout: {stdout}"
        );
        assert!(stdout.contains("rejected:"), "actual stdout: {stdout}");
        assert!(
            stdout.contains("result: no match"),
            "actual stdout: {stdout}"
        );
    }

    #[test]
    fn layouts_explain_json_includes_reason_for_sole_layout_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("layouts");
        fs::create_dir_all(&layouts).unwrap();
        fs::write(
            layouts.join("cover.html"),
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        fs::write(
            &deck,
            "<!-- {\"key\":\"bad\"} -->\n# Hi\n\n![alt](pic.png)\n",
        )
        .unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("bad")
            .arg("--json")
            .assert()
            .code(1);

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["dispatch"]["kind"], "sole-layout");
        assert_eq!(json["dispatch"]["result"]["kind"], "no-match");
        assert_eq!(json["slide"]["key"], "bad");
        let reason = json["dispatch"]["result"]["reason"].as_str().unwrap();
        assert!(!reason.is_empty());
        assert!(reason.contains("image"), "actual reason: {reason}");
    }

    #[test]
    fn layouts_explain_sole_layout_failure_prints_reason() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("layouts");
        fs::create_dir_all(&layouts).unwrap();
        fs::write(
            layouts.join("cover.html"),
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        fs::write(
            &deck,
            "<!-- {\"key\":\"hello\"} -->\n# Hello\n\n![alt](pic.png)\n",
        )
        .unwrap();

        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("layouts")
            .arg(&deck)
            .arg("--explain")
            .arg("hello")
            .assert()
            .code(1);

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert!(
            stdout.contains("dispatch: sole layout"),
            "actual stdout: {stdout}"
        );
        assert!(
            stdout.contains("result: no match"),
            "actual stdout: {stdout}"
        );
        assert!(
            stdout.contains("reason: no slot accepts image in layout 'cover'"),
            "actual stdout: {stdout}"
        );
    }

    fn contains_watch_path(paths: &[PathBuf], path: &Path) -> bool {
        let path_key = watch_path_key(path);
        paths
            .iter()
            .any(|existing| watch_path_key(existing) == path_key)
    }

    #[derive(Default)]
    struct RecordingWatchController {
        watched: Vec<PathBuf>,
        unwatched: Vec<PathBuf>,
        fail_watch: Vec<PathBuf>,
        fail_unwatch: Vec<PathBuf>,
    }

    impl WatchController for RecordingWatchController {
        fn watch_dir(&mut self, dir: &Path) -> miette::Result<()> {
            self.watched.push(dir.to_path_buf());
            if contains_watch_path(&self.fail_watch, dir) {
                return Err(miette::miette!(
                    "injected watch failure for {}",
                    dir.display()
                ));
            }
            Ok(())
        }

        fn unwatch_dir(&mut self, dir: &Path) -> miette::Result<()> {
            self.unwatched.push(dir.to_path_buf());
            if contains_watch_path(&self.fail_unwatch, dir) {
                return Err(miette::miette!(
                    "injected unwatch failure for {}",
                    dir.display()
                ));
            }
            Ok(())
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum PreviewReloadEvent {
        Swap(PathBuf),
        Broadcast,
    }

    struct RecordingPreviewReloadTarget {
        generation: Cell<u64>,
        events: RefCell<Vec<PreviewReloadEvent>>,
    }

    impl RecordingPreviewReloadTarget {
        fn new(generation: u64) -> Self {
            Self {
                generation: Cell::new(generation),
                events: RefCell::new(Vec::new()),
            }
        }
    }

    impl PreviewReloadTarget for RecordingPreviewReloadTarget {
        fn generation(&self) -> u64 {
            self.generation.get()
        }

        fn swap_root(&self, root: PathBuf) {
            self.events
                .borrow_mut()
                .push(PreviewReloadEvent::Swap(root));
        }

        fn broadcast_reload(&self) -> u64 {
            let generation = self.generation.get() + 1;
            self.generation.set(generation);
            self.events.borrow_mut().push(PreviewReloadEvent::Broadcast);
            generation
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

    fn watch_state_with_fonts() -> (tempfile::TempDir, WatchState, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let deck = root.join("deck.md");
        let fonts = root.join("fonts");
        fs::create_dir_all(&fonts).unwrap();
        fs::write(&deck, "---\nfonts: ./fonts\n---\n# Intro\n").unwrap();
        let targets = resolve_watch_targets(&deck).unwrap();
        let watched_dirs = targets.watch_dirs();
        (dir, WatchState::new(deck, targets, watched_dirs), fonts)
    }

    fn watch_state_for_fixture(fixture: &WatchFixture) -> WatchState {
        WatchState::new(
            fixture.options.input.clone(),
            fixture.targets.clone(),
            fixture.targets.watch_dirs(),
        )
    }

    fn empty_assets() -> ResolvedAssets {
        ResolvedAssets {
            layouts: Provenance::Builtin,
            css: Provenance::Builtin,
            syntaxes: Provenance::Builtin,
            fonts: Provenance::Builtin,
        }
    }

    fn workspace_root_for_tests() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .to_path_buf()
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        }
    }
}
