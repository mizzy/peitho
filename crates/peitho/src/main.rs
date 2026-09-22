#![allow(clippy::result_large_err)]

use std::{
    borrow::Cow,
    collections::{hash_map::DefaultHasher, BTreeMap, HashSet},
    env,
    error::Error,
    ffi::{OsStr, OsString},
    fmt, fs,
    hash::Hasher,
    io::{self, IsTerminal, Read, Write},
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::{Component, Path, PathBuf},
    process::{Child, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use chrono::TimeZone;
use clap::{CommandFactory, Parser, Subcommand, ValueHint};
use clap_complete::{generate, Shell};
use miette::IntoDiagnostic;
use serde::Serialize;
use sha2::{Digest, Sha256};

mod asset_resolution;
mod cdp;
mod diagnostics;
mod docs;
mod doctor;
mod lint;
mod new_cmd;

use asset_resolution::{resolve_assets, Provenance, ResolvedAssets};
use diagnostics::{plain_diagnostic_text, render_diagnostic, DeckDiagnostic, LabelStyle};
use peitho::{browser, server};
use peitho_core::domain::SlideKey;

struct BuildArtifacts {
    slide_count: usize,
    rendered: peitho_core::Deck<peitho_core::Rendered>,
    manifest_json: String,
    slide_sources_json: String,
    image_assets: Vec<peitho_core::ResolvedImageAsset>,
    fonts_source: Option<PathBuf>,
}

pub(crate) struct LoadedDeckSource {
    pub(crate) deck_path: PathBuf,
    pub(crate) source: String,
    pub(crate) frontmatter: peitho_core::ParsedFrontmatter,
    pub(crate) line_map: peitho_core::include::LineMap,
}

impl LoadedDeckSource {
    pub(crate) fn translate<T>(&self, result: peitho_core::Result<T>) -> miette::Result<T> {
        core_for_deck(result, &self.deck_path, Some(&self.line_map))
    }

    pub(crate) fn included_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        for origin in self.line_map.origins() {
            if same_watch_path(&origin.file, &self.deck_path) {
                continue;
            }
            if files.iter().any(|path| same_watch_path(path, &origin.file)) {
                continue;
            }
            files.push(origin.file.clone());
        }
        files
    }
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

struct CliEmbedRenderer;

impl peitho_core::code_images::EmbedRenderer for CliEmbedRenderer {
    fn render(
        &self,
        normalized_url: &str,
        params: peitho_core::code_images::EmbedRenderParams,
    ) -> peitho_core::Result<Vec<u8>> {
        let chrome = locate_chrome().map_err(|err| {
            // A `Report` interpolated as `{err}` prints only its message; the
            // structured help must ride into the BuildError's own help or it
            // would be silently dropped.
            let help = diagnostics::report_help(&err).unwrap_or_default();
            cli_embed_renderer_error(&err, help)
        })?;
        let temp = tempfile::tempdir().map_err(|err| {
            cli_embed_renderer_error(
                format!("failed to create temporary tweet embed workspace: {err}"),
                "make the system temporary directory writable and retry",
            )
        })?;
        render_embed_with_chrome(&chrome, temp.path(), normalized_url, params).map_err(|err| {
            const RETRY_HELP: &str = "retry with Chrome and network access to X; set PEITHO_CHROME_PATH=<absolute-path> to choose Chrome";
            let help = match diagnostics::report_help(&err) {
                Some(inner) => format!("{inner}; {RETRY_HELP}"),
                None => RETRY_HELP.to_string(),
            };
            cli_embed_renderer_error(&err, help)
        })
    }
}

const OEMBED_CURL_MAX_TIME_SECS: u64 = 30;
const OEMBED_CURL_RUNNER_MARGIN_SECS: u64 = 5;
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenericOEmbedFetchOperation {
    DiscoveryPage,
    DiscoveredEndpoint,
    Thumbnail,
}

impl GenericOEmbedFetchOperation {
    const fn max_bytes(self) -> usize {
        match self {
            Self::DiscoveryPage => peitho_core::MAX_OEMBED_DISCOVERY_PAGE_BYTES,
            Self::DiscoveredEndpoint => peitho_core::MAX_OEMBED_RESPONSE_BYTES,
            Self::Thumbnail => peitho_core::MAX_OEMBED_THUMBNAIL_BYTES,
        }
    }
}

struct CliOEmbedFetcher;

impl peitho_core::code_images::OEmbedFetcher for CliOEmbedFetcher {
    fn fetch(&self, normalized_url: &str) -> peitho_core::Result<String> {
        fetch_oembed_with_invoker(normalized_url, &SystemOEmbedCurlInvoker)
    }

    fn fetch_discovery_page(&self, page_url: &str) -> peitho_core::Result<Vec<u8>> {
        fetch_generic_oembed_with_invoker(
            page_url,
            GenericOEmbedFetchOperation::DiscoveryPage,
            &SystemOEmbedCurlInvoker,
        )
    }

    fn fetch_discovered_oembed(&self, endpoint_url: &str) -> peitho_core::Result<Vec<u8>> {
        fetch_generic_oembed_with_invoker(
            endpoint_url,
            GenericOEmbedFetchOperation::DiscoveredEndpoint,
            &SystemOEmbedCurlInvoker,
        )
    }

    fn fetch_thumbnail(&self, image_url: &str) -> peitho_core::Result<Vec<u8>> {
        fetch_generic_oembed_with_invoker(
            image_url,
            GenericOEmbedFetchOperation::Thumbnail,
            &SystemOEmbedCurlInvoker,
        )
    }
}

#[derive(Debug)]
enum OEmbedCurlOutcome {
    Exited {
        success: bool,
        code: Option<i32>,
        status: String,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    TimedOut {
        stderr: Vec<u8>,
    },
}

trait OEmbedCurlInvoker {
    fn invoke(
        &self,
        program: &OsStr,
        args: &[OsString],
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<OEmbedCurlOutcome, ProcessRunError>;
}

struct SystemOEmbedCurlInvoker;

impl OEmbedCurlInvoker for SystemOEmbedCurlInvoker {
    fn invoke(
        &self,
        program: &OsStr,
        args: &[OsString],
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<OEmbedCurlOutcome, ProcessRunError> {
        match run_child_with_timeout(program, args, stdin, timeout, |_, _| false)? {
            ProcessOutcome::Ready { .. } => {
                unreachable!("oEmbed curl completion predicate never fires")
            }
            ProcessOutcome::Exited {
                status,
                stdout,
                stderr,
            } => Ok(OEmbedCurlOutcome::Exited {
                success: status.success(),
                code: status.code(),
                status: status.to_string(),
                stdout,
                stderr,
            }),
            ProcessOutcome::TimedOut { stderr } => Ok(OEmbedCurlOutcome::TimedOut { stderr }),
        }
    }
}

fn fetch_oembed_with_invoker<I: OEmbedCurlInvoker>(
    normalized_url: &str,
    invoker: &I,
) -> peitho_core::Result<String> {
    let endpoint = peitho_core::builtin_oembed_request_url(normalized_url);
    debug_assert!(endpoint.starts_with("https://publish.x.com/oembed?"));
    let args = [
        OsString::from("-fsS"),
        OsString::from("--max-time"),
        OsString::from(OEMBED_CURL_MAX_TIME_SECS.to_string()),
        OsString::from("--max-filesize"),
        OsString::from(peitho_core::MAX_OEMBED_RESPONSE_BYTES.to_string()),
        OsString::from(endpoint),
    ];
    let timeout = Duration::from_secs(OEMBED_CURL_MAX_TIME_SECS + OEMBED_CURL_RUNNER_MARGIN_SECS);
    let outcome = invoker
        .invoke(OsStr::new("curl"), &args, None, timeout)
        .map_err(oembed_curl_process_error)?;

    match outcome {
        OEmbedCurlOutcome::Exited {
            success: true,
            stdout,
            ..
        } => String::from_utf8(stdout).map_err(|err| {
            cli_oembed_fetcher_error(
                format!("curl returned non-UTF-8 oEmbed data: {err}"),
                "retry the oEmbed fetch; X must return UTF-8 JSON",
            )
        }),
        OEmbedCurlOutcome::Exited {
            success: false,
            code,
            status,
            stderr,
            ..
        } => {
            let help = if code == Some(22) {
                "the X post may have been deleted or made private; check network access to publish.x.com and retry"
            } else {
                "check network access to publish.x.com and retry with curl installed"
            };
            Err(cli_oembed_fetcher_error(
                format!(
                    "curl exited with {status}; stderr: {}",
                    stderr_excerpt(&stderr)
                ),
                help,
            ))
        }
        OEmbedCurlOutcome::TimedOut { stderr } => Err(cli_oembed_fetcher_error(
            format!(
                "curl timed out after {}s; stderr: {}",
                timeout.as_secs(),
                stderr_excerpt(&stderr)
            ),
            "retry with network access to publish.x.com",
        )),
    }
}

fn fetch_generic_oembed_with_invoker<I: OEmbedCurlInvoker>(
    url: &str,
    operation: GenericOEmbedFetchOperation,
    invoker: &I,
) -> peitho_core::Result<Vec<u8>> {
    let args = [
        OsString::from("-fsS"),
        OsString::from("-L"),
        OsString::from("--max-redirs"),
        OsString::from("5"),
        OsString::from("--proto"),
        OsString::from("=http,https"),
        OsString::from("--proto-redir"),
        OsString::from("=http,https"),
        OsString::from("--max-time"),
        OsString::from(OEMBED_CURL_MAX_TIME_SECS.to_string()),
        OsString::from("--max-filesize"),
        OsString::from(operation.max_bytes().to_string()),
        OsString::from(url),
    ];
    let timeout = Duration::from_secs(OEMBED_CURL_MAX_TIME_SECS + OEMBED_CURL_RUNNER_MARGIN_SECS);
    let outcome = invoker
        .invoke(OsStr::new("curl"), &args, None, timeout)
        .map_err(oembed_curl_process_error)?;

    match outcome {
        OEmbedCurlOutcome::Exited {
            success: true,
            stdout,
            ..
        } => Ok(stdout),
        OEmbedCurlOutcome::Exited {
            success: false,
            status,
            stderr,
            ..
        } => Err(cli_oembed_fetcher_error(
            format!(
                "generic oEmbed curl exited with {status}; stderr: {}",
                stderr_excerpt(&stderr)
            ),
            "check HTTP(S) network access, provider redirects, and the URL, then retry with curl installed",
        )),
        OEmbedCurlOutcome::TimedOut { stderr } => Err(cli_oembed_fetcher_error(
            format!(
                "generic oEmbed curl timed out after {}s; stderr: {}",
                timeout.as_secs(),
                stderr_excerpt(&stderr)
            ),
            "check HTTP(S) network access and retry; the provider must respond within the bounded timeout",
        )),
    }
}

fn oembed_curl_process_error(err: ProcessRunError) -> peitho_core::BuildError {
    match err {
        ProcessRunError::Spawn(err) => cli_oembed_fetcher_error(
            format!("failed to start curl: {err}"),
            "install curl and retry; curl ships with macOS, common Linux CI images, and Windows 10+",
        ),
        ProcessRunError::CaptureStdout => cli_oembed_fetcher_error(
            "failed to capture curl stdout",
            "retry the oEmbed fetch and report the process setup failure",
        ),
        ProcessRunError::CaptureStderr => cli_oembed_fetcher_error(
            "failed to capture curl stderr",
            "retry the oEmbed fetch and report the process setup failure",
        ),
        ProcessRunError::CaptureStdin => cli_oembed_fetcher_error(
            "failed to capture curl stdin",
            "report this error; the oEmbed curl command must not receive stdin",
        ),
        ProcessRunError::Wait(err) => cli_oembed_fetcher_error(
            format!("failed to wait on curl: {err}"),
            "retry the oEmbed fetch",
        ),
        ProcessRunError::Kill(err) => cli_oembed_fetcher_error(
            format!("failed to terminate curl: {err}"),
            "report the underlying process error",
        ),
    }
}

fn cli_oembed_fetcher_error(
    message: impl Into<String>,
    help: impl Into<String>,
) -> peitho_core::BuildError {
    peitho_core::BuildError::new(peitho_core::error::ErrorKind::Asset, None, message, help)
}

fn cli_embed_renderer_error(
    message: impl fmt::Display,
    help: impl Into<String>,
) -> peitho_core::BuildError {
    peitho_core::BuildError::new(
        peitho_core::error::ErrorKind::Asset,
        None,
        message.to_string(),
        help,
    )
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
    directory_candidate: bool,
}

#[derive(Debug, Clone)]
struct WatchTargets {
    roots: Vec<WatchRoot>,
    assets: ResolvedAssets,
}

struct WatchState {
    input: PathBuf,
    targets: WatchTargets,
    input_snapshot: InputSnapshot,
    candidate_snapshot: Option<InputSnapshot>,
    /// Label style for the notes this state emits. It rides the struct rather
    /// than each method's argument list because the notes are written to a
    /// `&mut dyn Write`, which cannot report whether it is a terminal.
    style: LabelStyle,
}

impl WatchState {
    fn new(input: PathBuf, targets: WatchTargets, style: LabelStyle) -> Self {
        let input_snapshot = capture_input_snapshot(&targets);
        Self {
            input,
            targets,
            input_snapshot,
            candidate_snapshot: None,
            style,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputFingerprint {
    Content(u64),
    Metadata {
        modified: Option<SystemTime>,
        len: u64,
    },
    Directory,
    Symlink {
        target: Option<PathBuf>,
        modified: Option<SystemTime>,
        len: Option<u64>,
        content: Option<u64>,
    },
    Missing,
    Other,
}

type InputSnapshot = BTreeMap<PathBuf, InputFingerprint>;

impl WatchTargets {
    /// The deck file, included Markdown, referenced images, and resolved asset
    /// paths. Each asset path may be a single file or a directory whose
    /// matching extension files are watched.
    fn new(
        input: PathBuf,
        assets: ResolvedAssets,
        included_files: Vec<PathBuf>,
        image_files: Vec<PathBuf>,
    ) -> Self {
        let mut roots = vec![WatchRoot::input(input.clone(), Some("md"))];
        roots.extend(
            included_files
                .iter()
                .cloned()
                .map(|path| WatchRoot::input(path, Some("md"))),
        );
        roots.extend(
            image_files
                .into_iter()
                .map(|path| WatchRoot::input(path, None)),
        );
        roots.push(WatchRoot::asset(
            &input,
            &assets.layouts,
            "layouts",
            Some("html"),
        ));
        roots.push(WatchRoot::asset(&input, &assets.css, "css", Some("css")));
        roots.push(WatchRoot::asset(
            &input,
            &assets.overrides,
            "overrides",
            Some("css"),
        ));
        roots.push(WatchRoot::asset(
            &input,
            &assets.syntaxes,
            "syntaxes",
            Some("sublime-syntax"),
        ));
        roots.push(WatchRoot::asset(&input, &assets.fonts, "fonts", None));
        Self { roots, assets }
    }
}

impl WatchRoot {
    fn input(path: PathBuf, ext: Option<&'static str>) -> Self {
        Self {
            path,
            ext,
            directory_candidate: false,
        }
    }

    fn asset(
        input: &Path,
        provenance: &Provenance,
        name: &'static str,
        ext: Option<&'static str>,
    ) -> Self {
        match provenance.path() {
            Some(path) => Self::input(path.to_path_buf(), ext),
            None => Self {
                path: asset_resolution::deck_parent(input).join(name),
                ext,
                directory_candidate: true,
            },
        }
    }
}

fn capture_input_snapshot(targets: &WatchTargets) -> InputSnapshot {
    let mut snapshot = BTreeMap::new();
    for root in &targets.roots {
        capture_watch_root(root, &mut snapshot);
    }
    snapshot
}

fn content_hash(path: &Path) -> Option<u64> {
    let bytes = fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    hasher.write(&bytes);
    Some(hasher.finish())
}

fn content_fingerprint(path: &Path) -> InputFingerprint {
    content_hash(path)
        .map(InputFingerprint::Content)
        .unwrap_or(InputFingerprint::Other)
}

fn metadata_fingerprint(metadata: &fs::Metadata) -> InputFingerprint {
    InputFingerprint::Metadata {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    }
}

fn directory_fingerprint() -> InputFingerprint {
    InputFingerprint::Directory
}

fn symlink_fingerprint(path: &Path, hash_content: bool) -> InputFingerprint {
    let metadata = fs::metadata(path).ok();
    InputFingerprint::Symlink {
        target: fs::read_link(path).ok(),
        modified: metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok()),
        len: metadata.as_ref().map(fs::Metadata::len),
        content: hash_content.then(|| content_hash(path)).flatten(),
    }
}

fn capture_watch_root(root: &WatchRoot, snapshot: &mut InputSnapshot) {
    let Ok(metadata) = fs::symlink_metadata(&root.path) else {
        snapshot.insert(root.path.clone(), InputFingerprint::Missing);
        return;
    };

    if metadata.file_type().is_symlink() {
        let followed = fs::metadata(&root.path).ok();
        if root.directory_candidate && !followed.as_ref().is_some_and(fs::Metadata::is_dir) {
            snapshot.insert(root.path.clone(), symlink_fingerprint(&root.path, false));
            return;
        }
        let target_is_file = followed.as_ref().is_some_and(fs::Metadata::is_file);
        snapshot.insert(
            root.path.clone(),
            symlink_fingerprint(&root.path, root.ext.is_some() && target_is_file),
        );
        if followed.as_ref().is_some_and(fs::Metadata::is_dir) {
            capture_directory_contents(root, snapshot);
        }
        return;
    }

    if root.directory_candidate && !metadata.is_dir() {
        let fingerprint = if metadata.is_file() {
            metadata_fingerprint(&metadata)
        } else {
            InputFingerprint::Other
        };
        snapshot.insert(root.path.clone(), fingerprint);
        return;
    }

    if metadata.is_file() {
        let fingerprint = match root.ext {
            Some(_) => content_fingerprint(&root.path),
            None => metadata_fingerprint(&metadata),
        };
        snapshot.insert(root.path.clone(), fingerprint);
        return;
    }

    if metadata.is_dir() {
        snapshot.insert(root.path.clone(), directory_fingerprint());
        capture_directory_contents(root, snapshot);
    } else {
        snapshot.insert(root.path.clone(), InputFingerprint::Other);
    }
}

fn capture_directory_contents(root: &WatchRoot, snapshot: &mut InputSnapshot) {
    match root.ext {
        Some(ext) => capture_text_directory(&root.path, ext, snapshot),
        None => capture_binary_directory(&root.path, snapshot),
    }
}

fn capture_text_directory(path: &Path, ext: &str, snapshot: &mut InputSnapshot) {
    let Ok(files) = collect_asset_files(path, ext) else {
        return;
    };
    for path in files {
        snapshot.insert(path.clone(), content_fingerprint(&path));
    }
}

fn capture_binary_directory(path: &Path, snapshot: &mut InputSnapshot) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        let fingerprint = if metadata.file_type().is_symlink() {
            symlink_fingerprint(&path, false)
        } else if metadata.is_file() {
            metadata_fingerprint(&metadata)
        } else if metadata.is_dir() {
            directory_fingerprint()
        } else {
            InputFingerprint::Other
        };
        let is_dir = metadata.is_dir();
        snapshot.entry(path.clone()).or_insert(fingerprint);
        if is_dir {
            capture_binary_directory(&path, snapshot);
        }
    }
}

struct PresentOptions {
    input: PathBuf,
    shell: Option<PathBuf>,
    port: Option<u16>,
    no_open: bool,
    no_serve: bool,
    no_presenter: bool,
    presenter_windowed: bool,
    host: Option<Option<IpAddr>>,
    rehearsal: bool,
    audio: bool,
}

struct RehearsalOptions {
    all: bool,
    rehearsals_dir: PathBuf,
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
#[command(
    long_about = "Build HTML-native presentations from Markdown.\n\nRun `peitho docs` for the embedded guide, available offline and suitable for agent reference."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scaffold a starter deck into a directory.
    New {
        #[arg(default_value = ".", value_hint = ValueHint::DirPath)]
        dir: PathBuf,
        #[arg(
            long,
            value_enum,
            default_value_t = new_cmd::LayoutVariant::Default,
            help = "Layout variant to scaffold"
        )]
        layouts: new_cmd::LayoutVariant,
        #[arg(
            long,
            value_enum,
            default_value_t = new_cmd::ThemeVariant::Light,
            help = "Theme variant to scaffold"
        )]
        theme: new_cmd::ThemeVariant,
        #[arg(
            long,
            help = "overwrite the scaffold-owned files (deck.md, layouts/, css/base.css, .gitignore) in a non-empty directory"
        )]
        force: bool,
    },
    /// Build the distributable dist/ directory.
    Build {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(
            long,
            default_value = "dist",
            value_hint = ValueHint::DirPath,
            help = "Output directory"
        )]
        out: PathBuf,
        #[arg(long, help = "Rebuild on every change to the deck and its assets")]
        watch: bool,
    },
    /// Render every slide in headless Chrome and report overflow.
    Lint {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
    },
    /// Print the resolved layouts and their slot contracts.
    Layouts {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(
            long,
            value_name = "SLIDE",
            help = "Explain layout dispatch for the slide with this key"
        )]
        explain: Option<String>,
        #[arg(long, help = "Print as JSON")]
        json: bool,
    },
    /// Diagnose the runtime environment and deck asset resolution.
    Doctor {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(long, help = "Print as JSON")]
        json: bool,
    },
    /// Watch, serve, and reload the deck on every successful rebuild.
    Preview {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(
            long,
            default_value_t = 0,
            help = "Port for the preview server (0 picks a random port)"
        )]
        port: u16,
        #[arg(long, help = "Do not open the browser")]
        no_open: bool,
    },
    /// Present the deck full-screen with the presenter view.
    Present {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(long, help = "shell bundle path (default: built-in present shell)")]
        shell: Option<PathBuf>,
        #[arg(long, help = present_port_help())]
        port: Option<u16>,
        #[arg(long, help = "Do not open the browser")]
        no_open: bool,
        #[arg(long, help = "Only write the present cache; start no server")]
        no_serve: bool,
        #[arg(long, help = "Open the slides window only")]
        no_presenter: bool,
        #[arg(long, help = "Open the presenter view windowed instead of full-screen")]
        presenter_windowed: bool,
        #[arg(long, help = "Record per-section actuals to .peitho/rehearsals/")]
        rehearsal: bool,
        #[arg(
            long,
            requires = "rehearsal",
            conflicts_with = "no_presenter",
            help = "Record timer-aligned microphone audio from the presenter view to .peitho/rehearsals/ (requires --rehearsal)"
        )]
        audio: bool,
        #[arg(
            long,
            value_name = "IP",
            num_args = 0..=1,
            help = "Expose /remote on an optional IP; bare --host picks the best address automatically (VPN, e.g. Tailscale, preferred)"
        )]
        host: Option<Option<IpAddr>>,
    },
    /// Print recorded rehearsals as a section timing table.
    Rehearsal {
        #[arg(long, help = "Print every recorded rehearsal, oldest first")]
        all: bool,
    },
    /// Check the built output, then run a deploy command against it.
    Publish {
        #[arg(
            long,
            default_value = "dist",
            value_hint = ValueHint::DirPath,
            help = "Built output directory to publish"
        )]
        dist: PathBuf,
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_hint = ValueHint::CommandWithArguments
        )]
        command: Vec<OsString>,
    },
    /// Export the deck to another format.
    Export {
        #[command(subcommand)]
        command: ExportCommand,
    },
    /// Read the embedded guide offline.
    Docs {
        #[arg(
            value_name = "TOPIC",
            conflicts_with = "all",
            value_parser = clap::builder::PossibleValuesParser::new(docs::topic_slugs()),
            help = "Guide topic slug; run `peitho docs` to list them"
        )]
        topic: Option<String>,
        #[arg(long, help = "Print every guide page in order")]
        all: bool,
    },
    /// Generate a shell completion script.
    Completions { shell: Shell },
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    /// Export a PDF.
    Pdf {
        #[arg(default_value = "deck.md")]
        input: PathBuf,
        #[arg(short, long, help = "Output path (default: <INPUT stem>.pdf)")]
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
const BUILTIN_CSS_FILE_NAME: &str = "base.css (built-in)";
/// The committed esbuild bundle; CI rebuilds it and fails on drift, the same
/// discipline as the generated TS types in bindings/.
const BUILTIN_SHELL_JS: &str = include_str!("../../../packages/peitho-present/dist/shell.js");
const BUILTIN_PREVIEW_JS: &str = include_str!("../../../packages/peitho-present/dist/preview.js");
const BUILTIN_REMOTE_JS: &str = include_str!("../../../packages/peitho-present/dist/remote.js");

// Keeps remote URLs and the microphone-permission origin stable across runs.
const STABLE_PRESENT_PORT: u16 = 6173;

fn present_port_help() -> String {
    format!(
        "Port for the present server (default: random; with --host or --audio and no --port: {}; pass 0 for an OS-assigned random port)",
        STABLE_PRESENT_PORT
    )
}

const PRESENT_CACHE: &str = ".peitho/present-cache";
const PREVIEW_CACHE: &str = ".peitho/preview-cache";
const REHEARSALS_DIR: &str = ".peitho/rehearsals";
const PRESENTATION_ONLY_DIST_FILES: &[&str] = &[
    "present.html",
    "presenter.html",
    "remote.html",
    "notes.json",
    "sources.json",
    "shell.js",
    "remote.js",
];
const PUBLISH_CONTAMINATION_HELP: &str =
    "remove presentation artifacts or run `peitho build` again";

fn main() {
    if let Err(err) = run() {
        // Printing through the shared renderer (instead of returning the error
        // and letting the runtime print `Error: {report:?}`) keeps this path
        // byte-identical to the swallowed watch/preview error paths and avoids
        // the runtime prefix colliding with the renderer's own `error:` label.
        eprintln!("{}", render_diagnostic(&err));
        std::process::exit(1);
    }
}

fn run() -> miette::Result<()> {
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
            let style = diagnostics::LabelStyle::for_stream(&stdout);
            let code = lint::run(input, &mut stdout, style)?;
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
            rehearsal,
            audio,
            host,
        } => present(PresentOptions {
            input,
            shell,
            port,
            no_open,
            no_serve,
            no_presenter,
            presenter_windowed,
            rehearsal,
            audio,
            host,
        }),
        Command::Rehearsal { all } => {
            let mut stdout = std::io::stdout();
            let style = LabelStyle::for_stream(&stdout);
            run_rehearsal(
                RehearsalOptions {
                    all,
                    rehearsals_dir: PathBuf::from(REHEARSALS_DIR),
                },
                &mut stdout,
                style,
            )
        }
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
        Command::Docs { topic, all } => {
            let output = core(docs::render(topic.as_deref(), all))?;
            match std::io::stdout().write_all(output.as_bytes()) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                Err(err) => Err(err).into_diagnostic(),
            }
        }
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
    let loaded = load_and_expand_deck_source(&input)?;
    let assets = resolve_assets(&input, &loaded.frontmatter)?;
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
    let parsed = loaded.translate(peitho_core::code_images::parse_deck_and_transform(
        &loaded.source,
        loaded.frontmatter.clone(),
        &highlighter,
        &CliSvgRunner::for_deck(&input),
        &CliEmbedRenderer,
        &CliOEmbedFetcher,
        &code_images_cache_dir(&input),
        &embeds_cache_dir(&input),
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
        eprintln!(
            "{}",
            render_diagnostic(&miette::Report::new(DeckDiagnostic::new(err)))
        );
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
        Provenance::Absent => "none".to_owned(),
    }
}

fn keep_workspace_for_error(tmp: tempfile::TempDir, err: miette::Report) -> miette::Report {
    let kept = tmp.keep();
    let kept_note = format!("workspace kept at {}", kept.display());
    let help = match diagnostics::report_help(&err) {
        Some(inner) => format!("{inner}\n{kept_note}"),
        None => kept_note,
    };
    miette::miette!(help = help, "{err}")
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
                writeln!(stderr, "build failed:\n{}", render_diagnostic(&err)).into_diagnostic()?;
                stderr.flush().into_diagnostic()?;
            }
        },
        Err(err) => {
            writeln!(stderr, "build failed:\n{}", render_diagnostic(&err)).into_diagnostic()?;
            stderr.flush().into_diagnostic()?;
        }
    }

    Ok(())
}

fn handle_watch_tick<F>(
    state: &mut WatchState,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    rebuild: &mut F,
) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    let current = capture_input_snapshot(&state.targets);
    // A truncate, partial write, or in-progress binary copy is only a
    // candidate. It must produce the same capture on the following tick
    // before it can trigger a rebuild.
    let Some(stable) = stable_snapshot_candidate(
        &state.input_snapshot,
        &mut state.candidate_snapshot,
        current,
    ) else {
        return Ok(());
    };

    refresh_watch_targets(state, stderr)?;
    let refreshed = capture_input_snapshot(&state.targets);
    if refreshed != stable {
        state.candidate_snapshot = Some(refreshed);
        return Ok(());
    }

    rebuild_with_input_snapshot(state, refreshed, stdout, stderr, rebuild)
}

fn stable_snapshot_candidate(
    installed: &InputSnapshot,
    candidate: &mut Option<InputSnapshot>,
    current: InputSnapshot,
) -> Option<InputSnapshot> {
    if &current == installed {
        *candidate = None;
        return None;
    }
    if candidate.as_ref() == Some(&current) {
        *candidate = None;
        return Some(current);
    }
    *candidate = Some(current);
    None
}

fn rebuild_with_input_snapshot<F>(
    state: &mut WatchState,
    input_snapshot: InputSnapshot,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    rebuild: &mut F,
) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    // Install the stable capture made before the build. Bytes written while
    // the build runs must not be marked as consumed; the next tick will see
    // them and may intentionally cause one extra rebuild.
    let result = rebuild(stdout, stderr);
    state.input_snapshot = input_snapshot;
    result
}

fn run_initial_action_after_watch_snapshot<T, F>(
    input: PathBuf,
    initial_action: F,
) -> miette::Result<(WatchState, T)>
where
    F: FnOnce() -> miette::Result<T>,
{
    // The initial build may overlap a save just like any later rebuild. Keep
    // the pre-build capture so bytes arriving during that action remain new.
    let state = prepare_watch_loop(input);
    let value = initial_action()?;
    Ok((state, value))
}

fn watch_build(options: BuildOptions) -> miette::Result<()> {
    let (state, ()) = run_initial_action_after_watch_snapshot(options.input.clone(), || {
        println!("watching deck, referenced images, and resolved asset paths");
        rebuild_once_for_watch(&options, &mut std::io::stdout(), &mut std::io::stderr())
    })?;
    watch_paths_loop(state, move |stdout, stderr| {
        rebuild_once_for_watch(&options, stdout, stderr)
    })
}

fn watch_paths_loop<F>(mut state: WatchState, mut rebuild: F) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    loop {
        thread::sleep(WATCH_POLL_INTERVAL);
        handle_watch_tick(
            &mut state,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
            &mut rebuild,
        )?;
    }
}

fn prepare_watch_loop(input: PathBuf) -> WatchState {
    let targets = resolve_watch_targets_or_deck_only(&input);
    WatchState::new(input, targets, LabelStyle::for_stderr())
}

fn resolve_watch_targets(input: &Path) -> miette::Result<WatchTargets> {
    let loaded = load_and_expand_deck_source(input)?;
    let assets = resolve_assets(input, &loaded.frontmatter)?;
    let image_files = load_highlighter(assets.syntaxes.path())
        .ok()
        .and_then(|highlighter| {
            peitho_core::referenced_image_paths(
                &loaded.source,
                loaded.frontmatter.clone(),
                &highlighter,
            )
            .ok()
        })
        .map(|image_paths| {
            image_paths
                .into_iter()
                .map(|raw| asset_resolution::deck_parent(input).join(raw.as_str()))
                .collect()
        })
        .unwrap_or_default();
    Ok(WatchTargets::new(
        input.to_path_buf(),
        assets,
        loaded.included_files(),
        image_files,
    ))
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
            overrides: Provenance::Absent,
            syntaxes: Provenance::Builtin,
            fonts: Provenance::Absent,
        },
        Vec::new(),
        Vec::new(),
    )
}

fn refresh_watch_targets(state: &mut WatchState, stderr: &mut dyn Write) -> miette::Result<()> {
    let current_targets = match resolve_watch_targets(&state.input) {
        Ok(targets) => targets,
        Err(_) => {
            return Ok(());
        }
    };
    let asset_paths_changed =
        resolved_asset_paths_changed(&state.targets.assets, &current_targets.assets);
    state.targets = current_targets;
    if !asset_paths_changed {
        return Ok(());
    }
    writeln!(
        stderr,
        "{}watching new asset paths from frontmatter: {}",
        state.style.note(),
        describe_resolved_assets(&state.targets.assets)
    )
    .into_diagnostic()?;
    stderr.flush().into_diagnostic()?;
    Ok(())
}

fn resolved_asset_paths_changed(old: &ResolvedAssets, new: &ResolvedAssets) -> bool {
    old.layouts.path() != new.layouts.path()
        || old.css.path() != new.css.path()
        || old.overrides.path() != new.overrides.path()
        || old.syntaxes.path() != new.syntaxes.path()
        || old.fonts.path() != new.fonts.path()
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
    if let Some(path) = assets.overrides.path() {
        parts.push(format!(
            "overrides={}({})",
            assets.overrides.kind(),
            path.display()
        ));
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
            help = format!("pass a .{ext} file or a directory containing them"),
            "cannot read {}\ncaused by: {err}",
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
            help = format!("add at least one .{ext} file to the directory"),
            "no *.{ext} files in {}",
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

fn builtin_css_file() -> peitho_core::CssFile {
    peitho_core::CssFile {
        name: BUILTIN_CSS_FILE_NAME.to_owned(),
        content: BUILTIN_BASE_CSS.to_owned(),
    }
}

fn load_css_files(path: &Path) -> miette::Result<Vec<peitho_core::CssFile>> {
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

fn load_css(
    css_path: Option<&Path>,
    overrides_path: Option<&Path>,
) -> miette::Result<Vec<peitho_core::CssFile>> {
    let mut files = match css_path {
        Some(path) => load_css_files(path)?,
        None => vec![builtin_css_file()],
    };
    if let Some(path) = overrides_path {
        files.extend(load_css_files(path)?);
    }
    Ok(files)
}

fn resolve_assets_and_highlighter(
    input: &Path,
    frontmatter: &peitho_core::ParsedFrontmatter,
) -> miette::Result<(ResolvedAssets, peitho_core::highlight::Highlighter)> {
    let assets = resolve_assets(input, frontmatter)?;
    let highlighter = load_highlighter(assets.syntaxes.path())?;
    Ok((assets, highlighter))
}

fn build_artifacts(input: &Path) -> miette::Result<BuildArtifacts> {
    build_cli_artifacts(input, peitho_core::EditAnnotations::Off)
}

fn build_preview_artifacts(input: &Path) -> miette::Result<BuildArtifacts> {
    build_cli_artifacts(input, peitho_core::EditAnnotations::On)
}

fn build_cli_artifacts(
    input: &Path,
    edit_annotations: peitho_core::EditAnnotations,
) -> miette::Result<BuildArtifacts> {
    let svg_runner = CliSvgRunner::for_deck(input);
    build_artifacts_with_services(
        input,
        &svg_runner,
        &CliEmbedRenderer,
        &CliOEmbedFetcher,
        edit_annotations,
    )
}

fn build_artifacts_with_services<S, E, F>(
    input: &Path,
    svg_runner: &S,
    embed_renderer: &E,
    oembed_fetcher: &F,
    edit_annotations: peitho_core::EditAnnotations,
) -> miette::Result<BuildArtifacts>
where
    S: peitho_core::code_images::SvgRunner,
    E: peitho_core::code_images::EmbedRenderer,
    F: peitho_core::code_images::OEmbedFetcher,
{
    let loaded = load_and_expand_deck_source(input)?;
    let (assets, highlighter) = resolve_assets_and_highlighter(input, &loaded.frontmatter)?;
    let layouts = load_layouts(assets.layouts.path())?;
    let css_files = load_css(assets.css.path(), assets.overrides.path())?;
    let parsed = loaded.translate(peitho_core::code_images::parse_deck_and_transform(
        &loaded.source,
        loaded.frontmatter.clone(),
        &highlighter,
        svg_runner,
        embed_renderer,
        oembed_fetcher,
        &code_images_cache_dir(input),
        &embeds_cache_dir(input),
    ))?;
    let slide_sources_json = core(peitho_core::slide_sources_json(
        &peitho_core::SlideSources::from_slides(
            &loaded.source,
            parsed.parsed_slides(),
            &highlighter,
        ),
    ))?;
    let mapped = loaded.translate(peitho_core::dispatch_by_convention(parsed, &layouts))?;
    let checked = loaded.translate(peitho_core::check_deck(mapped))?;
    let slide_count = checked.slide_count();
    let theme_css = core(peitho_core::build_theme_css(
        &css_files,
        &checked.slide_slot_classes(),
        &layouts.slot_classes(),
        &layouts.root_classes(),
    ))?;
    let mut image_resolver = ImageResolver::new(input);
    let (resolved, image_assets) = loaded
        .translate(peitho_core::resolve_image_paths(checked, |request| {
            image_resolver.resolve(request)
        }))?;
    let manifest = peitho_core::build_manifest(&resolved, &image_assets);
    let manifest_json = core(peitho_core::manifest_json(&manifest))?;
    let rendered = loaded.translate(peitho_core::render_deck(
        resolved,
        &highlighter,
        theme_css,
        edit_annotations,
    ))?;

    Ok(BuildArtifacts {
        slide_count,
        rendered,
        manifest_json,
        slide_sources_json,
        image_assets,
        fonts_source: assets.fonts.path().map(Path::to_path_buf),
    })
}

pub(crate) fn load_and_expand_deck_source(input: &Path) -> miette::Result<LoadedDeckSource> {
    let markdown = read_deck_source(input)?;
    let frontmatter = core_for_deck(peitho_core::parse_frontmatter(&markdown), input, None)?;
    let expanded = core_for_deck(
        peitho_core::include::expand_includes(&markdown, frontmatter.body_start(), input),
        input,
        None,
    )?;
    Ok(LoadedDeckSource {
        deck_path: input.to_path_buf(),
        source: expanded.source,
        frontmatter,
        line_map: expanded.line_map,
    })
}

fn classify_preview_deck_report(report: miette::Report) -> server::DeckWriteError {
    let is_conflict = report.downcast_ref::<DeckDiagnostic>().is_some();
    let message = plain_diagnostic_text(&report);
    if is_conflict {
        server::DeckWriteError::Conflict(message)
    } else {
        server::DeckWriteError::Io(message)
    }
}

fn preview_deck_conflict(error: peitho_core::BuildError) -> server::DeckWriteError {
    let report = miette::Report::new(DeckDiagnostic::new(error));
    server::DeckWriteError::Conflict(plain_diagnostic_text(&report))
}

fn preview_deck_drift_conflict() -> server::DeckWriteError {
    server::DeckWriteError::Conflict("the deck changed on disk; reload and retry".to_owned())
}

fn preview_deck_missing_slide_conflict(key: &SlideKey) -> server::DeckWriteError {
    preview_deck_conflict(peitho_core::BuildError::new(
        peitho_core::error::ErrorKind::Parse,
        None,
        format!("slide key '{}' not found in current deck", key.as_str()),
        "reload preview and retry on a slide whose key still exists",
    ))
}

fn preview_deck_unprocessable(report: miette::Report) -> server::DeckWriteError {
    server::DeckWriteError::Unprocessable(plain_diagnostic_text(&report))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewOriginRewriteScope {
    Slide(peitho_core::domain::SourceSpan),
    EditableBlock(peitho_core::domain::SourceSpan),
}

impl PreviewOriginRewriteScope {
    fn source_span(self) -> peitho_core::domain::SourceSpan {
        match self {
            Self::Slide(span) | Self::EditableBlock(span) => span,
        }
    }

    fn requires_whole_translation(self) -> bool {
        match self {
            Self::Slide(_) => false,
            Self::EditableBlock(_) => true,
        }
    }
}

fn preview_deck_span_conflict(
    input: &Path,
    file: &Path,
    line: usize,
    scope: PreviewOriginRewriteScope,
) -> server::DeckWriteError {
    let (message, help) = match scope {
        PreviewOriginRewriteScope::Slide(_) => (
            "this slide cannot be edited from preview",
            format!("edit the slide in {}", file.display()),
        ),
        PreviewOriginRewriteScope::EditableBlock(_) => (
            "this block cannot be edited from preview",
            format!("edit the block in {}", file.display()),
        ),
    };
    preview_deck_conflict(
        peitho_core::BuildError::new(
            peitho_core::error::ErrorKind::Parse,
            Some(line),
            message,
            help,
        )
        .with_origin_file(origin_for_display(file, input)),
    )
}

fn preview_deck_io(
    action: &str,
    path: &Path,
    help: &str,
    err: io::Error,
) -> server::DeckWriteError {
    let report = miette::miette!(
        help = help.to_owned(),
        "failed to {action} {}\ncaused by: {err}",
        path.display()
    );
    server::DeckWriteError::Io(plain_diagnostic_text(&report))
}

fn contains_bare_lf(source: &str) -> bool {
    source.as_bytes().iter().enumerate().any(|(index, byte)| {
        *byte == b'\n' && (index == 0 || source.as_bytes()[index - 1] != b'\r')
    })
}

fn convert_bare_lf_to_crlf(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut converted = String::with_capacity(source.len());
    for (index, character) in source.char_indices() {
        if character == '\n' && (index == 0 || bytes[index - 1] != b'\r') {
            converted.push('\r');
        }
        converted.push(character);
    }
    converted
}

fn last_nonblank_line_content_end(source: &str) -> usize {
    let content_end = source.trim_end_matches([' ', '\t', '\r', '\n']).len();
    if content_end == 0 {
        0
    } else {
        content_end
            + source[content_end..]
                .find(['\r', '\n'])
                .unwrap_or(source.len() - content_end)
    }
}

fn preview_scope_span_conflict(
    input: &Path,
    loaded: &LoadedDeckSource,
    combined_source: &str,
    scope: PreviewOriginRewriteScope,
) -> server::DeckWriteError {
    let requested = scope.source_span();
    let mut offset = requested.start.min(combined_source.len());
    while offset > 0 && !combined_source.is_char_boundary(offset) {
        offset -= 1;
    }
    let combined_line = peitho_core::parser::line_for_offset(combined_source, offset);
    let (translated_file, translated_line) = loaded.line_map.translate(combined_line);
    let file = if translated_file.as_os_str().is_empty() {
        input
    } else {
        translated_file.as_path()
    };
    preview_deck_span_conflict(input, file, translated_line, scope)
}

fn write_preview_origin_rewrite(
    input: &Path,
    loaded: &LoadedDeckSource,
    scope: PreviewOriginRewriteScope,
    before: &str,
    after: &str,
) -> Result<(), server::DeckWriteError> {
    if before == after {
        return Ok(());
    }

    let requested = scope.source_span();
    let rewritten_scope_end = if after.len() >= before.len() {
        requested.end.checked_add(after.len() - before.len())
    } else {
        requested.end.checked_sub(before.len() - after.len())
    }
    .ok_or_else(|| preview_scope_span_conflict(input, loaded, before, scope))?;
    let outside_scope_is_unchanged = before.get(..requested.start) == after.get(..requested.start)
        && before.get(requested.end..) == after.get(rewritten_scope_end..);
    if !outside_scope_is_unchanged {
        return Err(preview_scope_span_conflict(input, loaded, before, scope));
    }

    let translated = loaded
        .line_map
        .translate_span(before, requested)
        .ok_or_else(|| preview_scope_span_conflict(input, loaded, before, scope))?;
    let clipped_tail = requested.end - translated.combined.end;
    if scope.requires_whole_translation() && translated.combined != requested {
        return Err(preview_deck_span_conflict(
            input,
            &translated.file,
            translated.start.line,
            scope,
        ));
    }
    let clipped_head_source = &before[requested.start..translated.combined.start];

    let origin_path = translated.file.as_path();
    let mut origin_source = fs::read_to_string(origin_path).map_err(|err| {
        preview_deck_io("read", origin_path, "make the file readable and retry", err)
    })?;
    let range = peitho_core::include::origin_span_to_range(&origin_source, &translated)
        .ok_or_else(|| {
            preview_deck_span_conflict(input, origin_path, translated.start.line, scope)
        })?;
    let expected_origin = before
        .get(translated.combined.start..translated.combined.end)
        .ok_or_else(|| {
            preview_deck_span_conflict(input, origin_path, translated.start.line, scope)
        })?;
    if origin_source.get(range.clone()) != Some(expected_origin) {
        return Err(preview_deck_span_conflict(
            input,
            origin_path,
            translated.start.line,
            scope,
        ));
    }

    let rewritten_requested = after
        .get(requested.start..rewritten_scope_end)
        .ok_or_else(|| {
            preview_deck_span_conflict(input, origin_path, translated.start.line, scope)
        })?;
    let origin_uses_crlf = origin_source.contains("\r\n") && !contains_bare_lf(&origin_source);
    let rewritten_scope: Cow<'_, str> = match scope {
        PreviewOriginRewriteScope::Slide(_) => {
            let rewritten_without_head = rewritten_requested
                .strip_prefix(clipped_head_source)
                .ok_or_else(|| {
                    preview_deck_span_conflict(input, origin_path, translated.start.line, scope)
                })?;
            if clipped_tail == 0 {
                Cow::Borrowed(rewritten_without_head)
            } else {
                // A clipped slide tail is only the blank boundary before a following separator,
                // so rebuild it from the origin's own trailing run. This may change those blank
                // lines or give an unterminated include a final line ending; neither changes
                // parsing.
                let rewritten_content_end = last_nonblank_line_content_end(rewritten_without_head);
                let origin_content_end = last_nonblank_line_content_end(expected_origin);
                let origin_trailing_run = &expected_origin[origin_content_end..];
                let mut mapped =
                    String::with_capacity(rewritten_content_end + origin_trailing_run.len() + 2);
                mapped.push_str(&rewritten_without_head[..rewritten_content_end]);
                mapped.push_str(origin_trailing_run);
                if !origin_trailing_run.contains('\n') {
                    mapped.push_str(if origin_uses_crlf { "\r\n" } else { "\n" });
                }
                Cow::Owned(mapped)
            }
        }
        PreviewOriginRewriteScope::EditableBlock(_) => Cow::Borrowed(rewritten_requested),
    };
    let rewritten_scope: Cow<'_, str> = if origin_uses_crlf {
        Cow::Owned(convert_bare_lf_to_crlf(&rewritten_scope))
    } else {
        rewritten_scope
    };
    if origin_source.get(range.clone()) == Some(rewritten_scope.as_ref()) {
        return Ok(());
    }

    origin_source.replace_range(range, rewritten_scope.as_ref());
    server::write_atomic(origin_path, origin_source.as_bytes()).map_err(|err| {
        preview_deck_io(
            "write",
            origin_path,
            "make the file and its directory writable and retry",
            err,
        )
    })
}

fn write_preview_note(
    input: &Path,
    key: &SlideKey,
    text: &str,
) -> Result<(), server::DeckWriteError> {
    let loaded = load_and_expand_deck_source(input).map_err(classify_preview_deck_report)?;
    let (_, highlighter) = resolve_assets_and_highlighter(input, &loaded.frontmatter)
        .map_err(classify_preview_deck_report)?;
    let combined_source = loaded.source.as_str();
    let parsed = loaded
        .translate(peitho_core::parse_deck(
            combined_source,
            loaded.frontmatter.clone(),
            &highlighter,
        ))
        .map_err(classify_preview_deck_report)?;

    let slide = parsed
        .parsed_slides()
        .iter()
        .find(|slide| slide.key == *key)
        .ok_or_else(|| preview_deck_missing_slide_conflict(key))?;

    let rewritten = loaded
        .translate(peitho_core::notes_edit::rewrite_note(
            combined_source,
            slide.source_span,
            &slide.note_spans,
            text,
            &highlighter,
        ))
        .map_err(preview_deck_unprocessable)?;

    write_preview_origin_rewrite(
        input,
        &loaded,
        PreviewOriginRewriteScope::Slide(slide.source_span),
        combined_source,
        &rewritten,
    )
}

fn write_preview_slide_edit(
    input: &Path,
    key: &SlideKey,
    start: usize,
    end: usize,
    old: &str,
    new: &str,
) -> Result<(), server::DeckWriteError> {
    let loaded = load_and_expand_deck_source(input).map_err(classify_preview_deck_report)?;
    let (_, highlighter) = resolve_assets_and_highlighter(input, &loaded.frontmatter)
        .map_err(classify_preview_deck_report)?;
    let combined_source = loaded.source.as_str();
    let parsed = loaded
        .translate(peitho_core::parse_deck(
            combined_source,
            loaded.frontmatter.clone(),
            &highlighter,
        ))
        .map_err(classify_preview_deck_report)?;
    let slide = parsed
        .parsed_slides()
        .iter()
        .find(|slide| slide.key == *key)
        .ok_or_else(preview_deck_drift_conflict)?;
    let requested = peitho_core::domain::SourceSpan { start, end };
    let span = slide
        .editable_spans()
        .into_iter()
        .find(|span| span.source_span() == requested)
        .ok_or_else(preview_deck_drift_conflict)?;
    if combined_source.get(start..end) != Some(old) {
        return Err(preview_deck_drift_conflict());
    }

    let rewritten = loaded
        .translate(peitho_core::slide_edit::rewrite_block(
            combined_source,
            slide,
            span,
            new,
            &highlighter,
        ))
        .map_err(preview_deck_unprocessable)?;
    write_preview_origin_rewrite(
        input,
        &loaded,
        PreviewOriginRewriteScope::EditableBlock(span.source_span()),
        combined_source,
        &rewritten,
    )
}

fn write_preview_slide_source(
    input: &Path,
    key: &SlideKey,
    old: &str,
    new: &str,
) -> Result<(SlideKey, String), server::DeckWriteError> {
    let loaded = load_and_expand_deck_source(input).map_err(classify_preview_deck_report)?;
    let (_, highlighter) = resolve_assets_and_highlighter(input, &loaded.frontmatter)
        .map_err(classify_preview_deck_report)?;
    let combined_source = loaded.source.as_str();
    let parsed = loaded
        .translate(peitho_core::parse_deck(
            combined_source,
            loaded.frontmatter.clone(),
            &highlighter,
        ))
        .map_err(classify_preview_deck_report)?;
    let slide = parsed
        .parsed_slides()
        .iter()
        .find(|slide| slide.key == *key)
        .ok_or_else(|| preview_deck_missing_slide_conflict(key))?;
    let current_body = loaded
        .translate(peitho_core::slide_source::slide_body(
            combined_source,
            slide,
            &highlighter,
        ))
        .map_err(preview_deck_unprocessable)?;
    if current_body != old {
        return Err(preview_deck_drift_conflict());
    }

    let peitho_core::slide_source::SlideBodyRewrite { source, key, body } = loaded
        .translate(peitho_core::slide_source::rewrite_slide_body(
            combined_source,
            slide,
            new,
            &highlighter,
        ))
        .map_err(preview_deck_unprocessable)?;
    write_preview_origin_rewrite(
        input,
        &loaded,
        PreviewOriginRewriteScope::Slide(slide.source_span),
        combined_source,
        &source,
    )?;
    Ok((key, body))
}

struct PreviewDeckWriter {
    input: PathBuf,
}

impl server::DeckWriter for PreviewDeckWriter {
    fn note(&mut self, key: SlideKey, text: String) -> Result<(), server::DeckWriteError> {
        write_preview_note(&self.input, &key, &text)
    }

    fn slide_edit(&mut self, edit: server::SlideEditWrite) -> Result<(), server::DeckWriteError> {
        let server::SlideEditWrite {
            key,
            start,
            end,
            old,
            new,
        } = edit;
        write_preview_slide_edit(&self.input, &key, start, end, &old, &new)
    }

    fn slide_source(
        &mut self,
        key: SlideKey,
        old: String,
        new: String,
    ) -> Result<server::SlideSourceSaved, server::DeckWriteError> {
        write_preview_slide_source(&self.input, &key, &old, &new)
            .map(|(key, body)| server::SlideSourceSaved { key, body })
    }
}

fn read_deck_source(input: &Path) -> miette::Result<String> {
    fs::read_to_string(input).map_err(|err| {
        miette::miette!(
            help = "the deck argument defaults to deck.md in the current directory when omitted; pass the deck path explicitly if it lives elsewhere",
            "failed to read {}\ncaused by: {err}",
            input.display()
        )
    })
}

fn code_images_cache_dir(input: &Path) -> PathBuf {
    asset_resolution::deck_parent(input).join(peitho_core::CODE_IMAGES_CACHE_DIR)
}

fn embeds_cache_dir(input: &Path) -> PathBuf {
    asset_resolution::deck_parent(input).join(peitho_core::EMBEDS_CACHE_DIR)
}

fn emit_distribution(out: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    write_shared_assets(out, artifacts)?;
    write_slide_fragments(out, &artifacts.rendered)?;
    fs::write(out.join("manifest.json"), &artifacts.manifest_json).into_diagnostic()?;
    fs::write(
        out.join("index.html"),
        peitho_core::render_distribution_index(
            artifacts.rendered.settings().aspect_ratio(),
            artifacts.rendered.settings().lang(),
        ),
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
    fs::write(dir.join("peitho.css"), artifacts.rendered.css()).into_diagnostic()?;
    write_image_assets(dir, &artifacts.image_assets)?;
    write_fonts_assets(dir, artifacts.fonts_source.as_deref())?;
    write_theme_fonts_assets(dir)?;
    write_katex_fonts_assets(dir, artifacts.rendered.math_assets())
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
            help = "point fonts: at a regular file or a directory",
            "unsupported fonts: source {} (symlink)",
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
            help = "point fonts: at a regular file or a directory",
            "unsupported fonts: source {} (special file)",
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
                help = "only regular files and subdirectories are supported inside fonts/",
                "unsupported entry in fonts directory: {} ({entry_type})",
                source_path.display()
            ));
        }
    }
    Ok(())
}

fn write_katex_fonts_assets(
    out: &Path,
    math_assets: Option<peitho_core::MathAssets>,
) -> miette::Result<()> {
    let fonts_dir = out.join("katex-fonts");
    if fonts_dir.exists() {
        fs::remove_dir_all(&fonts_dir).into_diagnostic()?;
    }
    let Some(math_assets) = math_assets else {
        return Ok(());
    };

    fs::create_dir_all(&fonts_dir).into_diagnostic()?;
    for font in math_assets.fonts() {
        fs::write(fonts_dir.join(font.file_name()), font.bytes()).into_diagnostic()?;
    }
    Ok(())
}

fn write_theme_fonts_assets(out: &Path) -> miette::Result<()> {
    let fonts_dir = out.join("theme-fonts");
    if fonts_dir.exists() {
        fs::remove_dir_all(&fonts_dir).into_diagnostic()?;
    }

    fs::create_dir_all(&fonts_dir).into_diagnostic()?;
    for font in peitho_core::theme_fonts() {
        fs::write(fonts_dir.join(font.file_name()), font.bytes()).into_diagnostic()?;
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
            help = "install Google Chrome or Chromium, or set PEITHO_CHROME_PATH=<absolute-path>",
            "Chrome not found at PEITHO_CHROME_PATH={}",
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
        help = "install Google Chrome or Chromium, or set PEITHO_CHROME_PATH=<absolute-path>",
        "Chrome not found"
    ))
}

fn find_chrome_in_path(program: &str, path_dirs: &[PathBuf]) -> Option<PathBuf> {
    path_dirs.iter().find_map(|dir| {
        let candidate = dir.join(program);
        candidate.is_file().then_some(candidate)
    })
}

#[derive(Clone, Copy)]
enum EmbedWrapperMode {
    Measure,
    Capture,
}

fn embed_wrapper_html(
    normalized_url: &str,
    params: peitho_core::code_images::EmbedRenderParams,
    mode: EmbedWrapperMode,
) -> String {
    let width = params.width_css_px;
    let theme = params.theme.as_str();
    let (script_error_release, timeout_release, settled_action) = match mode {
        EmbedWrapperMode::Measure => (
            "  js.onerror = releaseLoad;\n",
            "setTimeout(releaseLoad, 15000);\n",
            concat!(
                "        document.title = \"peitho-embed-height:\" + stableBottom;\n",
                "        releaseLoad();\n"
            ),
        ),
        EmbedWrapperMode::Capture => {
            // Capture has no failure release: a named timeout is safer than
            // completing load and silently caching a non-rendered screenshot.
            ("", "", "        releaseLoad();\n")
        }
    };
    // `{normalized_url}` is safe only under `parse_x_status_url`'s strict grammar; revisit if loosened.
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>peitho-embed-pending</title>
<style>html,body{{margin:0;padding:0;width:{width}px;background:#fff;overflow:hidden}}blockquote{{margin:0}}div.twitter-tweet-rendered{{margin:0 !important}}</style>
</head>
<body>
<blockquote class="twitter-tweet" data-width="{width}" data-theme="{theme}"><a href="{normalized_url}">View post on X</a></blockquote>
<iframe id="peitho-load-holder" style="visibility:hidden;width:0;height:0;border:0"></iframe>
<script>
var holder = document.getElementById("peitho-load-holder");
holder.contentDocument.open();
holder.contentDocument.write("holding load");
var loadReleased = false;
function releaseLoad() {{
  if (loadReleased) return;
  loadReleased = true;
  try {{
    holder.contentDocument.close();
  }} catch (error) {{
    console.error("failed to release peitho load holder", error);
  }}
}}
window.twttr = (function(d, s, id) {{
  var js, first = d.getElementsByTagName(s)[0], twttr = window.twttr || {{}};
  if (d.getElementById(id)) return twttr;
  js = d.createElement(s);
  js.id = id;
  js.src = "https://platform.x.com/widgets.js";
{script_error_release}  first.parentNode.insertBefore(js, first);
  twttr._e = [];
  twttr.ready = function(callback) {{ twttr._e.push(callback); }};
  return twttr;
}}(document, "script", "twitter-wjs"));
{timeout_release}twttr.ready(function(twttr) {{
  twttr.events.bind("rendered", function(event) {{
    var iframe = event && event.target && event.target.tagName === "IFRAME"
      ? event.target
      : document.querySelector("iframe:not(#peitho-load-holder)");
    if (!iframe) return;
    var stableBottom = 0;
    var stableCount = 0;
    function pollSettledBottom() {{
      if (loadReleased) return;
      var bottom = Math.ceil(iframe.getBoundingClientRect().bottom);
      if (!Number.isFinite(bottom) || bottom <= 0) {{
        stableBottom = 0;
        stableCount = 0;
      }} else if (bottom === stableBottom) {{
        stableCount += 1;
      }} else {{
        stableBottom = bottom;
        stableCount = 1;
      }}
      if (stableCount >= 3) {{
{settled_action}        return;
      }}
      setTimeout(pollSettledBottom, 100);
    }}
    setTimeout(pollSettledBottom, 100);
  }});
}});
</script>
</body>
</html>
"#
    )
}

fn embed_measure_args(
    profile: &Path,
    wrapper_url: &str,
    params: peitho_core::code_images::EmbedRenderParams,
) -> Vec<OsString> {
    vec![
        OsString::from("--headless=new"),
        OsString::from("--disable-gpu"),
        OsString::from("--no-sandbox"),
        OsString::from(format!("--user-data-dir={}", profile.display())),
        OsString::from(format!("--window-size={},10000", params.width_css_px)),
        OsString::from(format!(
            "--force-device-scale-factor={}",
            params.scale_factor
        )),
        OsString::from("--dump-dom"),
        OsString::from(wrapper_url),
    ]
}

fn embed_capture_args(
    profile: &Path,
    output_path: &Path,
    wrapper_url: &str,
    height: u32,
    params: peitho_core::code_images::EmbedRenderParams,
) -> Vec<OsString> {
    vec![
        OsString::from("--headless=new"),
        OsString::from("--disable-gpu"),
        OsString::from("--no-sandbox"),
        OsString::from(format!("--user-data-dir={}", profile.display())),
        OsString::from(format!("--window-size={},{}", params.width_css_px, height)),
        OsString::from(format!(
            "--force-device-scale-factor={}",
            params.scale_factor
        )),
        OsString::from(format!("--screenshot={}", output_path.display())),
        OsString::from(wrapper_url),
    ]
}

fn parse_embed_height(dom: &[u8]) -> miette::Result<u32> {
    let dom = String::from_utf8_lossy(dom);
    let title_start = dom.find("<title>").ok_or_else(|| {
        miette::miette!(
            help = "check network access to X and that the post is public",
            "official X embed did not publish a rendered height"
        )
    })? + "<title>".len();
    let title_end = dom[title_start..]
        .find("</title>")
        .map(|offset| title_start + offset)
        .ok_or_else(|| {
            miette::miette!(
                help = "retry with Chrome and network access to X",
                "official X embed height title was incomplete"
            )
        })?;
    let title = &dom[title_start..title_end];
    let height = title
        .strip_prefix("peitho-embed-height:")
        .and_then(|height| height.parse::<u32>().ok())
        .filter(|height| *height > 0)
        .ok_or_else(|| {
            miette::miette!(
                help = "check network access to X and that the post is public",
                "official X embed did not publish a valid rendered height (title: {title})"
            )
        })?;
    if height >= 10_000 {
        return Err(miette::miette!(
            "rendered embed height {height} reaches the 10000px measurement viewport; the post is too tall to embed"
        ));
    }
    Ok(height)
}

fn embed_dump_has_complete_title(stdout: &[u8]) -> bool {
    const TITLE_PREFIX: &[u8] = b"<title>peitho-embed-";
    const TITLE_END: &[u8] = b"</title>";

    let Some(start) = stdout
        .windows(TITLE_PREFIX.len())
        .position(|window| window == TITLE_PREFIX)
    else {
        return false;
    };
    stdout[start + TITLE_PREFIX.len()..]
        .windows(TITLE_END.len())
        .any(|window| window == TITLE_END)
}

fn render_embed_with_invoker<F>(
    temp_dir: &Path,
    normalized_url: &str,
    params: peitho_core::code_images::EmbedRenderParams,
    mut invoke: F,
) -> miette::Result<Vec<u8>>
where
    F: FnMut(&[OsString], ChromeCompletion) -> miette::Result<ChromeOutput>,
{
    let measure_wrapper_path = temp_dir.join("embed-measure.html");
    fs::write(
        &measure_wrapper_path,
        embed_wrapper_html(normalized_url, params, EmbedWrapperMode::Measure),
    )
    .into_diagnostic()?;
    let measure_wrapper_url = file_url(&measure_wrapper_path)?;

    let measure_profile = temp_dir.join("measure-profile");
    fs::create_dir_all(&measure_profile).into_diagnostic()?;

    let measure_args = embed_measure_args(&measure_profile, &measure_wrapper_url, params);
    let measured = invoke(&measure_args, ChromeCompletion::EmbedMeasured)?;
    let height = parse_embed_height(&measured.stdout)?;

    let capture_wrapper_path = temp_dir.join("embed-capture.html");
    fs::write(
        &capture_wrapper_path,
        embed_wrapper_html(normalized_url, params, EmbedWrapperMode::Capture),
    )
    .into_diagnostic()?;
    let capture_wrapper_url = file_url(&capture_wrapper_path)?;
    let capture_profile = temp_dir.join("capture-profile");
    fs::create_dir_all(&capture_profile).into_diagnostic()?;

    let output_path = temp_dir.join("embed.png");
    let capture_args = embed_capture_args(
        &capture_profile,
        &output_path,
        &capture_wrapper_url,
        height,
        params,
    );
    invoke(&capture_args, ChromeCompletion::PngWritten)?;
    fs::read(&output_path).into_diagnostic()
}

fn render_embed_with_chrome(
    chrome: &Path,
    temp_dir: &Path,
    normalized_url: &str,
    params: peitho_core::code_images::EmbedRenderParams,
) -> miette::Result<Vec<u8>> {
    render_embed_with_invoker(temp_dir, normalized_url, params, |args, completion| {
        run_one_shot_chrome(chrome, args, completion, CHROME_ONE_SHOT_TIMEOUT)
    })
}

const CHROME_ONE_SHOT_TIMEOUT: Duration = Duration::from_secs(60);

/// Give Chrome a brief chance to shut down cleanly after the complete PDF has
/// been written. Chrome 149 on macOS can linger after waking GoogleUpdater, so
/// post-print cleanup gets its own bounded courtesy window and cannot turn a
/// valid export into a failure.
const POST_PRINT_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// How long to keep draining a child's pipes after it exits, waiting for a
/// completion signal that was written but may not have been delivered yet.
///
/// Chrome can exit while a completion console message is still in flight, and
/// lingering grandchildren can hold the pipes open so EOF never arrives — the
/// drain cannot simply wait for disconnection. Five seconds is far above the
/// scheduling delays seen on loaded CI runners while staying well under
/// `CHROME_ONE_SHOT_TIMEOUT`.
const POST_EXIT_DRAIN_WINDOW: Duration = Duration::from_secs(5);

enum ChromeCompletion {
    LintResultLogged,
    EmbedMeasured,
    PngWritten,
}

#[derive(Default)]
struct ChromeCompletionState {
    stderr_scanned: usize,
    signal_seen: bool,
}

impl ChromeCompletion {
    fn description(&self) -> &'static str {
        match self {
            Self::LintResultLogged => "lint measurement payload",
            Self::EmbedMeasured => "official X embed rendered height",
            Self::PngWritten => "tweet embed PNG output",
        }
    }

    fn retry_help(&self) -> &'static str {
        match self {
            Self::LintResultLogged => {
                "retry lint; if a lint workspace was kept, inspect lint.html there"
            }
            Self::EmbedMeasured | Self::PngWritten => {
                "retry the build with Chrome and network access to X"
            }
        }
    }

    fn timeout_help(&self) -> &'static str {
        match self {
            Self::LintResultLogged => {
                "retry lint; if a lint workspace was kept, inspect lint.html there"
            }
            Self::EmbedMeasured | Self::PngWritten => {
                "retry the build with Chrome and network access to X"
            }
        }
    }

    fn process_setup_help(&self) -> String {
        format!(
            "{}; this is an internal process setup error",
            self.retry_help()
        )
    }

    fn is_ready(&self, stdout: &[u8], stderr: &[u8], state: &mut ChromeCompletionState) -> bool {
        match self {
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
            Self::EmbedMeasured => embed_dump_has_complete_title(stdout),
            Self::PngWritten => {
                if !state.signal_seen {
                    state.signal_seen = scan_for_needle(
                        stderr,
                        &mut state.stderr_scanned,
                        b"bytes written to file",
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
            Self::LintResultLogged => self.is_ready(stdout, stderr, state),
            Self::EmbedMeasured => embed_dump_has_complete_title(stdout),
            Self::PngWritten => self.is_ready(stdout, stderr, state),
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
                    help = "check that Chrome can run in headless mode",
                    "Chrome failed during one-shot operation with status {}\nstderr: {}",
                    status,
                    String::from_utf8_lossy(&stderr).trim()
                ));
            }
            Err(miette::miette!(
                help = format!("expected {} before Chrome exited", completion.description()),
                "Chrome completed before one-shot output was ready\nstderr: {}",
                String::from_utf8_lossy(&stderr).trim()
            ))
        }
        ProcessOutcome::TimedOut { stderr } => Err(miette::miette!(
            help = completion.timeout_help(),
            "Chrome timed out after {}s waiting for {}\nstderr: {}",
            timeout.as_secs(),
            completion.description(),
            String::from_utf8_lossy(&stderr).trim(),
        )),
    }
}

#[derive(Debug)]
pub(crate) struct ChromeOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

fn chrome_output(stdout: Vec<u8>, stderr: Vec<u8>) -> ChromeOutput {
    ChromeOutput { stdout, stderr }
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
            // The child is gone, but its output may still be in flight: the
            // reader threads are never joined, and lingering grandchildren can
            // hold the pipes open so EOF never arrives. Keep draining until the
            // completion signal shows up, the pipes disconnect, or this window
            // expires — the old fixed 100ms silently truncated the tail of the
            // stream under load, dropping completion signals that had been
            // written but not yet delivered.
            //
            // The window is bounded well under the overall timeout so a caller
            // whose completion predicate stays false after exit is not made to
            // wait indefinitely.
            let drain_deadline = deadline.min(Instant::now() + POST_EXIT_DRAIN_WINDOW);
            if drain_until_complete_or_disconnect(
                &rx,
                &mut stdout,
                &mut stderr,
                drain_deadline,
                &mut is_complete,
            ) {
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

/// Drain remaining pipe output after the child has exited, stopping as soon as
/// the completion predicate is satisfied. Returns whether it was satisfied.
///
/// Output can still be in flight after exit, and the reader threads may never
/// observe EOF when a grandchild holds a pipe open, so this waits on a real
/// terminating condition (signal seen, pipes disconnected, or deadline) rather
/// than a fixed window.
fn drain_until_complete_or_disconnect<F>(
    rx: &mpsc::Receiver<ProcessPipeEvent>,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    deadline: Instant,
    is_complete: &mut F,
) -> bool
where
    F: FnMut(&[u8], &[u8]) -> bool,
{
    loop {
        drain_process_events(rx, stdout, stderr);
        if is_complete(stdout, stderr) {
            return true;
        }

        let now = Instant::now();
        if now >= deadline {
            return false;
        }

        let remaining = deadline.saturating_duration_since(now);
        let poll = remaining.min(Duration::from_millis(10));
        match receive_process_event_until(rx, stdout, stderr, poll) {
            ProcessReceive::Event | ProcessReceive::Timeout => {}
            // Both pipes hit EOF: everything the child wrote has been
            // delivered, so one last check settles it.
            ProcessReceive::Disconnected => {
                drain_process_events(rx, stdout, stderr);
                return is_complete(stdout, stderr);
            }
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
            help = completion.process_setup_help(),
            "failed to capture Chrome stdout"
        ),
        ProcessRunError::CaptureStderr => miette::miette!(
            help = completion.process_setup_help(),
            "failed to capture Chrome stderr"
        ),
        ProcessRunError::CaptureStdin => miette::miette!(
            help = completion.process_setup_help(),
            "failed to capture Chrome stdin"
        ),
        ProcessRunError::Spawn(err) => miette::miette!(
            help = "install Google Chrome or set PEITHO_CHROME_PATH=<absolute-path>",
            "failed to run Chrome at {}\ncaused by: {err}",
            chrome.display()
        ),
        ProcessRunError::Wait(err) => miette::miette!(
            help = completion.retry_help(),
            "failed to wait on Chrome: {err}"
        ),
        ProcessRunError::Kill(err) => miette::miette!(
            help = "report the underlying io error",
            "failed to terminate Chrome: {err}"
        ),
    }
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
    let sanitized = String::from_utf8_lossy(stderr)
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    let excerpt = normalized.chars().take(200).collect::<String>();
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
        OsString::from("--enable-logging=stderr"),
        OsString::from(format!("--user-data-dir={}", profile.display())),
        OsString::from(format!("--print-to-pdf={}", out.display())),
        OsString::from(url),
    ]
}

fn chrome_export_args(profile: &Path) -> Vec<OsString> {
    vec![
        OsString::from("--headless=new"),
        OsString::from("--disable-gpu"),
        OsString::from("--no-sandbox"),
        OsString::from("--remote-debugging-port=0"),
        OsString::from("--enable-logging=stderr"),
        OsString::from(format!("--user-data-dir={}", profile.display())),
        OsString::from("about:blank"),
    ]
}

fn run_chrome_print(chrome: &Path, workspace: &Path, out: &Path) -> miette::Result<()> {
    run_chrome_print_with_timeout(chrome, workspace, out, CHROME_ONE_SHOT_TIMEOUT)
}

fn run_chrome_print_with_timeout(
    chrome: &Path,
    workspace: &Path,
    out: &Path,
    timeout: Duration,
) -> miette::Result<()> {
    let abs_out = absolute_path_for_output(out)?;
    let profile = workspace.join("chrome-profile");
    fs::create_dir_all(&profile).into_diagnostic()?;
    let pdf_html = workspace.join("pdf.html");
    let url = file_url(&pdf_html)?;
    let args = chrome_export_args(&profile);
    let deadline = Instant::now() + timeout;
    let mut process = CdpChromeProcess::spawn(chrome, &args)?;

    let result = (|| {
        let port = cdp::wait_for_devtools_port(&profile, deadline, || {
            process.ensure_running_before_devtools_port()
        })?;
        let discovered_url = cdp::fetch_page_websocket_url(port, deadline)?;
        let mut client = cdp::CdpClient::connect(port, &discovered_url, deadline)?;
        client.page_enable(deadline)?;
        client.page_navigate(&url, deadline)?;
        cdp::wait_for_pdf_flattening(&mut client, deadline)?;

        let pdf = client.page_print_to_pdf(deadline)?;
        if pdf.is_empty() {
            return Err(miette::miette!(
                "Chrome Page.printToPDF returned an empty PDF"
            ));
        }
        fs::write(&abs_out, pdf).map_err(|err| {
            miette::miette!(
                "failed to write Chrome PDF output at {}: {err}",
                abs_out.display()
            )
        })?;

        best_effort_cdp_shutdown(
            &mut process,
            POST_PRINT_SHUTDOWN_GRACE,
            |shutdown_deadline| client.browser_close(shutdown_deadline),
        );
        Ok(())
    })();

    match result {
        Ok(()) => Ok(()),
        Err(err) => Err(cdp_export_error(&mut process, err)),
    }
}

fn best_effort_cdp_shutdown(
    process: &mut CdpChromeProcess,
    grace: Duration,
    close_browser: impl FnOnce(Instant) -> miette::Result<()>,
) {
    let deadline = Instant::now() + grace;
    if close_browser(deadline).is_err() || process.wait_for_exit(deadline).is_err() {
        let _ = process.kill_and_reap();
    }
}

struct CdpChromeProcess {
    child: Child,
    stderr_rx: mpsc::Receiver<ProcessPipeEvent>,
    stderr_reader: Option<thread::JoinHandle<()>>,
    stderr: Vec<u8>,
    reaped: bool,
}

impl CdpChromeProcess {
    fn spawn(chrome: &Path, args: &[OsString]) -> miette::Result<Self> {
        let mut child = std::process::Command::new(chrome)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                miette::miette!(
                    help = "install Google Chrome or set PEITHO_CHROME_PATH=<absolute-path>",
                    "failed to run Chrome at {} for ordered PDF printing\ncaused by: {err}",
                    chrome.display()
                )
            })?;
        let Some(stderr_pipe) = child.stderr.take() else {
            let _ = kill_and_reap_process_child(&mut child);
            return Err(miette::miette!(
                "failed to capture Chrome stderr for ordered PDF printing"
            ));
        };
        let (stderr_tx, stderr_rx) = mpsc::channel();
        let stderr_reader = spawn_process_pipe_reader(stderr_pipe, ProcessPipe::Stderr, stderr_tx);
        Ok(Self {
            child,
            stderr_rx,
            stderr_reader: Some(stderr_reader),
            stderr: Vec::new(),
            reaped: false,
        })
    }

    fn ensure_running_before_devtools_port(&mut self) -> miette::Result<()> {
        self.drain_stderr();
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(status)) => {
                self.reaped = true;
                self.drain_stderr();
                Err(miette::miette!(
                    "Chrome exited with status {status} before publishing DevToolsActivePort"
                ))
            }
            Err(err) => Err(miette::miette!(
                "failed to inspect Chrome while waiting for DevToolsActivePort: {err}"
            )),
        }
    }

    fn wait_for_exit(&mut self, deadline: Instant) -> miette::Result<ExitStatus> {
        loop {
            self.drain_stderr();
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.reaped = true;
                    self.drain_stderr();
                    return Ok(status);
                }
                Ok(None) => {}
                Err(err) => {
                    return Err(miette::miette!(
                        "failed to wait for Chrome after Browser.close: {err}"
                    ));
                }
            }

            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| {
                    miette::miette!("timed out waiting for Chrome to exit after Browser.close")
                })?;
            thread::sleep(Duration::from_millis(25).min(remaining));
        }
    }

    fn kill_and_reap(&mut self) -> io::Result<()> {
        if self.reaped {
            self.drain_stderr_tail();
            return Ok(());
        }
        kill_and_reap_process_child(&mut self.child)?;
        self.reaped = true;
        self.drain_stderr_tail();
        Ok(())
    }

    fn drain_stderr(&mut self) {
        let mut ignored_stdout = Vec::new();
        drain_process_events(&self.stderr_rx, &mut ignored_stdout, &mut self.stderr);
    }

    fn drain_stderr_tail(&mut self) {
        // The pipe reader runs on another thread. After kill+wait, give it a
        // bounded window to deliver bytes that were already written before
        // the child died; Chrome grandchildren may keep the pipe open, so
        // waiting for EOF is not safe.
        let tail_deadline = Instant::now() + Duration::from_millis(500);
        let mut ignored_stdout = Vec::new();
        loop {
            self.drain_stderr();
            let now = Instant::now();
            if now >= tail_deadline {
                break;
            }
            let poll = Duration::from_millis(10).min(tail_deadline - now);
            if matches!(
                receive_process_event_until(
                    &self.stderr_rx,
                    &mut ignored_stdout,
                    &mut self.stderr,
                    poll,
                ),
                ProcessReceive::Disconnected
            ) {
                self.drain_stderr();
                break;
            }
        }
    }

    fn join_stderr_reader(&mut self) {
        if let Some(stderr_reader) = self.stderr_reader.take() {
            let _ = stderr_reader.join();
        }
        self.drain_stderr();
    }
}

impl Drop for CdpChromeProcess {
    fn drop(&mut self) {
        let _ = self.kill_and_reap();
    }
}

fn cdp_export_error(process: &mut CdpChromeProcess, err: miette::Error) -> miette::Error {
    let cleanup_error = process.kill_and_reap().err();
    if cleanup_error.is_none() {
        process.join_stderr_reader();
    }
    let stderr = String::from_utf8_lossy(&process.stderr);
    let stderr = if stderr.trim().is_empty() {
        "(empty)"
    } else {
        stderr.trim()
    };
    let cleanup = cleanup_error
        .map(|cleanup_err| {
            format!("\ncleanup error: failed to kill and reap Chrome: {cleanup_err}")
        })
        .unwrap_or_default();
    miette::miette!(
        help = "if an export workspace was kept, inspect its pdf.html and the Chrome stderr above",
        "Chrome PDF export failed\ncaused by: {err}\nstderr: {stderr}{cleanup}"
    )
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
            help = "deployment is delegated to IaC or CI; example: peitho publish -- aws s3 sync dist/ s3://bucket",
            "publish command is missing"
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
                help = "check that the command exists and is executable",
                "failed to run publish command: {}\ncaused by: {err}",
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
    reject_preview_edit_annotations(dist)?;

    read_publish_manifest(dist)?;
    let canonical = fs::canonicalize(dist).map_err(|err| {
        miette::miette!(
            help = "run `peitho build` first",
            "distribution is incomplete: failed to resolve {}\ncaused by: {err}",
            dist.display()
        )
    })?;

    Ok(PublishDistribution { dist: canonical })
}

fn reject_presentation_only_files(dist: &Path) -> miette::Result<()> {
    for file in PRESENTATION_ONLY_DIST_FILES {
        if dist.join(file).exists() {
            return Err(publish_contamination_error(format!(
                "distribution contains presentation-only file: {file}"
            )));
        }
    }
    Ok(())
}

fn publish_contamination_error(message: String) -> miette::Report {
    miette::miette!(help = PUBLISH_CONTAMINATION_HELP, "{message}")
}

fn reject_preview_edit_annotations(dist: &Path) -> miette::Result<()> {
    let mut visited_dirs = HashSet::new();
    reject_preview_edit_annotations_in_dir(dist, dist, &mut visited_dirs)
}

fn reject_preview_edit_annotations_in_dir(
    dist: &Path,
    dir: &Path,
    visited_dirs: &mut HashSet<PathBuf>,
) -> miette::Result<()> {
    let canonical_dir =
        fs::canonicalize(dir).map_err(|err| publish_entry_inspection_error(dist, dir, err))?;
    if !visited_dirs.insert(canonical_dir) {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)
        .map_err(|err| publish_entry_inspection_error(dist, dir, err))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|err| publish_entry_inspection_error(dist, dir, err))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata =
            fs::metadata(&path).map_err(|err| publish_entry_inspection_error(dist, &path, err))?;
        if metadata.is_dir() {
            reject_preview_edit_annotations_in_dir(dist, &path, visited_dirs)?;
            continue;
        }
        let is_html = path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"));
        if !metadata.is_file() || !is_html {
            continue;
        }

        let relative = path.strip_prefix(dist).map_err(|err| {
            miette::miette!(
                "failed to identify distribution file {}\ncaused by: {err}",
                path.display()
            )
        })?;
        let html = fs::read_to_string(&path).map_err(|err| {
            miette::miette!(
                help = "ensure every HTML file under dist/ is UTF-8 or run `peitho build` again",
                "failed to read distribution HTML as UTF-8: {}\ncaused by: {err}",
                relative.display()
            )
        })?;
        let attribute = peitho_core::find_edit_annotation_attribute(&html).map_err(|err| {
            let message = err.message;
            miette::miette!(
                help = "run `peitho build` again",
                "distribution file could not be parsed as HTML: {}\ncaused by: {}",
                relative.display(),
                message
            )
        })?;
        if let Some(attribute) = attribute {
            return Err(publish_contamination_error(format!(
                "distribution contains preview-only attribute {attribute}: {}",
                relative.display()
            )));
        }
    }

    Ok(())
}

fn publish_entry_inspection_error(dist: &Path, path: &Path, err: std::io::Error) -> miette::Report {
    let relative = path.strip_prefix(dist).unwrap_or(path);
    let relative = if relative.as_os_str().is_empty() {
        Path::new(".")
    } else {
        relative
    };
    miette::miette!(
        help = "remove the unreadable entry or run `peitho build` again",
        "failed to inspect distribution entry {}\ncaused by: {err}",
        relative.display()
    )
}

fn latest_rehearsal_record(
    dir: &Path,
) -> miette::Result<Option<(PathBuf, peitho_core::RehearsalRecord)>> {
    let Some(latest_path) = rehearsal_record_paths_by_name(dir)?.pop() else {
        return Ok(None);
    };

    Ok(Some((
        latest_path.clone(),
        read_rehearsal_record(&latest_path)?,
    )))
}

fn rehearsal_record_paths(dir: &Path) -> miette::Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(dir)
        .map_err(|err| rehearsal_record_paths_entry_error(dir, err))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|err| rehearsal_record_paths_entry_error(dir, err))?
        .into_iter()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| path.file_name().map(|name| name.to_os_string()));
    Ok(paths)
}

fn rehearsal_record_paths_entry_error(dir: &Path, err: std::io::Error) -> miette::Report {
    miette::miette!(
        help = "check permissions or move the rehearsals directory",
        "failed to read rehearsal records in {}\ncaused by: {err}",
        dir.display()
    )
}

fn rehearsal_record_paths_by_name(dir: &Path) -> miette::Result<Vec<PathBuf>> {
    let mut records = rehearsal_record_paths(dir)?
        .into_iter()
        .filter_map(|path| {
            let key = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(server::parse_rehearsal_filename)?;
            Some((key, path))
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|(key, _)| key.clone());
    Ok(records.into_iter().map(|(_, path)| path).collect())
}

fn read_rehearsal_record(path: &Path) -> miette::Result<peitho_core::RehearsalRecord> {
    let json = fs::read_to_string(path).map_err(|err| {
        let help = rehearsal_record_recovery_help(path);
        miette::miette!(
            help = help,
            "failed to read rehearsal record {}\ncaused by: {err}",
            path.display()
        )
    })?;
    let record: peitho_core::RehearsalRecord = serde_json::from_str(&json).map_err(|err| {
        let help = rehearsal_record_recovery_help(path);
        miette::miette!(
            help = help,
            "failed to parse rehearsal record {}\ncaused by: {err}",
            path.display()
        )
    })?;
    if let peitho_core::RehearsalRecord::V2(record) = &record {
        if let Some(audio) = record.audio() {
            let expected = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(server::parse_rehearsal_filename)
                .map(|identity| identity.webm_name());
            if expected.as_deref() != Some(audio.file()) {
                let help = rehearsal_record_recovery_help(path);
                return Err(miette::miette!(
                    help = help,
                    "rehearsal record {} has audio filename {:?}, which does not match its session filename",
                    path.display(),
                    audio.file()
                ));
            }
        }
    }
    Ok(record)
}

fn rehearsal_audio_path(
    record_path: &Path,
    record: &peitho_core::RehearsalRecord,
) -> Option<PathBuf> {
    let peitho_core::RehearsalRecord::V2(record) = record else {
        return None;
    };
    let audio = record.audio()?;
    Some(
        record_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(audio.file()),
    )
}

fn rehearsal_record_recovery_help(path: &Path) -> String {
    format!(
        "delete or move {} and run `peitho rehearsal` again",
        path.display()
    )
}

fn run_rehearsal(
    options: RehearsalOptions,
    stdout: &mut dyn Write,
    style: LabelStyle,
) -> miette::Result<()> {
    let records = if options.all {
        rehearsal_record_paths_by_name(&options.rehearsals_dir)?
            .into_iter()
            .map(|path| {
                let record = read_rehearsal_record(&path)?;
                Ok((path, record))
            })
            .collect::<miette::Result<Vec<_>>>()?
    } else {
        latest_rehearsal_record(&options.rehearsals_dir)?
            .into_iter()
            .collect()
    };

    if records.is_empty() {
        writeln!(
            stdout,
            "no rehearsal records in {}",
            options.rehearsals_dir.display()
        )
        .into_diagnostic()?;
        writeln!(
            stdout,
            "{}run peitho present --rehearsal deck.md to record one",
            style.help()
        )
        .into_diagnostic()?;
        return Ok(());
    }

    for (index, (path, record)) in records.iter().enumerate() {
        if index > 0 {
            writeln!(stdout).into_diagnostic()?;
        }
        write_rehearsal_record_summary(stdout, path, record, style)?;
    }
    Ok(())
}

fn write_rehearsal_record_summary(
    stdout: &mut dyn Write,
    path: &Path,
    record: &peitho_core::RehearsalRecord,
    style: LabelStyle,
) -> miette::Result<()> {
    let audio_path = rehearsal_audio_path(path, record);
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("rehearsal");
    let recorded = local_recorded_minute(path, record.recorded_at_ms())?;
    writeln!(stdout, "{stem}  (recorded {recorded})").into_diagnostic()?;
    writeln!(stdout).into_diagnostic()?;

    let name_width = record
        .sections()
        .iter()
        .map(|section| section.name().chars().count())
        .chain(std::iter::once("section".len()))
        .max()
        .unwrap_or("section".len());
    write_rehearsal_row(stdout, name_width, "section", "planned", "actual", "delta")?;

    let mut total_planned = 0_u64;
    let mut total_actual = 0_u64;
    for section in record.sections() {
        total_planned = total_planned.saturating_add(section.planned_duration_ms());
        total_actual = total_actual.saturating_add(section.actual_ms());
        write_rehearsal_row(
            stdout,
            name_width,
            section.name(),
            &format_minute_seconds(section.planned_duration_ms()),
            &format_minute_seconds(section.actual_ms()),
            &format_rehearsal_delta(section.actual_ms(), section.planned_duration_ms()),
        )?;
    }
    write_rehearsal_row(
        stdout,
        name_width,
        "total",
        &format_minute_seconds(total_planned),
        &format_minute_seconds(total_actual),
        &format_rehearsal_delta(total_actual, total_planned),
    )?;

    if let peitho_core::RehearsalRecord::V2(record) = record {
        write_rehearsal_slide_summary(stdout, record)?;
        if let Some(audio_path) = audio_path {
            writeln!(stdout, "  audio   {}", audio_path.display()).into_diagnostic()?;
            let audio = record.audio().expect("audio path requires audio metadata");
            if round_non_negative_millis_to_seconds(audio.start_ms()) > 0 {
                writeln!(
                    stdout,
                    "  offset  {}",
                    format_minute_seconds(audio.start_ms())
                )
                .into_diagnostic()?;
            }
            if !audio_path.is_file() {
                writeln!(
                    stdout,
                    "{}audio file is missing: {}",
                    style.note(),
                    audio_path.display()
                )
                .into_diagnostic()?;
            }
        }
    }
    Ok(())
}

struct RehearsalSlideTotal {
    index: u32,
    key: String,
    first_at_ms: u64,
    first_visit_end_ms: u64,
    visits: usize,
    total_ms: u64,
}

struct RehearsalSlideRow<'a> {
    slide: &'a str,
    key: &'a str,
    entered: &'a str,
    seek: Option<&'a str>,
    visits: &'a str,
    total: &'a str,
}

fn write_rehearsal_slide_summary(
    stdout: &mut dyn Write,
    record: &peitho_core::RehearsalRecordV2,
) -> miette::Result<()> {
    writeln!(stdout).into_diagnostic()?;
    if record.timeline().is_empty() {
        writeln!(stdout, "  (no slide entries)").into_diagnostic()?;
        return Ok(());
    }

    let totals = rehearsal_slide_totals(record);
    let key_width = totals
        .iter()
        .map(|total| total.key.chars().count())
        .chain(std::iter::once("key".len()))
        .max()
        .unwrap_or("key".len());
    let seek_start_ms = record
        .audio()
        .map(peitho_core::RehearsalAudio::start_ms)
        .filter(|start_ms| round_non_negative_millis_to_seconds(*start_ms) > 0);
    let seek_width = seek_start_ms.map_or(0, |_| 3 + 7);
    let table_width = 2 + 5 + 3 + key_width + 3 + 7 + seek_width + 3 + 6 + 3 + 5;
    write_rehearsal_slide_row(
        stdout,
        key_width,
        RehearsalSlideRow {
            slide: "slide",
            key: "key",
            entered: "entered",
            seek: seek_start_ms.map(|_| "seek"),
            visits: "visits",
            total: "total",
        },
    )?;

    let leading_ms = record.timeline()[0].at_ms();
    if leading_ms > 0 {
        write_rehearsal_slide_total_row(
            stdout,
            table_width,
            "(before first entry)",
            seek_start_ms.map(|_| "-"),
            leading_ms,
        )?;
    }
    for total in &totals {
        let slide = format!("#{}", u64::from(total.index) + 1);
        let seek = seek_start_ms.map(|start_ms| {
            if total.first_visit_end_ms <= start_ms {
                "-".to_owned()
            } else {
                format_minute_seconds(total.first_at_ms.saturating_sub(start_ms))
            }
        });
        write_rehearsal_slide_row(
            stdout,
            key_width,
            RehearsalSlideRow {
                slide: &slide,
                key: &total.key,
                entered: &format_minute_seconds(total.first_at_ms),
                seek: seek.as_deref(),
                visits: &total.visits.to_string(),
                total: &format_minute_seconds(total.total_ms),
            },
        )?;
    }
    let displayed_total_ms = leading_ms + totals.iter().map(|total| total.total_ms).sum::<u64>();
    debug_assert_eq!(displayed_total_ms, record.elapsed_ms());
    write_rehearsal_slide_total_row(stdout, table_width, "total", None, displayed_total_ms)?;
    Ok(())
}

fn write_rehearsal_slide_row(
    stdout: &mut dyn Write,
    key_width: usize,
    row: RehearsalSlideRow<'_>,
) -> miette::Result<()> {
    let RehearsalSlideRow {
        slide,
        key,
        entered,
        seek,
        visits,
        total,
    } = row;
    if let Some(seek) = seek {
        writeln!(
            stdout,
            "  {slide:<5}   {key:<key_width$}   {entered:>7}   {seek:>7}   {visits:>6}   {total:>5}"
        )
        .into_diagnostic()
    } else {
        writeln!(
            stdout,
            "  {slide:<5}   {key:<key_width$}   {entered:>7}   {visits:>6}   {total:>5}"
        )
        .into_diagnostic()
    }
}

fn write_rehearsal_slide_total_row(
    stdout: &mut dyn Write,
    table_width: usize,
    label: &str,
    seek: Option<&str>,
    total_ms: u64,
) -> miette::Result<()> {
    let label = format!("  {label}");
    if let Some(seek) = seek {
        let label_width = table_width - 7 - 3 - 6 - 3 - 5;
        return writeln!(
            stdout,
            "{label:<label_width$}{seek:>7}   {:>6}   {:>5}",
            "",
            format_minute_seconds(total_ms)
        )
        .into_diagnostic();
    }
    let label_width = table_width - 5;
    writeln!(
        stdout,
        "{label:<label_width$}{:>5}",
        format_minute_seconds(total_ms)
    )
    .into_diagnostic()
}

fn rehearsal_slide_totals(record: &peitho_core::RehearsalRecordV2) -> Vec<RehearsalSlideTotal> {
    let timeline = record.timeline();
    let mut totals: Vec<RehearsalSlideTotal> = Vec::new();
    for (position, entry) in timeline.iter().enumerate() {
        let end_ms = timeline
            .get(position + 1)
            .map(peitho_core::RehearsalSlideEntry::at_ms)
            .unwrap_or_else(|| record.elapsed_ms());
        let duration_ms = end_ms - entry.at_ms();
        if let Some(total) = totals
            .iter_mut()
            .find(|total| total.index == entry.index() && total.key == entry.key().as_str())
        {
            total.visits += 1;
            total.total_ms += duration_ms;
        } else {
            totals.push(RehearsalSlideTotal {
                index: entry.index(),
                key: entry.key().as_str().to_owned(),
                first_at_ms: entry.at_ms(),
                first_visit_end_ms: end_ms,
                visits: 1,
                total_ms: duration_ms,
            });
        }
    }
    totals
}

fn write_rehearsal_row(
    stdout: &mut dyn Write,
    name_width: usize,
    name: &str,
    planned: &str,
    actual: &str,
    delta: &str,
) -> miette::Result<()> {
    writeln!(
        stdout,
        "  {name:<name_width$}    {planned:>7}   {actual:>6}    {delta:>5}"
    )
    .into_diagnostic()
}

fn local_recorded_minute(path: &Path, recorded_at_ms: u64) -> miette::Result<String> {
    let recorded_at_ms = i64::try_from(recorded_at_ms).map_err(|_| {
        let help = rehearsal_record_recovery_help(path);
        miette::miette!(
            help = help,
            "rehearsal record timestamp is outside the supported range in rehearsal record {}",
            path.display()
        )
    })?;
    let recorded = chrono::Local
        .timestamp_millis_opt(recorded_at_ms)
        .single()
        .ok_or_else(|| {
            let help = rehearsal_record_recovery_help(path);
            miette::miette!(
                help = help,
                "rehearsal record timestamp is outside the supported range in rehearsal record {}",
                path.display()
            )
        })?;
    Ok(recorded.format("%Y-%m-%d %H:%M").to_string())
}

fn format_minute_seconds(ms: u64) -> String {
    let total_seconds = round_non_negative_millis_to_seconds(ms);
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

fn round_non_negative_millis_to_seconds(ms: u64) -> u64 {
    ms.saturating_add(500) / 1000
}

fn format_rehearsal_delta(actual_ms: u64, planned_ms: u64) -> String {
    let diff_ms = i128::from(actual_ms) - i128::from(planned_ms);
    let diff_sec = round_millis_to_seconds(diff_ms);
    let sign = if diff_sec > 0 { '+' } else { '-' };
    format!("{sign}{}", format_signed_abs_seconds(diff_sec))
}

fn round_millis_to_seconds(ms: i128) -> i128 {
    if ms >= 0 {
        (ms + 500) / 1000
    } else {
        -((-ms + 499) / 1000)
    }
}

fn format_signed_abs_seconds(seconds: i128) -> String {
    let total_seconds = seconds.unsigned_abs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

fn read_publish_manifest(dist: &Path) -> miette::Result<peitho_core::Manifest> {
    let path = dist.join("manifest.json");
    let json = fs::read_to_string(&path).map_err(|err| {
        miette::miette!(
            help = "run `peitho build` first",
            "failed to read manifest.json\ncaused by: {err}"
        )
    })?;

    let manifest: peitho_core::Manifest = serde_json::from_str(&json).map_err(|err| {
        miette::miette!(
            help = "run `peitho build` first",
            "failed to parse manifest.json\ncaused by: {err}"
        )
    })?;

    validate_manifest_refs(dist, &manifest)?;
    Ok(manifest)
}

fn validate_manifest_refs(dist: &Path, manifest: &peitho_core::Manifest) -> miette::Result<()> {
    if manifest.slide_count() != manifest.slides().len() {
        return Err(miette::miette!(
            help = "run `peitho build` first",
            "manifest slideCount does not match slides length"
        ));
    }

    if manifest.slides().is_empty() || manifest.slide_count() == 0 {
        return Err(miette::miette!(
            help = "run `peitho build` first",
            "manifest has no slides"
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
            help = kind.invalid_help(),
            "{}",
            kind.invalid_message(src)
        ));
    }

    let canonical_dist = fs::canonicalize(dist).map_err(|err| {
        miette::miette!(
            help = "run `peitho build` first",
            "distribution is incomplete: failed to resolve dist directory\ncaused by: {err}"
        )
    })?;
    let target = dist.join(path);
    let canonical_target = match fs::canonicalize(&target) {
        Ok(path) => path,
        Err(_) => {
            return Err(miette::miette!(
                help = "run `peitho build` first",
                "{}",
                kind.missing_message(src)
            ));
        }
    };
    if !canonical_target.starts_with(&canonical_dist) {
        return Err(miette::miette!(
            help = kind.invalid_help(),
            "{}",
            kind.invalid_message(src)
        ));
    }

    if !canonical_target.is_file() {
        return Err(miette::miette!(
            help = "run `peitho build` first",
            "{}",
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
        help = "run `peitho build` first",
        "distribution is incomplete: missing {file}"
    ))
}

fn require_slides_dir_with_files(dist: &Path) -> miette::Result<()> {
    let slides = dist.join("slides");
    if !slides.is_dir() {
        return Err(miette::miette!(
            help = "run `peitho build` first",
            "distribution is incomplete: missing slides/"
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
            help = "run `peitho build` first",
            "distribution is incomplete: slides/ must contain at least one file"
        ))
    }
}

fn present(options: PresentOptions) -> miette::Result<()> {
    validate_present_options(&options)?;
    let resolved_host = resolve_present_host(options.host)?;
    let resolved_port = resolve_present_port(options.port, &resolved_host, options.audio);

    let cache = PathBuf::from(PRESENT_CACHE);
    if cache.exists() {
        fs::remove_dir_all(&cache).into_diagnostic()?;
    }
    fs::create_dir_all(&cache).into_diagnostic()?;

    let artifacts = build_artifacts(&options.input)?;
    if options.rehearsal {
        validate_rehearsal_sections(&artifacts)?;
    }
    if options.no_serve {
        emit_present_cache(&cache, &artifacts, options.shell.as_deref(), false, false)?;
        println!("generated present cache at {}", cache.display());
        return Ok(());
    }

    let rehearsal_sink = if options.rehearsal {
        Some(server::RehearsalSink::new(
            PathBuf::from(REHEARSALS_DIR),
            expected_rehearsal_sections(&artifacts),
            expected_rehearsal_slide_keys(&artifacts),
            if options.audio {
                server::AudioRecording::Enabled
            } else {
                server::AudioRecording::Disabled
            },
        ))
    } else {
        None
    };
    let mut server =
        bind_present_server(cache.clone(), resolved_port, "present.html", &resolved_host)?;
    if let Some(sink) = rehearsal_sink {
        server = server.with_rehearsal_sink(sink);
    }
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
    emit_present_cache(
        &cache,
        &artifacts,
        options.shell.as_deref(),
        presenter_open,
        options.audio,
    )?;
    println!("serving presentation at {url}");
    if options.rehearsal {
        println!("{}", rehearsal_startup_line(options.audio));
    }
    if let Some(target) =
        remote_control_target_for_resolved_host(&resolved_host, server.addr().port())
    {
        let output = remote_control_output(&target);
        for line in output.lines {
            println!("{line}");
        }
        if let Some(qr) = output.qr {
            match peitho::qr::qr_unicode_lines(qr.url.as_str()) {
                Ok(lines) => {
                    println!();
                    println!("{}", qr.caption);
                    for line in lines {
                        println!("{line}");
                    }
                }
                Err(err) => {
                    eprintln!(
                        "{}failed to render remote control QR for {}: {err}",
                        LabelStyle::for_stderr().warning(),
                        qr.url
                    );
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

fn rehearsal_startup_line(audio: bool) -> String {
    if audio {
        format!("recording rehearsal (with audio) to {REHEARSALS_DIR}/")
    } else {
        format!("recording rehearsal to {REHEARSALS_DIR}/")
    }
}

fn validate_present_options(options: &PresentOptions) -> miette::Result<()> {
    if options.rehearsal && options.no_serve {
        return Err(miette::miette!(
            help = "remove --no-serve or omit --rehearsal",
            "--rehearsal requires the present server"
        ));
    }
    let Some(host) = options.host else {
        return Ok(());
    };
    if options.no_serve {
        return Err(miette::miette!(
            help = "remove --no-serve or omit --host",
            "--host requires the present server"
        ));
    }
    if host.is_some_and(|host| host.is_loopback()) {
        return Err(miette::miette!(
            help = "use the default loopback server without --host, or pass a reachable VPN/LAN IP address",
            "--host must be non-loopback"
        ));
    }
    Ok(())
}

fn validate_rehearsal_sections(artifacts: &BuildArtifacts) -> miette::Result<()> {
    if artifacts.rendered.settings().sections().is_empty() {
        return Err(miette::miette!(
            help = "declare {\"section\":...} page comments to define the agenda",
            "--rehearsal requires agenda sections"
        ));
    }
    Ok(())
}

fn expected_rehearsal_sections(artifacts: &BuildArtifacts) -> Vec<(String, u64)> {
    artifacts
        .rendered
        .settings()
        .sections()
        .iter()
        .map(|section| (section.name().to_owned(), section.planned().as_millis()))
        .collect()
}

fn expected_rehearsal_slide_keys(artifacts: &BuildArtifacts) -> Vec<SlideKey> {
    artifacts
        .rendered
        .slides()
        .iter()
        .map(|slide| slide.key().clone())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedPresentPort {
    port: u16,
    source: PresentPortSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentPortSource {
    Explicit,
    RandomDefault,
    StableDefault,
}

fn resolve_present_port(
    port: Option<u16>,
    host: &ResolvedPresentHost,
    audio: bool,
) -> ResolvedPresentPort {
    match port {
        Some(port) => ResolvedPresentPort {
            port,
            source: PresentPortSource::Explicit,
        },
        None if audio || !matches!(host, ResolvedPresentHost::None) => ResolvedPresentPort {
            port: STABLE_PRESENT_PORT,
            source: PresentPortSource::StableDefault,
        },
        None => ResolvedPresentPort {
            port: 0,
            source: PresentPortSource::RandomDefault,
        },
    }
}

fn bind_present_server(
    root: PathBuf,
    port: ResolvedPresentPort,
    default_document: &'static str,
    resolved_host: &ResolvedPresentHost,
) -> miette::Result<server::PresentServer> {
    server::PresentServer::bind_with_remote_assets(
        root,
        port.port,
        default_document,
        resolved_host.bind_host(),
        true,
    )
    .map_err(|err| annotate_present_bind_error(port, err))
}

fn annotate_present_bind_error(port: ResolvedPresentPort, err: miette::Report) -> miette::Report {
    if port.source == PresentPortSource::StableDefault
        && present_server_bind_error_kind(&err) == Some(io::ErrorKind::AddrInUse)
    {
        return miette::Report::new(PresentDefaultPortInUseError {
            port: port.port,
            source: err,
        });
    }
    err
}

fn present_server_bind_error_kind(err: &miette::Report) -> Option<io::ErrorKind> {
    err.downcast_ref::<server::PresentServerBindError>()
        .map(server::PresentServerBindError::io_kind)
}

#[derive(Debug)]
struct PresentDefaultPortInUseError {
    port: u16,
    source: miette::Report,
}

#[cfg(test)]
impl PresentDefaultPortInUseError {
    fn source_io_kind(&self) -> Option<io::ErrorKind> {
        present_server_bind_error_kind(&self.source)
    }
}

impl fmt::Display for PresentDefaultPortInUseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to bind default present port {}", self.port)
    }
}

impl Error for PresentDefaultPortInUseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(<miette::Report as AsRef<dyn Error>>::as_ref(&self.source))
    }
}

impl miette::Diagnostic for PresentDefaultPortInUseError {
    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(
            "another `peitho present` is probably running; pass `--port` to choose another port, or close the other instance",
        ))
    }

    fn diagnostic_source(&self) -> Option<&dyn miette::Diagnostic> {
        Some(self.source.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoHostCandidate {
    address: IpAddr,
    label: Option<peitho::remote_url::RemoteUrlLabel>,
}

enum ResolvedPresentHost {
    None,
    Explicit(IpAddr),
    Auto(AutoHostCandidate),
}

impl ResolvedPresentHost {
    fn bind_host(&self) -> Option<IpAddr> {
        match self {
            Self::None => None,
            Self::Explicit(host) => Some(*host),
            Self::Auto(candidate) => Some(candidate.address),
        }
    }
}

struct RemoteControlEndpoint {
    url: peitho::remote_url::RemoteUrl,
    label: Option<peitho::remote_url::RemoteUrlLabel>,
}

enum RemoteControlTarget {
    Specific(RemoteControlEndpoint),
    Candidates(Vec<peitho::remote_url::RemoteUrlCandidate>),
}

struct RemoteControlOutput<'a> {
    lines: Vec<String>,
    qr: Option<RemoteControlQr<'a>>,
}

struct RemoteControlQr<'a> {
    url: &'a peitho::remote_url::RemoteUrl,
    caption: String,
}

fn resolve_present_host(host: Option<Option<IpAddr>>) -> miette::Result<ResolvedPresentHost> {
    match host {
        None => Ok(ResolvedPresentHost::None),
        Some(Some(host)) => Ok(ResolvedPresentHost::Explicit(host)),
        Some(None) => Ok(ResolvedPresentHost::Auto(
            auto_host_candidate_from_interfaces()?,
        )),
    }
}

fn auto_host_candidate_from_interfaces() -> miette::Result<AutoHostCandidate> {
    let addrs = if_addrs::get_if_addrs()
        .into_diagnostic()
        .map_err(|err| {
            miette::miette!(
                help = "pass --host <IP> explicitly or check the network",
                "failed to enumerate network interfaces for --host\ncaused by: {err}"
            )
        })?
        .into_iter()
        .map(|addr| addr.ip())
        .collect::<Vec<_>>();
    auto_host_candidate_from_addresses(&addrs, default_route_ipv4())
}

fn auto_host_candidate_from_addresses(
    addrs: &[IpAddr],
    default_route: Option<IpAddr>,
) -> miette::Result<AutoHostCandidate> {
    let candidate = peitho::remote_url::remote_url_candidates(addrs, default_route, 0, None)
        .into_iter()
        .next()
        .ok_or_else(|| {
            miette::miette!(
                help = "pass --host <IP> explicitly or check the network",
                "no non-loopback network address found for --host"
            )
        })?;
    Ok(AutoHostCandidate {
        address: candidate.address,
        label: candidate.label,
    })
}

fn remote_control_target_for_resolved_host(
    host: &ResolvedPresentHost,
    port: u16,
) -> Option<RemoteControlTarget> {
    match host {
        ResolvedPresentHost::None => None,
        ResolvedPresentHost::Explicit(host) => Some(remote_control_target_for_host(*host, port)),
        ResolvedPresentHost::Auto(candidate) => Some(RemoteControlTarget::Specific(
            remote_control_endpoint(candidate.address, port, candidate.label),
        )),
    }
}

fn remote_control_target_for_host(host: IpAddr, port: u16) -> RemoteControlTarget {
    if host.is_unspecified() {
        RemoteControlTarget::Candidates(remote_url_candidates_from_interfaces(port, host))
    } else {
        RemoteControlTarget::Specific(remote_control_endpoint(
            host,
            port,
            peitho::remote_url::remote_url_label(host),
        ))
    }
}

fn remote_control_endpoint(
    host: IpAddr,
    port: u16,
    label: Option<peitho::remote_url::RemoteUrlLabel>,
) -> RemoteControlEndpoint {
    RemoteControlEndpoint {
        url: peitho::remote_url::remote_url_for_addr(host, port),
        label,
    }
}

fn remote_control_output(target: &RemoteControlTarget) -> RemoteControlOutput<'_> {
    match target {
        RemoteControlTarget::Specific(endpoint) => RemoteControlOutput {
            lines: vec![format_remote_control_line(endpoint.label, &endpoint.url)],
            qr: Some(remote_control_qr(endpoint.label, &endpoint.url)),
        },
        RemoteControlTarget::Candidates(candidates) if candidates.is_empty() => {
            RemoteControlOutput {
                lines: vec!["remote control: no non-loopback network addresses found".to_owned()],
                qr: None,
            }
        }
        RemoteControlTarget::Candidates(candidates) => {
            let mut lines = Vec::with_capacity(candidates.len());
            let mut qr = None;
            for candidate in candidates {
                if qr.is_none() {
                    qr = Some(remote_control_qr(candidate.label, &candidate.url));
                }
                lines.push(format_remote_control_line(candidate.label, &candidate.url));
            }
            RemoteControlOutput { lines, qr }
        }
    }
}

fn format_remote_control_line(
    label: Option<peitho::remote_url::RemoteUrlLabel>,
    url: &peitho::remote_url::RemoteUrl,
) -> String {
    match label {
        Some(label) => format!("remote control ({}): {url}", label.as_str()),
        None => format!("remote control: {url}"),
    }
}

fn remote_control_qr(
    label: Option<peitho::remote_url::RemoteUrlLabel>,
    url: &peitho::remote_url::RemoteUrl,
) -> RemoteControlQr<'_> {
    RemoteControlQr {
        url,
        caption: format_qr_caption(label, url),
    }
}

fn format_qr_caption(
    label: Option<peitho::remote_url::RemoteUrlLabel>,
    url: &peitho::remote_url::RemoteUrl,
) -> String {
    match label {
        Some(label) => format!("scan to open ({}): {url}", label.as_str()),
        None => format!("scan to open: {url}"),
    }
}

fn remote_url_candidates_from_interfaces(
    port: u16,
    bound_wildcard: IpAddr,
) -> Vec<peitho::remote_url::RemoteUrlCandidate> {
    let addrs = match if_addrs::get_if_addrs() {
        Ok(addrs) => addrs.into_iter().map(|addr| addr.ip()).collect::<Vec<_>>(),
        Err(err) => {
            eprintln!(
                "{}failed to enumerate network interfaces for remote URL: {err}",
                LabelStyle::for_stderr().warning()
            );
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
    let (watch, root) = run_initial_action_after_watch_snapshot(options.input.clone(), || {
        emit_initial_preview_root(&options.input, &cache, &mut std::io::stderr())
    })?;

    let server = server::PresentServer::bind(root, options.port, "index.html")?.with_deck_writer(
        PreviewDeckWriter {
            input: options.input.clone(),
        },
    );
    let url = server.preview_url();
    let _watch = spawn_preview_watch(watch, cache, server.clone());
    println!("serving preview at {url}");
    std::io::stdout().flush().into_diagnostic()?;
    if !options.no_open {
        let mut stderr = std::io::stderr();
        let style = LabelStyle::for_stream(&stderr);
        open_preview_browser_or_warn(&url, &mut stderr, style, open_default_browser)?;
    }
    server.serve_forever()
}

fn spawn_preview_watch(
    watch: WatchState,
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
    watch: WatchState,
    cache: PathBuf,
    server: server::PresentServer,
) -> miette::Result<()> {
    let rebuild_input = watch.input.clone();
    watch_paths_loop(watch, move |stdout, stderr| {
        rebuild_preview_once_for_watch(&rebuild_input, &cache, &server, stdout, stderr)
    })
}

trait PreviewReloadTarget {
    fn generation(&self) -> u64;
    fn swap_root(&self, root: PathBuf);
    fn broadcast_reload(&self) -> u64;
    fn report_build_error(&self, error: String) -> u64;
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

    fn report_build_error(&self, error: String) -> u64 {
        server::PresentServer::report_build_error(self, error)
    }
}

fn emit_initial_preview_root(
    input: &Path,
    cache: &Path,
    stderr: &mut dyn Write,
) -> miette::Result<PathBuf> {
    match build_preview_artifacts(input) {
        Ok(artifacts) => {
            let root = emit_preview_cache_generation(cache, 0, &artifacts)?;
            prune_preview_cache_generations(cache, 0)?;
            Ok(root)
        }
        Err(err) => {
            writeln!(stderr, "build failed:\n{}", render_diagnostic(&err)).into_diagnostic()?;
            stderr.flush().into_diagnostic()?;
            let root = emit_preview_error_page(cache, 0, &plain_diagnostic_text(&err))?;
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
    match build_preview_artifacts(input).and_then(|artifacts| {
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
            writeln!(stderr, "build failed:\n{}", render_diagnostic(&err)).into_diagnostic()?;
            stderr.flush().into_diagnostic()?;
            server.report_build_error(plain_diagnostic_text(&err));
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
            help = format!("pass --no-open and open {url} manually"),
            "cannot open preview browser on this platform"
        ));
    };
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|err| {
            miette::miette!(
                help = format!("pass --no-open and open {url} manually"),
                "failed to open preview browser with {program}\ncaused by: {err}"
            )
        })
}

fn open_preview_browser_or_warn<F>(
    url: &str,
    stderr: &mut dyn Write,
    style: LabelStyle,
    open: F,
) -> miette::Result<()>
where
    F: FnOnce(&str) -> miette::Result<()>,
{
    if let Err(err) = open(url) {
        writeln!(
            stderr,
            "{}failed to open preview browser: {err}",
            style.warning()
        )
        .into_diagnostic()?;
        writeln!(stderr, "{}open {url} manually", style.help()).into_diagnostic()?;
        stderr.flush().into_diagnostic()?;
    }
    Ok(())
}

fn emit_present_cache(
    cache: &Path,
    artifacts: &BuildArtifacts,
    shell: Option<&Path>,
    presenter_open: bool,
    rehearsal_audio: bool,
) -> miette::Result<()> {
    if let Some(shell) = shell {
        ensure_shell_bundle(shell)?;
    }
    write_shared_assets(cache, artifacts)?;
    write_slide_fragments(cache, &artifacts.rendered)?;
    fs::write(cache.join("manifest.json"), &artifacts.manifest_json).into_diagnostic()?;
    write_notes_json(cache, artifacts)?;
    fs::write(
        cache.join("present.json"),
        core(peitho_core::present_config_json(
            &peitho_core::PresentConfig::new(presenter_open, rehearsal_audio),
        ))?,
    )
    .into_diagnostic()?;
    fs::write(
        cache.join("present.html"),
        peitho_core::render_present_index(
            artifacts.rendered.settings().aspect_ratio(),
            artifacts.rendered.settings().lang(),
        ),
    )
    .into_diagnostic()?;
    fs::write(
        cache.join("presenter.html"),
        peitho_core::render_presenter_index(
            artifacts.rendered.settings().aspect_ratio(),
            artifacts.rendered.settings().lang(),
        ),
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

fn write_notes_json(dir: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    fs::write(
        dir.join("notes.json"),
        core(peitho_core::notes_json(&peitho_core::Notes::from_slides(
            artifacts.rendered.slides(),
        )))?,
    )
    .into_diagnostic()
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
        generation_dir.join("sources.json"),
        &artifacts.slide_sources_json,
    )
    .into_diagnostic()?;
    write_notes_json(&generation_dir, artifacts)?;
    fs::write(
        generation_dir.join("index.html"),
        peitho_core::render_preview_index(
            artifacts.rendered.settings().aspect_ratio(),
            artifacts.rendered.settings().lang(),
        ),
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
        help = "run `cd packages/peitho-present && npm run build` or pass --shell <path>",
        "shell bundle not found: {}",
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
    result.map_err(|err| miette::Report::new(DeckDiagnostic::new(err)))
}

fn core_for_deck<T>(
    result: peitho_core::Result<T>,
    deck: &Path,
    line_map: Option<&peitho_core::include::LineMap>,
) -> miette::Result<T> {
    result.map_err(|err| {
        miette::Report::new(DeckDiagnostic::new(translate_deck_error(
            err, deck, line_map,
        )))
    })
}

fn translate_deck_error(
    mut err: peitho_core::BuildError,
    deck: &Path,
    line_map: Option<&peitho_core::include::LineMap>,
) -> peitho_core::BuildError {
    if let Some(origin) = err.origin_file.take() {
        err.origin_file = Some(origin_for_display(&origin, deck));
        return err;
    }

    let Some(line) = err.line else {
        return err;
    };
    let Some(line_map) = line_map else {
        err.origin_file = Some(origin_for_display(deck, deck));
        return err;
    };
    let (origin, original_line) = line_map.translate(line);
    if origin.as_os_str().is_empty() {
        return err;
    }
    err.line = Some(original_line);
    if !same_watch_path(&origin, deck) {
        err.origin_file = Some(origin_for_display(&origin, deck));
    }
    err
}

fn origin_for_display(origin: &Path, deck: &Path) -> PathBuf {
    origin
        .strip_prefix(asset_resolution::deck_parent(deck))
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| origin.to_path_buf())
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
    use chrono::TimeZone;
    use lol_html::{element, rewrite_str, RewriteStrSettings};
    use peitho_core::code_images::BUILTIN_EMBED_PARAMS;
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
    const TEST_IMAGE_LAYOUT_HTML: &str = r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="image" accepts="image" arity="1"></slot></section>"#;

    struct DeterministicSvgRunner;

    impl peitho_core::code_images::SvgRunner for DeterministicSvgRunner {
        fn run(
            &self,
            _command: &peitho_core::domain::CodeImageCommand,
            _stdin: &str,
        ) -> peitho_core::Result<Vec<u8>> {
            Ok(br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50"><text x="1" y="20">fixture</text></svg>"#.to_vec())
        }
    }

    struct DeterministicEmbedRenderer;

    impl peitho_core::code_images::EmbedRenderer for DeterministicEmbedRenderer {
        fn render(
            &self,
            _normalized_url: &str,
            _params: peitho_core::code_images::EmbedRenderParams,
        ) -> peitho_core::Result<Vec<u8>> {
            Ok(b"\x89PNG\r\n\x1a\nfixture".to_vec())
        }
    }

    struct DeterministicOEmbedFetcher;

    impl peitho_core::code_images::OEmbedFetcher for DeterministicOEmbedFetcher {
        fn fetch(&self, normalized_url: &str) -> peitho_core::Result<String> {
            Ok(serde_json::json!({
                "html": format!(
                    r#"<blockquote class="twitter-tweet"><p>fixture</p><a href="{normalized_url}">January 1, 2026</a></blockquote>"#
                ),
                "author_name": "Fixture",
                "url": normalized_url,
            })
            .to_string())
        }

        fn fetch_discovery_page(&self, _page_url: &str) -> peitho_core::Result<Vec<u8>> {
            Ok(br#"<html><head><link rel="alternate" type="application/json+oembed" href="/oembed.json"></head></html>"#.to_vec())
        }

        fn fetch_discovered_oembed(&self, _endpoint_url: &str) -> peitho_core::Result<Vec<u8>> {
            Ok(br#"{"type":"link","title":"Fixture","author_name":"Peitho","provider_name":"Fixture Provider"}"#.to_vec())
        }

        fn fetch_thumbnail(&self, _image_url: &str) -> peitho_core::Result<Vec<u8>> {
            Ok(b"\x89PNG\r\n\x1a\nfixture-thumbnail".to_vec())
        }
    }

    fn deterministic_example_slide_bytes(html: &str) -> Vec<u8> {
        let html = &version_independent_mermaid_asset_names(html);
        if !html.contains(r#"class="peitho-math""#) {
            return html.as_bytes().to_vec();
        }

        // katex-rs stores generated attributes and declarations in randomly
        // seeded hash maps. Canonicalize only that generated subtree so every
        // other byte in each example slide remains pinned verbatim.
        rewrite_str(
            html,
            RewriteStrSettings {
                element_content_handlers: vec![element!(
                    ".peitho-math, .peitho-math *",
                    |element| {
                        let mut attributes = element
                            .attributes()
                            .iter()
                            .map(|attribute| {
                                let name = attribute.name_preserve_case();
                                let mut value = attribute.value();
                                if name.eq_ignore_ascii_case("style") {
                                    let mut declarations = value
                                        .split(';')
                                        .map(str::trim)
                                        .filter(|declaration| !declaration.is_empty())
                                        .collect::<Vec<_>>();
                                    declarations.sort_unstable();
                                    value = declarations.join(";");
                                    if !value.is_empty() {
                                        value.push(';');
                                    }
                                }
                                (name, value)
                            })
                            .collect::<Vec<_>>();
                        for (name, _) in &attributes {
                            element.remove_attribute(name);
                        }
                        attributes.sort_unstable();
                        for (name, value) in attributes {
                            element.set_attribute(&name, &value)?;
                        }
                        Ok(())
                    }
                )],
                ..RewriteStrSettings::new()
            },
        )
        .expect("KaTeX snapshot HTML is valid")
        .into_bytes()
    }

    // The built-in Mermaid cache key hashes CARGO_PKG_VERSION and becomes the
    // asset file stem, so it changes on every release. Blank only that stem;
    // the content-hash prefix still pins the SVG bytes.
    fn version_independent_mermaid_asset_names(html: &str) -> String {
        if !html.contains(r#"alt="diagram (mermaid)""#) {
            return html.to_owned();
        }
        rewrite_str(
            html,
            RewriteStrSettings {
                element_content_handlers: vec![element!(
                    r#"img[alt="diagram (mermaid)"]"#,
                    |element| {
                        let src = element.get_attribute("src").expect("mermaid img has src");
                        let (content_hash, _) =
                            src.split_once('-').expect("asset name is <hash>-<stem>");
                        element.set_attribute("src", &format!("{content_hash}-mermaid.svg"))?;
                        Ok(())
                    }
                )],
                ..RewriteStrSettings::new()
            },
        )
        .expect("example slide HTML is valid")
    }

    fn has_arg(args: &[OsString], expected: &str) -> bool {
        args.iter().any(|arg| arg == OsStr::new(expected))
    }

    fn has_arg_prefix(args: &[OsString], prefix: &str) -> bool {
        args.iter()
            .any(|arg| arg.to_string_lossy().starts_with(prefix))
    }

    fn user_data_dir(args: &[OsString]) -> String {
        args.iter()
            .find_map(|arg| {
                arg.to_str()
                    .and_then(|arg| arg.strip_prefix("--user-data-dir="))
            })
            .expect("Chrome args include a user-data-dir")
            .to_owned()
    }

    fn fake_embed_chrome_output(
        args: &[OsString],
        completion: ChromeCompletion,
        height: u32,
        png: &[u8],
    ) -> miette::Result<ChromeOutput> {
        match completion {
            ChromeCompletion::EmbedMeasured => Ok(chrome_output(
                format!(
                    "<!doctype html><html><head><title>peitho-embed-height:{height}</title></head></html>"
                )
                .into_bytes(),
                Vec::new(),
            )),
            ChromeCompletion::PngWritten => {
                let output_path = args
                    .iter()
                    .find_map(|arg| {
                        arg.to_str()
                            .and_then(|arg| arg.strip_prefix("--screenshot="))
                    })
                    .map(PathBuf::from)
                    .expect("embed capture args include a screenshot path");
                fs::write(output_path, png).into_diagnostic()?;
                Ok(chrome_output(
                    Vec::new(),
                    b"bytes written to file".to_vec(),
                ))
            }
            ChromeCompletion::LintResultLogged => {
                panic!("embed orchestration must not request lint completion")
            }
        }
    }

    #[test]
    fn embed_wrapper_html_splits_measure_failure_release_from_strict_capture() {
        let measure = embed_wrapper_html(
            "https://x.com/gosukenator/status/2083825695709597710",
            BUILTIN_EMBED_PARAMS,
            EmbedWrapperMode::Measure,
        );
        let capture = embed_wrapper_html(
            "https://x.com/gosukenator/status/2083825695709597710",
            BUILTIN_EMBED_PARAMS,
            EmbedWrapperMode::Capture,
        );
        for html in [&measure, &capture] {
            assert!(html.contains(r#"class="twitter-tweet""#));
            assert!(html.contains(r#"data-width="550""#));
            assert!(html.contains(r#"data-theme="light""#));
            assert!(html.contains("https://platform.x.com/widgets.js"));
            assert!(html.contains("twttr.events.bind(\"rendered\""));
            assert!(html.contains("getBoundingClientRect().bottom"));
            assert!(!html.contains("getBoundingClientRect().height"));
            assert!(html.contains("function pollSettledBottom()"));
            assert!(html.contains("stableCount += 1;"));
            assert!(html.contains("stableCount = 1;"));
            assert!(html.contains("if (stableCount >= 3)"));
            assert!(html.contains("setTimeout(pollSettledBottom, 100);"));
            assert!(html.contains(r#"<iframe id="peitho-load-holder""#));
            assert!(html.contains("holder.contentDocument.open()"));
            assert!(html.contains(r#"holder.contentDocument.write("holding load")"#));
            assert!(html.contains("holder.contentDocument.close()"));
            assert!(html.contains("margin:0"));
            assert!(html.contains("div.twitter-tweet-rendered{margin:0 !important}"));
            assert!(!html.contains("image.decode"));
        }
        assert!(measure.contains("document.title = \"peitho-embed-height:\" + stableBottom;"));
        assert!(measure.contains("js.onerror = releaseLoad;"));
        assert!(measure.contains("setTimeout(releaseLoad, 15000);"));
        assert!(!capture.contains("document.title = \"peitho-embed-height:"));
        assert!(!capture.contains("js.onerror = releaseLoad;"));
        assert!(!capture.contains("setTimeout(releaseLoad, 15000);"));
    }

    #[test]
    fn embed_chrome_orchestration_measures_then_captures() {
        let temp = tempfile::tempdir().unwrap();
        let mut invocations = Vec::new();
        let png = render_embed_with_invoker(
            temp.path(),
            "https://x.com/a/status/1",
            BUILTIN_EMBED_PARAMS,
            |args, completion| {
                invocations.push(args.to_vec());
                fake_embed_chrome_output(args, completion, 742, b"\x89PNG\r\n\x1a\nfixture")
            },
        )
        .unwrap();
        assert_eq!(png, b"\x89PNG\r\n\x1a\nfixture");
        assert_eq!(invocations.len(), 2);
        assert!(has_arg(&invocations[0], "--dump-dom"));
        assert!(has_arg(&invocations[1], "--window-size=550,742"));
        assert!(has_arg(&invocations[1], "--force-device-scale-factor=2"));
        assert!(!has_arg_prefix(&invocations[0], "--virtual-time-budget"));
        assert!(!has_arg_prefix(&invocations[1], "--virtual-time-budget"));
        assert!(invocations[0]
            .last()
            .unwrap()
            .to_string_lossy()
            .ends_with("embed-measure.html"));
        assert!(invocations[1]
            .last()
            .unwrap()
            .to_string_lossy()
            .ends_with("embed-capture.html"));
        assert_ne!(
            user_data_dir(&invocations[0]),
            user_data_dir(&invocations[1])
        );
    }

    #[test]
    fn embed_chrome_height_parser_enforces_exclusive_measurement_ceiling() {
        assert_eq!(
            parse_embed_height(b"<title>peitho-embed-height:742</title>").unwrap(),
            742
        );
        assert_eq!(
            parse_embed_height(b"<title>peitho-embed-height:9999</title>").unwrap(),
            9999
        );
        assert!(parse_embed_height(b"<title>peitho-embed-pending</title>").is_err());
        assert!(parse_embed_height(b"<title>peitho-embed-height:0</title>").is_err());
        let err = parse_embed_height(b"<title>peitho-embed-height:10000</title>").unwrap_err();
        assert!(err.to_string().contains(
            "rendered embed height 10000 reaches the 10000px measurement viewport; the post is too tall to embed"
        ));
    }

    #[test]
    fn embed_measurement_completion_accepts_only_complete_peitho_titles() {
        let pending = b"<html><title>peitho-embed-pending</title></html>";
        let partial = b"<html><title>peitho-embed-pending";

        let mut running_state = ChromeCompletionState::default();
        assert!(ChromeCompletion::EmbedMeasured.is_ready(pending, &[], &mut running_state));
        let mut exited_state = ChromeCompletionState::default();
        assert!(
            ChromeCompletion::EmbedMeasured.is_ready_after_successful_exit(
                pending,
                &[],
                &mut exited_state
            )
        );
        let mut partial_running_state = ChromeCompletionState::default();
        assert!(!ChromeCompletion::EmbedMeasured.is_ready(
            partial,
            &[],
            &mut partial_running_state
        ));
        let mut partial_exited_state = ChromeCompletionState::default();
        assert!(
            !ChromeCompletion::EmbedMeasured.is_ready_after_successful_exit(
                partial,
                &[],
                &mut partial_exited_state
            )
        );

        let err = parse_embed_height(pending).unwrap_err();
        assert!(err
            .to_string()
            .contains("official X embed did not publish a valid rendered height"));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("check network access to X"));
        assert!(help.contains("post is public"));
    }

    #[test]
    fn load_and_expand_deck_source_returns_source_frontmatter_and_line_map() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let source = "---\ntime: 1m\n---\n# Intro\n";
        fs::write(&deck, source).unwrap();

        let loaded = load_and_expand_deck_source(&deck).unwrap();

        assert_eq!(loaded.source, source);
        assert_eq!(loaded.frontmatter.body_start(), 16);
        assert_eq!(loaded.line_map.translate(4), (deck.clone(), 4));
    }

    #[test]
    fn load_and_expand_deck_source_reports_distinct_included_files() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let shared = dir.path().join("shared");
        let intro = shared.join("intro.md");
        let outro = shared.join("outro.md");
        fs::create_dir_all(&shared).unwrap();
        fs::write(&intro, "# Intro\n").unwrap();
        fs::write(&outro, "# Outro\n").unwrap();
        fs::write(
            &deck,
            "<!-- {\"include\":\"shared/intro.md\"} -->\n---\n# Middle\n---\n<!-- {\"include\":\"shared/intro.md\"} -->\n---\n<!-- {\"include\":\"shared/outro.md\"} -->\n",
        )
        .unwrap();

        let loaded = load_and_expand_deck_source(&deck).unwrap();

        assert_eq!(loaded.included_files(), vec![intro, outro]);
    }

    #[test]
    fn load_and_expand_deck_source_frontmatter_errors_include_deck_filename() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "---\ntime: [\n---\n# Intro\n").unwrap();

        let err = match load_and_expand_deck_source(&deck) {
            Ok(_) => panic!("invalid frontmatter should fail"),
            Err(err) => err,
        };

        let message = err.to_string();
        assert!(
            message.contains("deck.md:3: invalid deck frontmatter"),
            "actual error: {message}"
        );
    }

    #[test]
    fn stable_snapshot_candidate_requires_two_identical_changed_captures() {
        let deck = PathBuf::from("deck.md");
        let installed = BTreeMap::from([(deck.clone(), InputFingerprint::Content(1))]);
        let changed = BTreeMap::from([(deck.clone(), InputFingerprint::Content(2))]);
        let newer = BTreeMap::from([(deck, InputFingerprint::Content(3))]);
        let mut candidate = None;

        assert_eq!(
            stable_snapshot_candidate(&installed, &mut candidate, installed.clone()),
            None
        );
        assert_eq!(candidate, None);
        assert_eq!(
            stable_snapshot_candidate(&installed, &mut candidate, changed.clone()),
            None
        );
        assert_eq!(candidate, Some(changed.clone()));
        assert_eq!(
            stable_snapshot_candidate(&installed, &mut candidate, newer.clone()),
            None
        );
        assert_eq!(candidate, Some(newer.clone()));
        assert_eq!(
            stable_snapshot_candidate(&installed, &mut candidate, newer.clone()),
            Some(newer)
        );
        assert_eq!(candidate, None);
    }

    #[test]
    fn input_snapshot_tracks_precise_text_and_binary_inputs_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let deck = root.join("deck.md");
        let included = root.join("includes/intro.md");
        let image = root.join("images/photo.png");
        let layouts = root.join("layouts");
        let css = root.join("css");
        let syntaxes = root.join("syntaxes");
        let fonts = root.join("fonts");
        let nested_fonts = fonts.join("noto");
        let dist = root.join("dist");
        for path in [
            included.parent().unwrap(),
            image.parent().unwrap(),
            &layouts,
            &css,
            &syntaxes,
            &nested_fonts,
            &dist,
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(&deck, "# Intro\n").unwrap();
        fs::write(&included, "# Included\n").unwrap();
        fs::write(&image, b"image").unwrap();
        let layout = layouts.join("title.html");
        let stylesheet = css.join("talk.css");
        let syntax = syntaxes.join("talk.sublime-syntax");
        let font = nested_fonts.join("talk.woff2");
        fs::write(&layout, "<main></main>").unwrap();
        fs::write(&stylesheet, "main {}").unwrap();
        fs::write(&syntax, "%YAML 1.2").unwrap();
        fs::write(&font, b"font").unwrap();
        let sibling_video = root.join("talk.mp4");
        fs::write(&sibling_video, b"large binary stand-in").unwrap();
        fs::write(dist.join("index.html"), "generated").unwrap();

        let targets = WatchTargets::new(
            deck.clone(),
            ResolvedAssets {
                layouts: Provenance::Explicit(layouts.clone()),
                css: Provenance::Explicit(css.clone()),
                overrides: Provenance::Absent,
                syntaxes: Provenance::Explicit(syntaxes.clone()),
                fonts: Provenance::Explicit(fonts.clone()),
            },
            vec![included.clone()],
            vec![image.clone()],
        );
        let snapshot = capture_input_snapshot(&targets);

        for path in [&deck, &included, &layout, &stylesheet, &syntax] {
            assert!(matches!(
                snapshot.get(path),
                Some(InputFingerprint::Content(_))
            ));
        }
        for path in [&image, &font] {
            assert!(matches!(
                snapshot.get(path),
                Some(InputFingerprint::Metadata { .. })
            ));
        }
        assert!(!snapshot.contains_key(&sibling_video));
        assert!(!snapshot.contains_key(&dist.join("index.html")));
    }

    #[test]
    fn input_snapshot_filters_text_asset_extensions_and_unrelated_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let css = dir.path().join("css");
        fs::create_dir_all(&css).unwrap();
        fs::write(&deck, "# Intro\n").unwrap();
        let targets = WatchTargets::new(
            deck,
            ResolvedAssets {
                css: Provenance::Explicit(css.clone()),
                ..empty_assets()
            },
            Vec::new(),
            Vec::new(),
        );
        let before = capture_input_snapshot(&targets);

        fs::write(dir.path().join("talk.mp4"), b"large binary stand-in").unwrap();
        fs::write(css.join("notes.txt"), "not css").unwrap();
        assert_eq!(capture_input_snapshot(&targets), before);

        fs::write(css.join("talk.css"), "body {}\n").unwrap();
        assert_ne!(capture_input_snapshot(&targets), before);
    }

    #[cfg(unix)]
    #[test]
    fn dangling_emacs_lock_for_text_asset_does_not_rebuild() {
        use std::os::unix::fs::symlink;

        let fixture = WatchFixture::new("# Intro\n");
        let css = fixture._dir.path().join("css");
        let lock = css.join(".#talk.css");
        let mut state = watch_state_for_fixture(&fixture);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        symlink("talk.css", &lock).unwrap();
        run_watch_ticks(
            &mut state,
            3,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 0);
        assert!(!capture_input_snapshot(&state.targets).contains_key(&lock));
    }

    #[test]
    fn regular_text_asset_loaded_by_build_is_tracked() {
        let fixture = WatchFixture::new("# Intro\n");
        let css = fixture._dir.path().join("css");
        let stylesheet = css.join("talk.css");
        fs::write(&stylesheet, "body {}\n").unwrap();

        assert!(collect_asset_files(&css, "css")
            .unwrap()
            .contains(&stylesheet));
        assert!(matches!(
            capture_input_snapshot(&fixture.targets).get(&stylesheet),
            Some(InputFingerprint::Content(_))
        ));
    }

    #[test]
    fn input_snapshot_tracks_nested_fonts_but_ignores_hidden_entries() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let fonts = dir.path().join("fonts");
        let nested = fonts.join("noto");
        fs::create_dir_all(&nested).unwrap();
        fs::write(&deck, "# Intro\n").unwrap();
        fs::write(nested.join("400.woff2"), b"font").unwrap();
        fs::write(nested.join(".swap"), b"hidden").unwrap();
        let targets = WatchTargets::new(
            deck,
            ResolvedAssets {
                fonts: Provenance::Explicit(fonts.clone()),
                ..empty_assets()
            },
            Vec::new(),
            Vec::new(),
        );
        let snapshot = capture_input_snapshot(&targets);

        assert!(snapshot.contains_key(&fonts));
        assert!(snapshot.contains_key(&nested));
        assert!(snapshot.contains_key(&nested.join("400.woff2")));
        assert!(!snapshot.contains_key(&nested.join(".swap")));
    }

    #[test]
    fn input_snapshot_detects_candidate_asset_root_appearing_and_disappearing() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let css = dir.path().join("css");
        fs::write(&deck, "# Intro\n").unwrap();
        let targets = resolve_watch_targets(&deck).unwrap();
        let missing = capture_input_snapshot(&targets);
        assert_eq!(missing.get(&css), Some(&InputFingerprint::Missing));

        fs::create_dir(&css).unwrap();
        fs::write(css.join("talk.css"), "body {}\n").unwrap();
        let present = capture_input_snapshot(&targets);
        assert_ne!(present, missing);
        assert_eq!(present.get(&css), Some(&InputFingerprint::Directory));

        fs::remove_dir_all(&css).unwrap();
        assert_eq!(capture_input_snapshot(&targets), missing);
    }

    #[test]
    fn missing_overrides_directory_is_captured_as_a_watch_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let overrides = dir.path().join("overrides");
        fs::write(&deck, "# Intro\n").unwrap();

        let targets = resolve_watch_targets(&deck).unwrap();
        let snapshot = capture_input_snapshot(&targets);

        assert_eq!(snapshot.get(&overrides), Some(&InputFingerprint::Missing));
    }

    fn render_example_slides(
        edit_annotations: peitho_core::EditAnnotations,
    ) -> Vec<(PathBuf, String)> {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let examples_root = repository_root.join("examples");
        let isolated_root = tempfile::tempdir().unwrap();
        let mut examples = fs::read_dir(&examples_root)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        examples.sort_by_key(|entry| entry.file_name());

        let mut slides = Vec::new();
        for example in examples {
            if !example.file_type().unwrap().is_dir() || !example.path().join("deck.md").is_file() {
                continue;
            }
            let name = example.file_name();
            let isolated_example = isolated_root.path().join(&name);
            copy_dir_contents(&example.path(), &isolated_example).unwrap();
            let copied_cache = isolated_example.join(".peitho");
            if copied_cache.exists() {
                fs::remove_dir_all(copied_cache).unwrap();
            }

            let deck = isolated_example.join("deck.md");
            let artifacts = build_artifacts_with_services(
                &deck,
                &DeterministicSvgRunner,
                &DeterministicEmbedRenderer,
                &DeterministicOEmbedFetcher,
                edit_annotations,
            )
            .unwrap_or_else(|err| panic!("failed to build {}: {err:?}", deck.display()));
            for slide in artifacts.rendered.slides() {
                slides.push((
                    PathBuf::from(&name).join(slide.src()),
                    String::from_utf8(deterministic_example_slide_bytes(slide.html()))
                        .expect("rendered example slide is UTF-8"),
                ));
            }
        }
        slides.sort_by(|left, right| left.0.cmp(&right.0));
        slides
    }

    fn strip_example_edit_annotations(html: &str) -> String {
        let stripped = rewrite_str(
            html,
            RewriteStrSettings {
                element_content_handlers: vec![element!("[data-peitho-src]", |element| {
                    assert!(
                        element.get_attribute("data-peitho-md").is_some(),
                        "data-peitho-src must be paired with data-peitho-md"
                    );
                    element.remove_attribute("data-peitho-src");
                    element.remove_attribute("data-peitho-md");
                    if element.tag_name().eq_ignore_ascii_case("span") {
                        element.remove_and_keep_content();
                    }
                    Ok(())
                })],
                ..RewriteStrSettings::new()
            },
        )
        .expect("annotated example slide HTML is valid");
        assert!(!stripped.contains("data-peitho-src"), "{html}");
        assert!(!stripped.contains("data-peitho-md"), "{html}");
        stripped
    }

    #[test]
    fn edit_annotations_off_example_slide_hashes() {
        let slides = render_example_slides(peitho_core::EditAnnotations::Off);

        let mut digest_index = String::new();
        for (path, html) in slides {
            let line = format!(
                "{}  {}\n",
                short_sha256_hex(html.as_bytes(), 64),
                path.display()
            );
            digest_index.push_str(&line);
        }

        insta::assert_snapshot!("edit_annotations_off_example_slide_hashes", digest_index);
    }

    #[test]
    fn edit_annotations_on_minus_attributes_equals_off_for_examples() {
        let off = render_example_slides(peitho_core::EditAnnotations::Off);
        let on = render_example_slides(peitho_core::EditAnnotations::On);

        assert_eq!(on.len(), off.len());
        for ((on_path, on_html), (off_path, off_html)) in on.iter().zip(&off) {
            assert_eq!(on_path, off_path);
            assert_eq!(
                strip_example_edit_annotations(on_html),
                *off_html,
                "{}",
                off_path.display()
            );
        }
    }

    #[test]
    fn build_artifacts_uses_builtin_layout_and_theme_without_flags() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n\nBody\n\n```rust\nfn main() {}\n```\n").unwrap();

        let artifacts = build_artifacts(&deck).unwrap();

        assert_eq!(artifacts.slide_count, 1);
        assert!(artifacts
            .rendered
            .css()
            .contains("width: var(--peitho-canvas-width, 1280px);"));
    }

    #[test]
    fn build_artifacts_prepends_katex_css_only_for_math_decks() {
        let math = WatchFixture::new("# Math\n\n```math\n\\frac{1}{2}\n```\n");
        let plain = WatchFixture::new("# Plain\n\nBody\n");

        let math_artifacts = build_artifacts(&math.options.input).unwrap();
        let plain_artifacts = build_artifacts(&plain.options.input).unwrap();
        let math_css = math_artifacts.rendered.css();
        let plain_css = plain_artifacts.rendered.css();

        assert!(math_css.contains(".katex"));
        assert!(math_css.contains("url(katex-fonts/"));
        assert!(!math_css.contains("url(fonts/KaTeX_"));
        let katex_index = math_css.find("url(katex-fonts/").unwrap();
        let theme_index = math_css.find(".slot-title { font-weight: 700; }").unwrap();
        assert!(
            katex_index < theme_index,
            "KaTeX CSS must come before author/theme CSS so author rules win"
        );
        assert!(math_artifacts.rendered.slides()[0]
            .html()
            .contains(r#"<div class="peitho-math"><span class="katex-display""#));
        assert!(!plain_css.contains(".katex"));
        assert!(!plain_css.contains("katex-fonts"));
    }

    #[test]
    fn build_artifacts_prepends_static_emphasis_css_before_theme() {
        let fixture =
            WatchFixture::new("# Code\n\n```rust {2}\nlet first = 1;\nlet second = 2;\n```\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        write_shared_assets(&fixture.options.out, &artifacts).unwrap();
        let css = fs::read_to_string(fixture.options.out.join("peitho.css")).unwrap();

        assert!(artifacts.rendered.slides()[0]
            .html()
            .contains(r#"class="code-line code-line-emphasis""#));
        assert!(css.contains(".code-line-emphasis {"), "{css}");
        assert!(
            css.contains("pre:has(.code-line-emphasis) .code-line:not(.code-line-emphasis)"),
            "{css}"
        );
        let emphasis_index = css.find(".code-line-emphasis {").unwrap();
        let theme_index = css.find(".slot-title { font-weight: 700; }").unwrap();
        assert!(
            emphasis_index < theme_index,
            "static-emphasis CSS must come before author/theme CSS so author rules win"
        );
    }

    #[test]
    fn build_artifacts_omits_emphasis_css_for_stepped_only_decks() {
        let fixture =
            WatchFixture::new("# Code\n\n```rust {1|2}\nlet first = 1;\nlet second = 2;\n```\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        write_shared_assets(&fixture.options.out, &artifacts).unwrap();
        let css = fs::read_to_string(fixture.options.out.join("peitho.css")).unwrap();

        assert!(artifacts.rendered.slides()[0]
            .html()
            .contains("data-emphasis-step"));
        assert!(!css.contains(".code-line"), "{css}");
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

    fn preview_edit_coordinates(
        deck: &Path,
        key: &SlideKey,
        old: &str,
        occurrence: usize,
    ) -> (usize, usize) {
        let loaded = load_and_expand_deck_source(deck).unwrap();
        let (_, highlighter) = resolve_assets_and_highlighter(deck, &loaded.frontmatter).unwrap();
        let parsed = loaded
            .translate(peitho_core::parse_deck(
                &loaded.source,
                loaded.frontmatter.clone(),
                &highlighter,
            ))
            .unwrap();
        let slide = parsed
            .parsed_slides()
            .iter()
            .find(|slide| slide.key == *key)
            .unwrap();
        let span = slide
            .editable_spans()
            .into_iter()
            .filter(|span| {
                let span = span.source_span();
                loaded.source.get(span.start..span.end) == Some(old)
            })
            .nth(occurrence)
            .unwrap()
            .source_span();
        (span.start, span.end)
    }

    fn assert_preview_deck_drift<T>(result: Result<T, server::DeckWriteError>) {
        match result {
            Err(server::DeckWriteError::Conflict(message)) => {
                assert_eq!(message, "the deck changed on disk; reload and retry")
            }
            Err(err) => panic!("deck drift must be a conflict: {err:?}"),
            Ok(_) => panic!("deck drift must be refused"),
        }
    }

    const TOP_SOURCE: &str = "<!-- {\"include\":\"included.md\"} -->\n\n---\n\n# Top\n";

    fn include_deck_fixture(
        top_source: &'static str,
        included_source: &str,
    ) -> (tempfile::TempDir, PathBuf, PathBuf, &'static str) {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("included.md");
        fs::write(&deck, top_source).unwrap();
        fs::write(&included, included_source).unwrap();
        (dir, deck, included, top_source)
    }

    fn dispatch_preview_note(
        writer: &mut dyn server::DeckWriter,
        key: SlideKey,
        text: &str,
    ) -> Result<(), server::DeckWriteError> {
        writer.note(key, text.to_owned())
    }

    #[test]
    fn write_preview_slide_source_rejects_missing_key_and_body_drift_as_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let source = "<!-- {\"key\":\"current\"} -->\n# Current\n\nOriginal body\n";
        fs::write(&deck, source).unwrap();

        let missing = write_preview_slide_source(
            &deck,
            &SlideKey::new("missing").unwrap(),
            "# Current\n\nOriginal body",
            "# Replacement",
        )
        .unwrap_err();
        let server::DeckWriteError::Conflict(message) = missing else {
            panic!("missing slide key must be a conflict: {missing:?}");
        };
        assert!(message.contains("slide key 'missing' not found in current deck"));
        assert!(message.contains("reload preview and retry on a slide whose key still exists"));

        assert_preview_deck_drift(write_preview_slide_source(
            &deck,
            &SlideKey::new("current").unwrap(),
            "# Current\n\nStale body",
            "# Replacement",
        ));
        assert_eq!(fs::read_to_string(&deck).unwrap(), source);
    }

    #[test]
    fn write_preview_slide_source_keeps_note_separated_list_tight_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let body = "# T\n\n- item\n- next";
        fs::write(&deck, "# T\n\n- item\n  <!-- note -->\n- next\n").unwrap();

        let (key, saved_body) =
            write_preview_slide_source(&deck, &SlideKey::new("t").unwrap(), body, body).unwrap();

        assert_eq!(key.as_str(), "t");
        assert_eq!(saved_body, body);
        assert_eq!(
            fs::read_to_string(&deck).unwrap(),
            "# T\n\n- item\n- next\n\n<!-- note -->\n"
        );
    }

    #[test]
    fn write_preview_slide_source_maps_slide_body_refusal_to_unprocessable() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let source = "<!-- {\"key\":\"bare-cr\"} -->\n# Bare CR\n\nBody\rTail\n";
        fs::write(&deck, source).unwrap();

        let err = write_preview_slide_source(
            &deck,
            &SlideKey::new("bare-cr").unwrap(),
            "# Bare CR\n\nBody\rTail",
            "# Revised",
        )
        .unwrap_err();

        let server::DeckWriteError::Unprocessable(message) = err else {
            panic!("slide body refusal must be unprocessable: {err:?}");
        };
        assert!(message.contains("bare CR line endings are not supported by preview editing"));
        assert!(message
            .contains("convert the deck to LF or CRLF line endings, then reload the preview"));
        assert_eq!(fs::read_to_string(&deck).unwrap(), source);
    }

    #[test]
    fn write_preview_slide_source_maps_structural_refusal_to_unprocessable() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let source = "<!-- {\"key\":\"structural\"} -->\n# Structural\n\nBody\n";
        fs::write(&deck, source).unwrap();

        let err = write_preview_slide_source(
            &deck,
            &SlideKey::new("structural").unwrap(),
            "# Structural\n\nBody",
            "# Structural\n\n---\n\n# Extra",
        )
        .unwrap_err();

        let server::DeckWriteError::Unprocessable(message) = err else {
            panic!("structural refusal must be unprocessable: {err:?}");
        };
        assert!(message.contains("slide body edit would change the deck's slide count"));
        assert!(message.contains(
            "remove any `---` separator, close any unclosed code fence, or keep some body content when the slide has no settings comment or note, then retry"
        ));
        assert_eq!(fs::read_to_string(&deck).unwrap(), source);
    }

    #[test]
    fn write_preview_slide_source_writes_parse_valid_layout_arity_violation() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("layouts");
        fs::create_dir(&layouts).unwrap();
        fs::write(
            layouts.join("strict.html"),
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="1"></slot></section>"#,
        )
        .unwrap();
        fs::write(
            &deck,
            "<!-- {\"key\":\"arity\",\"layout\":\"strict\"} -->\n# Arity\n\nOne\n",
        )
        .unwrap();
        let new_body = "# Arity\n\nOne\n\nTwo";

        let (key, body) = write_preview_slide_source(
            &deck,
            &SlideKey::new("arity").unwrap(),
            "# Arity\n\nOne",
            new_body,
        )
        .unwrap();

        assert_eq!(key.as_str(), "arity");
        assert_eq!(body, new_body);
        assert_eq!(
            fs::read_to_string(&deck).unwrap(),
            "<!-- {\"key\":\"arity\",\"layout\":\"strict\"} -->\n# Arity\n\nOne\n\nTwo\n"
        );
        let build_error = match build_artifacts(&deck) {
            Ok(_) => panic!("the saved body must still fail the layout arity check"),
            Err(err) => err,
        };
        assert!(plain_diagnostic_text(&build_error)
            .contains("slot 'body' got 2 item(s), but layout 'strict' allows 1"));
    }

    #[test]
    fn write_preview_slide_source_writes_a_skipped_slide() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            &deck,
            "<!-- {\"key\":\"skipped\",\"skip\":true} -->\n# Old\n",
        )
        .unwrap();
        let new_body = "# Revised\n\nBody";

        let (key, body) = write_preview_slide_source(
            &deck,
            &SlideKey::new("skipped").unwrap(),
            "# Old",
            new_body,
        )
        .unwrap();

        assert_eq!(key.as_str(), "skipped");
        assert_eq!(body, new_body);
        assert_eq!(
            fs::read_to_string(&deck).unwrap(),
            "<!-- {\"key\":\"skipped\",\"skip\":true} -->\n# Revised\n\nBody\n"
        );
    }

    #[test]
    fn write_preview_slide_source_returns_the_accepting_reparse_identity() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Old heading\n").unwrap();

        let (key, body) = write_preview_slide_source(
            &deck,
            &SlideKey::new("old-heading").unwrap(),
            "# Old heading",
            "\n# New heading\r\n\r\nBody\n\n",
        )
        .unwrap();

        assert_eq!(key.as_str(), "new-heading");
        assert_eq!(body, "# New heading\n\nBody");
        assert_eq!(
            fs::read_to_string(&deck).unwrap(),
            "# New heading\n\nBody\n"
        );
    }

    #[test]
    fn write_preview_slide_source_preserves_a_leading_origin_bom() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, b"\xef\xbb\xbf<!-- {\"key\":\"bom\"} -->\n# Old\n").unwrap();

        let (key, body) =
            write_preview_slide_source(&deck, &SlideKey::new("bom").unwrap(), "# Old", "# New")
                .unwrap();

        assert_eq!(key.as_str(), "bom");
        assert_eq!(body, "# New");
        assert_eq!(
            fs::read(&deck).unwrap(),
            b"\xef\xbb\xbf<!-- {\"key\":\"bom\"} -->\n# New\n"
        );
    }

    #[test]
    fn write_preview_slide_source_preserves_pure_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            &deck,
            b"<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\nbefore\r\n",
        )
        .unwrap();

        let (key, body) = write_preview_slide_source(
            &deck,
            &SlideKey::new("crlf").unwrap(),
            "# CRLF\n\nbefore",
            "# CRLF\n\nafter",
        )
        .unwrap();

        assert_eq!(key.as_str(), "crlf");
        assert_eq!(body, "# CRLF\n\nafter");
        let bytes = fs::read(&deck).unwrap();
        assert_eq!(
            bytes,
            b"<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\nafter\r\n"
        );
        assert!(bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte != b'\n' || (index > 0 && bytes[index - 1] == b'\r')));
    }

    #[test]
    fn write_preview_slide_source_does_not_run_code_image_commands() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let sentinel = dir.path().join("code-image-command-ran");
        fs::write(
            &deck,
            format!(
                "---\ncode_images:\n  dot: sh -c 'touch {}'\n---\n<!-- {{\"key\":\"diagram\"}} -->\n# Diagram\n\nSafe paragraph\n\n```dot\ndigraph {{}}\n```\n",
                sentinel.display()
            ),
        )
        .unwrap();
        let old_body = "# Diagram\n\nSafe paragraph\n\n```dot\ndigraph {}\n```";
        let new_body = "# Diagram\n\nEdited safely\n\n```dot\ndigraph {}\n```";

        let (_, body) = write_preview_slide_source(
            &deck,
            &SlideKey::new("diagram").unwrap(),
            old_body,
            new_body,
        )
        .unwrap();

        assert_eq!(body, new_body);
        assert!(fs::read_to_string(&deck).unwrap().contains("Edited safely"));
        assert!(!sentinel.exists());
    }

    #[test]
    fn write_preview_slide_source_maps_origin_read_and_write_failures_to_io() {
        let read_dir = tempfile::tempdir().unwrap();
        let read_deck = read_dir.path().join("deck.md");
        fs::write(&read_deck, "<!-- {\"key\":\"vanished\"} -->\n# Before\n").unwrap();
        let loaded = load_and_expand_deck_source(&read_deck).unwrap();
        let (_, highlighter) =
            resolve_assets_and_highlighter(&read_deck, &loaded.frontmatter).unwrap();
        let parsed = loaded
            .translate(peitho_core::parse_deck(
                &loaded.source,
                loaded.frontmatter.clone(),
                &highlighter,
            ))
            .unwrap();
        let slide = &parsed.parsed_slides()[0];
        let rewritten = loaded
            .translate(peitho_core::slide_source::rewrite_slide_body(
                &loaded.source,
                slide,
                "# After",
                &highlighter,
            ))
            .unwrap();
        fs::remove_file(&read_deck).unwrap();

        let read_err = write_preview_origin_rewrite(
            &read_deck,
            &loaded,
            PreviewOriginRewriteScope::Slide(slide.source_span),
            &loaded.source,
            &rewritten.source,
        )
        .unwrap_err();

        let server::DeckWriteError::Io(read_message) = read_err else {
            panic!("origin read failure must be I/O: {read_err:?}");
        };
        assert!(read_message.contains("make the file readable and retry"));

        let write_dir = tempfile::tempdir().unwrap();
        let deck = write_dir.path().join("deck.md");
        let source = "<!-- {\"key\":\"unwritable\"} -->\n# Before\n";
        fs::write(&deck, source).unwrap();
        fs::create_dir(write_dir.path().join("deck.md.tmp")).unwrap();

        let err = write_preview_slide_source(
            &deck,
            &SlideKey::new("unwritable").unwrap(),
            "# Before",
            "# After",
        )
        .unwrap_err();

        let server::DeckWriteError::Io(message) = err else {
            panic!("origin write failure must be I/O: {err:?}");
        };
        assert!(message.contains("make the file and its directory writable and retry"));
        assert_eq!(fs::read_to_string(&deck).unwrap(), source);
    }

    #[test]
    fn write_preview_slide_source_writes_first_middle_and_last_included_slides() {
        let included_source = concat!(
            "# Included first\n\n---\n\n",
            "# Included middle\n\n---\n\n",
            "# Included last\n",
        );
        for (old_key, old_body, new_body, response_key) in [
            (
                "included-first",
                "# Included first",
                "# Revised first\n\n- added",
                "revised-first",
            ),
            (
                "included-middle",
                "# Included middle",
                "# Revised middle\n\n- added",
                "revised-middle",
            ),
            (
                "included-last",
                "# Included last",
                "# Revised last\n\n- added",
                "revised-last",
            ),
        ] {
            let (dir, deck, included, top_source) =
                include_deck_fixture(TOP_SOURCE, included_source);
            let key = SlideKey::new(old_key).unwrap();

            let (result_key, result_body) =
                write_preview_slide_source(&deck, &key, old_body, new_body).unwrap();

            assert_eq!(result_key.as_str(), response_key);
            assert_eq!(result_body, new_body);
            assert_eq!(fs::read_to_string(&deck).unwrap(), top_source);
            assert_eq!(
                fs::read_to_string(&included).unwrap(),
                included_source.replacen(old_body, new_body, 1),
            );
            drop(dir);
        }
    }

    #[test]
    fn write_preview_slide_source_writes_tight_separator_included_slides_exactly() {
        let included_source = "# Included first\n---\n# Included middle\n---\n# Included last\n";
        for (old_key, old_body, new_body, response_key, expected) in [
            (
                "included-first",
                "# Included first",
                "# Revised first\n\n- added",
                "revised-first",
                "# Revised first\n\n- added\n\n---\n# Included middle\n---\n# Included last\n",
            ),
            (
                "included-middle",
                "# Included middle",
                "# Revised middle\n\n- added",
                "revised-middle",
                "# Included first\n---\n# Revised middle\n\n- added\n\n---\n# Included last\n",
            ),
            (
                "included-last",
                "# Included last",
                "# Revised last\n\n- added",
                "revised-last",
                "# Included first\n---\n# Included middle\n---\n# Revised last\n\n- added\n",
            ),
        ] {
            let (dir, deck, included, top_source) =
                include_deck_fixture(TOP_SOURCE, included_source);
            let key = SlideKey::new(old_key).unwrap();

            let (result_key, result_body) =
                write_preview_slide_source(&deck, &key, old_body, new_body).unwrap();

            assert_eq!(result_key.as_str(), response_key);
            assert_eq!(result_body, new_body);
            assert_eq!(fs::read_to_string(&deck).unwrap(), top_source);
            assert_eq!(fs::read_to_string(&included).unwrap(), expected);
            drop(dir);
        }
    }

    #[test]
    fn write_preview_note_does_not_leak_a_clipped_include_tail() {
        let included_source = concat!(
            "# Included first\n\n---\n\n",
            "# Included middle\n\n---\n\n",
            "# Included last\n",
        );
        let (_dir, deck, included, top_source) = include_deck_fixture(TOP_SOURCE, included_source);
        let key = SlideKey::new("included-last").unwrap();

        for text in ["a", "b", "c"] {
            write_preview_note(&deck, &key, text).unwrap();

            assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
            assert_eq!(
                fs::read_to_string(&included).unwrap(),
                format!(
                    "# Included first\n\n---\n\n# Included middle\n\n---\n\n# Included last\n\n<!-- {text} -->\n"
                ),
            );
        }
    }

    #[test]
    fn write_preview_note_preserves_a_crlf_terminator_for_a_clipped_include_tail() {
        let (_dir, deck, included, top_source) =
            include_deck_fixture(TOP_SOURCE, "# Included last\r\n");
        let key = SlideKey::new("included-last").unwrap();

        for text in ["a", "b"] {
            write_preview_note(&deck, &key, text).unwrap();

            assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
            assert_eq!(
                fs::read(&included).unwrap(),
                format!("# Included last\r\n\r\n<!-- {text} -->\r\n").as_bytes(),
            );
        }
    }

    #[test]
    fn write_preview_note_writes_unterminated_includes_across_separator_and_line_endings() {
        for (name, top_source, included_source, content, line_ending) in [
            (
                "lf-tight",
                "<!-- {\"include\":\"included.md\"} -->\n---\n# Top\n",
                "# I1",
                "# I1",
                "\n",
            ),
            (
                "lf-loose",
                "<!-- {\"include\":\"included.md\"} -->\n\n---\n\n# Top\n",
                "# I1",
                "# I1",
                "\n",
            ),
            (
                "crlf-tight",
                "<!-- {\"include\":\"included.md\"} -->\r\n---\r\n# Top\r\n",
                "# I1\r\nBody",
                "# I1\r\nBody",
                "\r\n",
            ),
            (
                "crlf-loose",
                "<!-- {\"include\":\"included.md\"} -->\r\n\r\n---\r\n\r\n# Top\r\n",
                "# I1\r\nBody",
                "# I1\r\nBody",
                "\r\n",
            ),
        ] {
            let (_dir, deck, included, top_source) =
                include_deck_fixture(top_source, included_source);
            let key = SlideKey::new("i1").unwrap();

            for text in ["n", "b"] {
                write_preview_note(&deck, &key, text)
                    .unwrap_or_else(|err| panic!("{name} note {text}: {err:?}"));
                assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes(), "{name}");
                assert_eq!(
                    fs::read_to_string(&included).unwrap(),
                    format!("{content}{line_ending}{line_ending}<!-- {text} -->{line_ending}"),
                    "{name}",
                );
            }

            write_preview_note(&deck, &key, "")
                .unwrap_or_else(|err| panic!("{name} note deletion: {err:?}"));
            assert_eq!(
                fs::read_to_string(&included).unwrap(),
                format!("{content}{line_ending}"),
                "{name}",
            );

            write_preview_note(&deck, &key, "c")
                .unwrap_or_else(|err| panic!("{name} note c: {err:?}"));
            assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes(), "{name}");
            assert_eq!(
                fs::read_to_string(&included).unwrap(),
                format!("{content}{line_ending}{line_ending}<!-- c -->{line_ending}"),
                "{name}",
            );
        }
    }

    #[test]
    fn write_preview_slide_source_then_note_writes_an_unterminated_include() {
        let top_source = "<!-- {\"include\":\"included.md\"} -->\n---\n# Top\n";
        let (_dir, deck, included, top_source) = include_deck_fixture(top_source, "# I1");

        let (key, body) = write_preview_slide_source(
            &deck,
            &SlideKey::new("i1").unwrap(),
            "# I1",
            "# I2\n\nBody",
        )
        .unwrap();

        assert_eq!(key.as_str(), "i2");
        assert_eq!(body, "# I2\n\nBody");
        assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
        assert_eq!(fs::read(&included).unwrap(), b"# I2\n\nBody\n");

        write_preview_note(&deck, &key, "n").unwrap();
        assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
        assert_eq!(
            fs::read(&included).unwrap(),
            b"# I2\n\nBody\n\n<!-- n -->\n"
        );
    }

    #[test]
    fn write_preview_note_preserves_trailing_spaces_on_the_last_content_line() {
        let included_source = "# I1\n\ntext  \n\n<!-- n -->\n";
        let (_dir, deck, included, top_source) = include_deck_fixture(TOP_SOURCE, included_source);

        write_preview_note(&deck, &SlideKey::new("i1").unwrap(), "").unwrap();

        assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
        assert_eq!(fs::read(&included).unwrap(), b"# I1\n\ntext  \n");
    }

    #[test]
    fn write_preview_note_preserves_an_ideographic_space_last_content_line_when_tail_is_clipped() {
        let included_source = "# I1\n\ntext\n\n\u{3000}\n";
        let (_dir, deck, included, top_source) = include_deck_fixture(TOP_SOURCE, included_source);

        write_preview_note(&deck, &SlideKey::new("i1").unwrap(), "n").unwrap();

        assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
        assert_eq!(
            fs::read(&included).unwrap(),
            "# I1\n\ntext\n\n\u{3000}\n\n<!-- n -->\n".as_bytes()
        );
    }

    #[test]
    fn preview_slide_and_note_saves_do_not_leak_a_single_slide_include_tail() {
        {
            let (_dir, deck, included, top_source) = include_deck_fixture(TOP_SOURCE, "# Only\n");
            let key = SlideKey::new("only").unwrap();

            let (key, body) =
                write_preview_slide_source(&deck, &key, "# Only", "# Once\n\nBody").unwrap();
            assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
            assert_eq!(fs::read(&included).unwrap(), b"# Once\n\nBody\n");

            write_preview_slide_source(&deck, &key, &body, "# Twice\n\nBody").unwrap();
            assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
            assert_eq!(fs::read(&included).unwrap(), b"# Twice\n\nBody\n");
        }

        {
            let (_dir, deck, included, top_source) = include_deck_fixture(TOP_SOURCE, "# Only\n");
            let key = SlideKey::new("only").unwrap();

            for text in ["a", "b"] {
                write_preview_note(&deck, &key, text).unwrap();

                assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
                assert_eq!(
                    fs::read_to_string(&included).unwrap(),
                    format!("# Only\n\n<!-- {text} -->\n"),
                );
            }
        }
    }

    #[test]
    fn write_preview_origin_rewrite_maps_a_non_newline_synthetic_byte_to_scope_conflict() {
        let included_source = "# Included last\n";
        let (_dir, deck, included, top_source) = include_deck_fixture(TOP_SOURCE, included_source);
        let loaded = load_and_expand_deck_source(&deck).unwrap();
        let (_, highlighter) = resolve_assets_and_highlighter(&deck, &loaded.frontmatter).unwrap();
        let parsed = loaded
            .translate(peitho_core::parse_deck(
                &loaded.source,
                loaded.frontmatter.clone(),
                &highlighter,
            ))
            .unwrap();
        let slide = parsed
            .parsed_slides()
            .iter()
            .find(|slide| slide.key.as_str() == "included-last")
            .unwrap();
        let requested = slide.source_span;
        let translated = loaded
            .line_map
            .translate_span(&loaded.source, requested)
            .unwrap();
        assert!(translated.combined.end < requested.end);
        let clipped_tail = &loaded.source[translated.combined.end..requested.end];
        assert!(clipped_tail.bytes().all(|byte| byte == b'\n'));
        let mut before = loaded.source.clone();
        before.replace_range(translated.combined.end..translated.combined.end + 1, "x");
        assert!(loaded.line_map.translate_span(&before, requested).is_none());
        let mut after = before.clone();
        let heading = after.find("# Included last").unwrap();
        after.replace_range(heading + 2..heading + "# Included".len(), "Replaced");

        let err = write_preview_origin_rewrite(
            &deck,
            &loaded,
            PreviewOriginRewriteScope::Slide(requested),
            &before,
            &after,
        )
        .unwrap_err();

        let server::DeckWriteError::Conflict(message) = err else {
            panic!("an untranslatable slide scope must be a conflict: {err:?}");
        };
        assert!(message.contains("this slide cannot be edited from preview"));
        assert!(message.contains("included.md"));
        assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
        assert_eq!(fs::read_to_string(&included).unwrap(), included_source);
    }

    #[test]
    fn write_preview_origin_rewrite_rejects_a_changed_clipped_slide_head() {
        let top_source = concat!(
            "---\n",
            "time: 1m\n",
            "---\n",
            "<!-- {\"include\":\"included.md\"} -->\n\n",
            "---\n\n",
            "# Top\n",
        );
        let included_source = "# Included first\n";
        let (_dir, deck, included, top_source) = include_deck_fixture(top_source, included_source);
        let loaded = load_and_expand_deck_source(&deck).unwrap();
        let (_, highlighter) = resolve_assets_and_highlighter(&deck, &loaded.frontmatter).unwrap();
        let parsed = loaded
            .translate(peitho_core::parse_deck(
                &loaded.source,
                loaded.frontmatter.clone(),
                &highlighter,
            ))
            .unwrap();
        let slide = parsed
            .parsed_slides()
            .iter()
            .find(|slide| slide.key.as_str() == "included-first")
            .unwrap();
        let requested = slide.source_span;
        let translated = loaded
            .line_map
            .translate_span(&loaded.source, requested)
            .unwrap();
        assert!(translated.combined.start > requested.start);
        assert_eq!(
            &loaded.source[requested.start..translated.combined.start],
            "\n"
        );
        let before = loaded.source.clone();
        let mut after = before.clone();
        after.replace_range(requested.start..translated.combined.start, "x");
        assert_eq!(before.get(..requested.start), after.get(..requested.start));
        assert_eq!(before.get(requested.end..), after.get(requested.end..));

        let err = write_preview_origin_rewrite(
            &deck,
            &loaded,
            PreviewOriginRewriteScope::Slide(requested),
            &before,
            &after,
        )
        .unwrap_err();

        let server::DeckWriteError::Conflict(message) = err else {
            panic!("a changed clipped slide head must be a conflict: {err:?}");
        };
        assert!(message.contains("this slide cannot be edited from preview"));
        assert!(message.contains("included.md"));
        assert_eq!(fs::read(&deck).unwrap(), top_source.as_bytes());
        assert_eq!(fs::read(&included).unwrap(), included_source.as_bytes());
    }

    #[test]
    fn write_preview_origin_rewrite_rejects_a_mixed_origin_slide_scope() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared.md");
        fs::write(
            &deck,
            "# Before\n\n---\n<!-- {\"include\":\"shared.md\"} -->\n---\n# After\n",
        )
        .unwrap();
        fs::write(&included, "# Included").unwrap();
        let loaded = load_and_expand_deck_source(&deck).unwrap();
        let before = loaded.source.clone();
        let included_start = before.find("# Included").unwrap();
        let after_start = before.find("# After").unwrap();
        let mut after = before.clone();
        after.replace_range(
            included_start + "# ".len()..included_start + "# Included".len(),
            "Revised",
        );

        let err = write_preview_origin_rewrite(
            &deck,
            &loaded,
            PreviewOriginRewriteScope::Slide(peitho_core::domain::SourceSpan {
                start: included_start,
                end: after_start + "# After".len(),
            }),
            &before,
            &after,
        )
        .unwrap_err();

        let server::DeckWriteError::Conflict(message) = err else {
            panic!("mixed-origin slide must be a conflict: {err:?}");
        };
        assert!(message.contains("this slide cannot be edited from preview"));
        assert!(message.contains("edit the slide in"));
        assert!(message.contains("shared.md"));
        assert_eq!(fs::read_to_string(&included).unwrap(), "# Included");
    }

    #[test]
    fn write_preview_slide_edit_reparses_and_rewrites_the_current_block() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let key = SlideKey::new("current").unwrap();
        let initial = concat!(
            "<!-- {\"key\":\"current\"} -->\n",
            "# Current\n\n",
            "Edit *this*.\n\n",
            "Other old\n",
        );
        fs::write(&deck, initial).unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "Edit *this*.", 0);
        let current = initial.replace("Other old", "Other new");
        fs::write(&deck, &current).unwrap();

        write_preview_slide_edit(
            &deck,
            &key,
            start,
            end,
            "Edit *this*.",
            "Edit **this now**.",
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(&deck).unwrap(),
            current.replace("Edit *this*.", "Edit **this now**.")
        );
    }

    #[test]
    fn write_preview_slide_edit_rejects_missing_key_span_and_old_bytes_as_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let key = SlideKey::new("current").unwrap();
        fs::write(
            &deck,
            "<!-- {\"key\":\"current\"} -->\n# Current\n\nOriginal text\n",
        )
        .unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "Original text", 0);

        assert_preview_deck_drift(write_preview_slide_edit(
            &deck,
            &SlideKey::new("missing").unwrap(),
            start,
            end,
            "Original text",
            "Replacement",
        ));
        assert_preview_deck_drift(write_preview_slide_edit(
            &deck,
            &key,
            start + 1,
            end,
            "Original text",
            "Replacement",
        ));
        assert_preview_deck_drift(write_preview_slide_edit(
            &deck,
            &key,
            start,
            end,
            "stale text",
            "Replacement",
        ));
        assert_eq!(
            fs::read_to_string(&deck).unwrap(),
            "<!-- {\"key\":\"current\"} -->\n# Current\n\nOriginal text\n"
        );
    }

    #[test]
    fn write_preview_slide_edit_uses_range_when_the_same_old_text_occurs_twice() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let key = SlideKey::new("repeat").unwrap();
        fs::write(&deck, "# Repeat\n\nsame old\n\nsame old\n").unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "same old", 1);

        write_preview_slide_edit(&deck, &key, start, end, "same old", "second changed").unwrap();

        assert_eq!(
            fs::read_to_string(&deck).unwrap(),
            "# Repeat\n\nsame old\n\nsecond changed\n"
        );
    }

    #[test]
    fn write_preview_slide_edit_maps_structural_refusal_to_unprocessable() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let key = SlideKey::new("structural").unwrap();
        fs::write(&deck, "# Structural\n\nplain text\n").unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "plain text", 0);

        let err = write_preview_slide_edit(&deck, &key, start, end, "plain text", "- list item")
            .unwrap_err();

        let server::DeckWriteError::Unprocessable(message) = err else {
            panic!("structural refusal must be unprocessable: {err:?}");
        };
        assert!(message.contains("inline edit would change the edited slide's block structure"));
    }

    #[test]
    fn write_preview_slide_edit_writes_only_the_included_origin_block() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared.md");
        let top_source = concat!(
            "---\n",
            "time: 1m\n",
            "---\n",
            "<!-- {\"include\":\"shared.md\"} -->\n",
            "---\n",
            "# Top\n",
        );
        fs::write(&deck, top_source).unwrap();
        fs::write(
            &included,
            "<!-- {\"key\":\"shared\"} -->\n# Shared\n\n共有 **本文**\n",
        )
        .unwrap();
        let key = SlideKey::new("shared").unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "共有 **本文**", 0);

        write_preview_slide_edit(&deck, &key, start, end, "共有 **本文**", "共有 **更新**")
            .unwrap();

        assert_eq!(fs::read_to_string(&deck).unwrap(), top_source);
        assert_eq!(
            fs::read_to_string(&included).unwrap(),
            "<!-- {\"key\":\"shared\"} -->\n# Shared\n\n共有 **更新**\n"
        );
    }

    #[test]
    fn write_preview_slide_edit_rejects_a_synthetic_or_mixed_origin_block_span() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared.md");
        fs::write(
            &deck,
            "# Before\n\n---\n<!-- {\"include\":\"shared.md\"} -->\n---\n# After\n",
        )
        .unwrap();
        fs::write(&included, "# Included").unwrap();
        let loaded = load_and_expand_deck_source(&deck).unwrap();
        let before = loaded.source.clone();
        let included_start = before.find("# Included").unwrap();
        let synthetic_end = included_start + "# Included\n".len();
        let mut clipped_after = before.clone();
        clipped_after.replace_range(
            included_start + "# ".len()..included_start + "# Included".len(),
            "Replaced",
        );

        let clipped = write_preview_origin_rewrite(
            &deck,
            &loaded,
            PreviewOriginRewriteScope::EditableBlock(peitho_core::domain::SourceSpan {
                start: included_start,
                end: synthetic_end,
            }),
            &before,
            &clipped_after,
        )
        .unwrap_err();
        let server::DeckWriteError::Conflict(clipped_message) = clipped else {
            panic!("synthetic clipping must be a conflict: {clipped:?}");
        };
        assert!(clipped_message.contains("shared.md"));

        let after_start = before.find("# After").unwrap();
        let mixed = write_preview_origin_rewrite(
            &deck,
            &loaded,
            PreviewOriginRewriteScope::EditableBlock(peitho_core::domain::SourceSpan {
                start: included_start,
                end: after_start + "# After".len(),
            }),
            &before,
            &clipped_after,
        )
        .unwrap_err();
        let server::DeckWriteError::Conflict(mixed_message) = mixed else {
            panic!("mixed origin span must be a conflict: {mixed:?}");
        };
        assert!(mixed_message.contains("shared.md"));
        assert_eq!(fs::read_to_string(&included).unwrap(), "# Included");
    }

    #[test]
    fn write_preview_slide_edit_preserves_a_leading_origin_bom() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let key = SlideKey::new("bom").unwrap();
        fs::write(
            &deck,
            b"\xef\xbb\xbf<!-- {\"key\":\"bom\"} -->\n# BOM\n\nbefore\n",
        )
        .unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "before", 0);

        write_preview_slide_edit(&deck, &key, start, end, "before", "after").unwrap();

        assert_eq!(
            fs::read(&deck).unwrap(),
            b"\xef\xbb\xbf<!-- {\"key\":\"bom\"} -->\n# BOM\n\nafter\n"
        );
    }

    #[test]
    fn write_preview_slide_edit_preserves_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let key = SlideKey::new("crlf").unwrap();
        fs::write(
            &deck,
            b"<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\nbefore\r\n",
        )
        .unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "before", 0);

        write_preview_slide_edit(&deck, &key, start, end, "before", "after").unwrap();

        let bytes = fs::read(&deck).unwrap();
        assert_eq!(
            bytes,
            b"<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\nafter\r\n"
        );
        assert!(bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte != b'\n' || (index > 0 && bytes[index - 1] == b'\r')));
    }

    #[test]
    fn write_preview_slide_edit_preserves_crlf_for_a_multiline_block() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let key = SlideKey::new("crlf").unwrap();
        let original = b"<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\nfirst\r\nsecond\r\n";
        fs::write(&deck, original).unwrap();
        let old = "first\r\nsecond";
        let (start, end) = preview_edit_coordinates(&deck, &key, old, 0);
        fs::create_dir(dir.path().join("deck.md.tmp")).unwrap();

        write_preview_slide_edit(&deck, &key, start, end, old, "first\nsecond").unwrap();

        assert_eq!(fs::read(&deck).unwrap(), original);
        fs::remove_dir(dir.path().join("deck.md.tmp")).unwrap();

        write_preview_slide_edit(&deck, &key, start, end, old, "first changed\nsecond").unwrap();

        let bytes = fs::read(&deck).unwrap();
        assert_eq!(
            bytes,
            b"<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\nfirst changed\r\nsecond\r\n"
        );
        assert!(bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte != b'\n' || (index > 0 && bytes[index - 1] == b'\r')));
    }

    #[test]
    fn write_preview_slide_edit_does_not_run_code_image_commands() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let sentinel = dir.path().join("code-image-command-ran");
        fs::write(
            &deck,
            format!(
                "---\ncode_images:\n  dot: sh -c 'touch {}'\n---\n<!-- {{\"key\":\"diagram\"}} -->\n# Diagram\n\nSafe paragraph\n\n```dot\ndigraph {{}}\n```\n",
                sentinel.display()
            ),
        )
        .unwrap();
        let key = SlideKey::new("diagram").unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "Safe paragraph", 0);

        write_preview_slide_edit(&deck, &key, start, end, "Safe paragraph", "Edited safely")
            .unwrap();

        assert!(fs::read_to_string(&deck).unwrap().contains("Edited safely"));
        assert!(!sentinel.exists());
    }

    #[test]
    fn preview_origin_writer_is_shared_by_all_preview_write_regressions() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let key = SlideKey::new("shared-seam").unwrap();
        fs::write(
            &deck,
            concat!(
                "<!-- {\"key\":\"shared-seam\"} -->\n",
                "# Shared seam\n\n",
                "before\n\n",
                "<!-- old note -->\n",
            ),
        )
        .unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "before", 0);

        write_preview_note(&deck, &key, "new note").unwrap();
        write_preview_slide_edit(&deck, &key, start, end, "before", "after").unwrap();
        let (result_key, result_body) = write_preview_slide_source(
            &deck,
            &key,
            "# Shared seam\n\nafter",
            "# Shared seam\n\nafter\n\n- added",
        )
        .unwrap();

        assert_eq!(result_key, key);
        assert_eq!(result_body, "# Shared seam\n\nafter\n\n- added");
        assert_eq!(
            fs::read_to_string(&deck).unwrap(),
            concat!(
                "<!-- {\"key\":\"shared-seam\"} -->\n",
                "# Shared seam\n\n",
                "after\n\n- added\n\n",
                "<!-- new note -->\n",
            )
        );

        let source =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        let atomic_needle = ["write_atomic", "(origin_path"].concat();
        assert_eq!(production.matches(&atomic_needle).count(), 1);
        let shared_needle = ["write_preview_origin", "_rewrite("].concat();
        assert_eq!(production.matches(&shared_needle).count(), 4);
    }

    #[test]
    fn preview_deck_writer_dispatches_note_and_slide_edit_to_the_same_origin_seam() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared.md");
        let top_source = concat!(
            "---\n",
            "time: 1m\n",
            "---\n",
            "<!-- {\"include\":\"shared.md\"} -->\n",
            "---\n",
            "# Top\n",
        );
        let key = SlideKey::new("shared-seam").unwrap();
        fs::write(&deck, top_source).unwrap();
        fs::write(
            &included,
            concat!(
                "<!-- {\"key\":\"shared-seam\"} -->\n",
                "# Shared seam\n\n",
                "before\n\n",
                "<!-- old note -->\n",
            ),
        )
        .unwrap();
        let (start, end) = preview_edit_coordinates(&deck, &key, "before", 0);
        let mut writer = PreviewDeckWriter {
            input: deck.clone(),
        };

        server::DeckWriter::note(&mut writer, key.clone(), "new note".to_owned()).unwrap();
        server::DeckWriter::slide_edit(
            &mut writer,
            server::SlideEditWrite {
                key,
                start,
                end,
                old: "before".to_owned(),
                new: "after".to_owned(),
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(&included).unwrap(),
            concat!(
                "<!-- {\"key\":\"shared-seam\"} -->\n",
                "# Shared seam\n\n",
                "after\n\n",
                "<!-- new note -->\n",
            )
        );
        assert_eq!(fs::read_to_string(&deck).unwrap(), top_source);
    }

    #[test]
    fn preview_deck_writer_serves_slide_source_route_end_to_end() {
        use std::net::{Shutdown, TcpStream};

        fn post(server: &server::PresentServer, body: &str) -> (u16, String) {
            let addr = server.addr();
            let server_for_request = server.clone();
            let handle = thread::spawn(move || server_for_request.handle_one());
            let mut stream = TcpStream::connect(addr).unwrap();
            let request = format!(
                "POST /slide-source HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            );
            stream.write_all(request.as_bytes()).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();

            let mut raw = String::new();
            stream.read_to_string(&mut raw).unwrap();
            handle.join().unwrap();
            let (head, body) = raw.split_once("\r\n\r\n").unwrap();
            let status = head
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap();
            (status, body.to_owned())
        }

        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            &deck,
            concat!(
                "<!-- {\"key\":\"editable\"} -->\n",
                "\n",
                "# Old\n\n",
                "Body\n\n",
                "<!-- note -->\n",
            ),
        )
        .unwrap();
        let server = server::PresentServer::bind(PathBuf::new(), 0, "present.html")
            .unwrap()
            .with_deck_writer(PreviewDeckWriter {
                input: deck.clone(),
            });

        let saved = post(
            &server,
            r##"{"key":"editable","old":"# Old\n\nBody","new":"# New\n\nBody with a list\n\n- one"}"##,
        );
        assert_eq!(saved.0, 200);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&saved.1).unwrap(),
            serde_json::json!({
                "key": "editable",
                "body": "# New\n\nBody with a list\n\n- one",
            }),
        );
        let rewritten = concat!(
            "<!-- {\"key\":\"editable\"} -->\n",
            "\n",
            "# New\n\n",
            "Body with a list\n\n",
            "- one\n\n",
            "<!-- note -->\n",
        );
        assert_eq!(fs::read_to_string(&deck).unwrap(), rewritten);

        let stale = post(
            &server,
            r##"{"key":"editable","old":"# Old\n\nBody","new":"# Stale"}"##,
        );
        assert_eq!(stale.0, 409);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stale.1).unwrap(),
            serde_json::json!({"error": "the deck changed on disk; reload and retry"}),
        );
        assert_eq!(fs::read_to_string(&deck).unwrap(), rewritten);

        let structural = post(
            &server,
            r##"{"key":"editable","old":"# New\n\nBody with a list\n\n- one","new":"---"}"##,
        );
        let structural_json: serde_json::Value = serde_json::from_str(&structural.1).unwrap();
        assert_eq!(structural.0, 422);
        assert!(structural_json["error"]
            .as_str()
            .unwrap()
            .contains("slide body edit would change the deck's slide count"));
        assert_eq!(fs::read_to_string(&deck).unwrap(), rewritten);
    }

    #[test]
    fn preview_deck_writer_reparses_and_rewrites_the_current_note() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            &deck,
            concat!(
                "<!-- {\"key\":\"one\"} -->\r\n",
                "# One\r\n\r\n",
                "<!-- first note -->\r\n",
                "---\n",
                "<!-- {\"key\":\"two\"} -->\n",
                "# Two before writer construction\n\n",
                "<!-- stale note -->\n",
            ),
        )
        .unwrap();
        let mut writer = PreviewDeckWriter {
            input: deck.clone(),
        };
        let current_source = concat!(
            "<!-- {\"key\":\"one\"} -->\r\n",
            "# One\r\n\r\n",
            "<!-- first note -->\r\n",
            "---\n",
            "<!-- {\"key\":\"two\"} -->\n",
            "# Two from the live file\n\n",
            "<!-- current note -->\n",
        );
        fs::write(&deck, current_source).unwrap();

        dispatch_preview_note(
            &mut writer,
            peitho_core::domain::SlideKey::new("two").unwrap(),
            "edited\nnote",
        )
        .unwrap();

        let expected = current_source.replace("<!-- current note -->", "<!--\nedited\nnote\n-->");
        assert_eq!(fs::read_to_string(&deck).unwrap(), expected);

        fs::create_dir(dir.path().join("deck.md.tmp")).unwrap();
        dispatch_preview_note(
            &mut writer,
            peitho_core::domain::SlideKey::new("two").unwrap(),
            "edited\nnote",
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&deck).unwrap(), expected);
    }

    #[test]
    fn preview_deck_writer_writes_the_note_include_origin() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared.md");
        let top_source = concat!(
            "---\n",
            "time: 1m\n",
            "---\n",
            "<!-- {\"include\":\"shared.md\"} -->\n",
            "---\n",
            "# Top\n",
        );
        fs::write(&deck, top_source).unwrap();
        fs::write(
            &included,
            concat!(
                "<!-- {\"key\":\"shared\"} -->\n",
                "# Shared\n\n",
                "<!-- original note -->",
            ),
        )
        .unwrap();
        let mut writer = PreviewDeckWriter {
            input: deck.clone(),
        };

        dispatch_preview_note(
            &mut writer,
            peitho_core::domain::SlideKey::new("shared").unwrap(),
            "edited in preview",
        )
        .unwrap();

        assert_eq!(fs::read_to_string(&deck).unwrap(), top_source);
        assert_eq!(
            fs::read_to_string(&included).unwrap(),
            concat!(
                "<!-- {\"key\":\"shared\"} -->\n",
                "# Shared\n\n",
                "<!-- edited in preview -->\n",
            )
        );

        dispatch_preview_note(
            &mut writer,
            peitho_core::domain::SlideKey::new("shared").unwrap(),
            "x",
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&deck).unwrap(), top_source);
        assert_eq!(
            fs::read_to_string(&included).unwrap(),
            concat!(
                "<!-- {\"key\":\"shared\"} -->\n",
                "# Shared\n\n",
                "<!-- x -->\n",
            )
        );
    }

    #[test]
    fn preview_deck_writer_preserves_a_leading_bom_for_notes() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            &deck,
            b"\xef\xbb\xbf<!-- {\"key\":\"bom\"} -->\n# BOM\n\n<!-- old note -->\n",
        )
        .unwrap();
        let mut writer = PreviewDeckWriter {
            input: deck.clone(),
        };

        dispatch_preview_note(
            &mut writer,
            peitho_core::domain::SlideKey::new("bom").unwrap(),
            "new note",
        )
        .unwrap();

        let bytes = fs::read(&deck).unwrap();
        assert!(bytes.starts_with(b"\xef\xbb\xbf"));
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "\u{feff}<!-- {\"key\":\"bom\"} -->\n# BOM\n\n<!-- new note -->\n"
        );
    }

    #[test]
    fn preview_deck_writer_note_does_not_run_code_image_commands() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let sentinel = dir.path().join("code-image-command-ran");
        fs::write(
            &deck,
            format!(
                "---\ncode_images:\n  dot: sh -c 'touch {}'\n---\n<!-- {{\"key\":\"diagram\"}} -->\n# Diagram\n\n```dot\ndigraph {{}}\n```\n\n<!-- old note -->\n",
                sentinel.display()
            ),
        )
        .unwrap();
        let mut writer = PreviewDeckWriter {
            input: deck.clone(),
        };

        dispatch_preview_note(
            &mut writer,
            peitho_core::domain::SlideKey::new("diagram").unwrap(),
            "safe note",
        )
        .unwrap();

        assert!(fs::read_to_string(&deck)
            .unwrap()
            .contains("<!-- safe note -->"));
        assert!(!sentinel.exists());
    }

    #[test]
    fn preview_deck_writer_note_uses_build_highlighter_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let syntaxes = dir.path().join("syntaxes");
        fs::create_dir_all(&syntaxes).unwrap();
        fs::write(
            syntaxes.join("carina.sublime-syntax"),
            CARINA_SUBLIME_SYNTAX,
        )
        .unwrap();
        fs::write(
            &deck,
            concat!(
                "<!-- {\"key\":\"infra\"} -->\n",
                "# Infra\n\n",
                "```carina\n",
                "resource \"aws_s3_bucket\" \"site\" {}\n",
                "```\n\n",
                "<!-- old note -->\n",
            ),
        )
        .unwrap();
        let mut writer = PreviewDeckWriter {
            input: deck.clone(),
        };

        dispatch_preview_note(
            &mut writer,
            peitho_core::domain::SlideKey::new("infra").unwrap(),
            "syntax-aware note",
        )
        .unwrap();

        assert!(fs::read_to_string(&deck)
            .unwrap()
            .contains("<!-- syntax-aware note -->"));
    }

    #[test]
    fn preview_deck_writer_atomic_save_rebuilds_once_after_settling() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let rebuilds = Cell::new(0);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuild = |_: &mut dyn Write, _: &mut dyn Write| {
            rebuilds.set(rebuilds.get() + 1);
            Ok(())
        };

        handle_watch_tick(&mut state, &mut stdout, &mut stderr, &mut rebuild).unwrap();

        assert_eq!(rebuilds.get(), 0);

        server::write_atomic(&fixture.options.input, b"# Changed\n").unwrap();
        run_watch_ticks(&mut state, 2, &mut stdout, &mut stderr, &mut rebuild);

        assert_eq!(rebuilds.get(), 1);
    }

    #[test]
    fn preview_deck_writer_note_classifies_conflict_unprocessable_and_io() {
        let broken_dir = tempfile::tempdir().unwrap();
        let broken_deck = broken_dir.path().join("deck.md");
        fs::write(&broken_deck, "---\ntime: [\n---\n# Broken\n").unwrap();
        let mut broken_writer = PreviewDeckWriter { input: broken_deck };
        assert!(matches!(
            dispatch_preview_note(
                &mut broken_writer,
                peitho_core::domain::SlideKey::new("broken").unwrap(),
                "note",
            ),
            Err(server::DeckWriteError::Conflict(_))
        ));

        let vanished_dir = tempfile::tempdir().unwrap();
        let vanished_deck = vanished_dir.path().join("deck.md");
        fs::write(
            &vanished_deck,
            "<!-- {\"key\":\"vanished\"} -->\n# Before\n",
        )
        .unwrap();
        let mut vanished_writer = PreviewDeckWriter {
            input: vanished_deck.clone(),
        };
        fs::write(
            &vanished_deck,
            "<!-- {\"key\":\"replacement\"} -->\n# After\n",
        )
        .unwrap();
        let missing = dispatch_preview_note(
            &mut vanished_writer,
            peitho_core::domain::SlideKey::new("vanished").unwrap(),
            "note",
        )
        .unwrap_err();
        let server::DeckWriteError::Conflict(message) = missing else {
            panic!("missing slide key must be a conflict: {missing:?}");
        };
        assert!(message.contains("slide key 'vanished' not found in current deck"));
        assert!(message.contains("reload preview and retry on a slide whose key still exists"));

        let invalid_dir = tempfile::tempdir().unwrap();
        let invalid_deck = invalid_dir.path().join("deck.md");
        fs::write(
            &invalid_deck,
            "<!-- {\"key\":\"invalid\"} -->\n# Invalid\n\n<!-- old note -->\n",
        )
        .unwrap();
        let mut invalid_writer = PreviewDeckWriter {
            input: invalid_deck,
        };
        assert!(matches!(
            dispatch_preview_note(
                &mut invalid_writer,
                peitho_core::domain::SlideKey::new("invalid").unwrap(),
                "cannot --> save",
            ),
            Err(server::DeckWriteError::Unprocessable(_))
        ));
        assert!(matches!(
            dispatch_preview_note(
                &mut invalid_writer,
                peitho_core::domain::SlideKey::new("invalid").unwrap(),
                "{looks like settings}",
            ),
            Err(server::DeckWriteError::Unprocessable(_))
        ));

        let deleted_dir = tempfile::tempdir().unwrap();
        let deleted_deck = deleted_dir.path().join("deck.md");
        fs::write(&deleted_deck, "# Deleted\n").unwrap();
        let mut deleted_writer = PreviewDeckWriter {
            input: deleted_deck.clone(),
        };
        fs::remove_file(&deleted_deck).unwrap();
        assert!(matches!(
            dispatch_preview_note(
                &mut deleted_writer,
                peitho_core::domain::SlideKey::new("deleted").unwrap(),
                "note",
            ),
            Err(server::DeckWriteError::Io(_))
        ));

        let unwritable_dir = tempfile::tempdir().unwrap();
        let unwritable_deck = unwritable_dir.path().join("deck.md");
        fs::write(
            &unwritable_deck,
            "<!-- {\"key\":\"unwritable\"} -->\n# Unwritable\n\n<!-- old note -->\n",
        )
        .unwrap();
        fs::create_dir(unwritable_dir.path().join("deck.md.tmp")).unwrap();
        let mut unwritable_writer = PreviewDeckWriter {
            input: unwritable_deck,
        };
        let write_error = dispatch_preview_note(
            &mut unwritable_writer,
            peitho_core::domain::SlideKey::new("unwritable").unwrap(),
            "new note",
        )
        .unwrap_err();
        let server::DeckWriteError::Io(message) = write_error else {
            panic!("origin write failure must be I/O: {write_error:?}");
        };
        assert!(message.contains("make the file and its directory writable and retry"));
    }

    #[test]
    fn preview_deck_writer_preserves_crlf_note_origin() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            &deck,
            b"<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\n<!-- old note -->\r\n",
        )
        .unwrap();
        let mut writer = PreviewDeckWriter {
            input: deck.clone(),
        };

        dispatch_preview_note(
            &mut writer,
            peitho_core::domain::SlideKey::new("crlf").unwrap(),
            "line one\nline two",
        )
        .unwrap();

        assert_eq!(
            fs::read(&deck).unwrap(),
            b"<!-- {\"key\":\"crlf\"} -->\r\n# CRLF\r\n\r\n<!--\r\nline one\r\nline two\r\n-->\r\n"
        );

        let mixed_deck = dir.path().join("mixed.md");
        let mixed_source = "<!-- {\"key\":\"a\"} -->\r\n# A\n\n<!-- old -->\n---\n# B\n";
        fs::write(&mixed_deck, mixed_source).unwrap();
        let mut mixed_writer = PreviewDeckWriter {
            input: mixed_deck.clone(),
        };

        dispatch_preview_note(
            &mut mixed_writer,
            peitho_core::domain::SlideKey::new("a").unwrap(),
            "new",
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(&mixed_deck).unwrap(),
            mixed_source.replace("<!-- old -->", "<!-- new -->")
        );

        let unterminated_deck = dir.path().join("unterminated.md");
        fs::write(
            &unterminated_deck,
            b"<!-- {\"key\":\"a\"} -->\r\n# A\r\n---\r\n# B",
        )
        .unwrap();
        let mut unterminated_writer = PreviewDeckWriter {
            input: unterminated_deck.clone(),
        };

        dispatch_preview_note(
            &mut unterminated_writer,
            peitho_core::domain::SlideKey::new("b").unwrap(),
            "m1\nm2",
        )
        .unwrap();

        let bytes = fs::read(&unterminated_deck).unwrap();
        assert_eq!(
            bytes,
            b"<!-- {\"key\":\"a\"} -->\r\n# A\r\n---\r\n# B\r\n\r\n<!--\r\nm1\r\nm2\r\n-->\r\n"
        );
        assert!(bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| { *byte != b'\n' || (index > 0 && bytes[index - 1] == b'\r') }));
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
    fn no_overrides_leaves_css_loading_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let css = dir.path().join("css");
        let content = ".slot-title { color: rebeccapurple; }\n";
        fs::create_dir_all(&css).unwrap();
        fs::write(css.join("overrides.css"), content).unwrap();

        let custom_files = load_css(Some(&css), None).unwrap();
        let builtin_files = load_css(None, None).unwrap();

        assert_eq!(custom_files.len(), 1);
        assert_eq!(custom_files[0].name, "overrides.css");
        assert_eq!(custom_files[0].content, content);
        assert_eq!(builtin_files.len(), 1);
        assert_eq!(builtin_files[0].name, BUILTIN_CSS_FILE_NAME);
        assert_eq!(builtin_files[0].content, BUILTIN_BASE_CSS);
    }

    #[test]
    fn overrides_are_appended_after_the_builtin_theme() {
        let dir = tempfile::tempdir().unwrap();
        let overrides = dir.path().join("overrides");
        let override_content = ".slot-title { color: rebeccapurple; }\n";
        fs::create_dir_all(&overrides).unwrap();
        fs::write(overrides.join("talk.css"), override_content).unwrap();

        let files = load_css(None, Some(&overrides)).unwrap();

        assert_eq!(
            files
                .iter()
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>(),
            vec![BUILTIN_CSS_FILE_NAME, "talk.css"]
        );
        assert_eq!(files[0].content, BUILTIN_BASE_CSS);
        assert_eq!(files[1].content, override_content);
    }

    #[test]
    fn overrides_are_appended_after_deck_css() {
        let dir = tempfile::tempdir().unwrap();
        let css = dir.path().join("css");
        let overrides = dir.path().join("overrides");
        let deck_content = ".peitho-slide { color: navy; }\n";
        let override_content = ".slot-title { color: gold; }\n";
        fs::create_dir_all(&css).unwrap();
        fs::create_dir_all(&overrides).unwrap();
        fs::write(css.join("deck.css"), deck_content).unwrap();
        fs::write(overrides.join("talk.css"), override_content).unwrap();

        let files = load_css(Some(&css), Some(&overrides)).unwrap();

        assert_eq!(
            files
                .iter()
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>(),
            vec!["deck.css", "talk.css"]
        );
        assert_eq!(files[0].content, deck_content);
        assert_eq!(files[1].content, override_content);
    }

    #[test]
    fn overrides_directory_reads_multiple_files_in_filename_order() {
        let dir = tempfile::tempdir().unwrap();
        let overrides = dir.path().join("overrides");
        fs::create_dir_all(&overrides).unwrap();
        fs::write(
            overrides.join("z-last.css"),
            ".slot-title { color: tomato; }\n",
        )
        .unwrap();
        fs::write(
            overrides.join("a-first.css"),
            ".slot-title { color: navy; }\n",
        )
        .unwrap();

        let files = load_css(None, Some(&overrides)).unwrap();

        assert_eq!(
            files
                .iter()
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>(),
            vec![BUILTIN_CSS_FILE_NAME, "a-first.css", "z-last.css"]
        );
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
    fn write_shared_assets_writes_katex_fonts_for_math_without_touching_user_fonts() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let fonts = dir.path().join("fonts");
        let out = dir.path().join("dist-math-assets");
        fs::create_dir_all(&fonts).unwrap();
        fs::write(fonts.join("deck-font.woff2"), b"font bytes").unwrap();
        fs::write(
            &deck,
            "---\nfonts: ./fonts\n---\n# Math\n\n```math\n\\frac{1}{2}\n```\n",
        )
        .unwrap();
        let artifacts = build_artifacts(&deck).unwrap();

        write_shared_assets(&out, &artifacts).unwrap();

        assert_eq!(
            fs::read(out.join("fonts/deck-font.woff2")).unwrap(),
            b"font bytes"
        );
        assert_eq!(
            fs::read(out.join("katex-fonts/KaTeX_Main-Regular.woff2")).unwrap(),
            katex_font_bytes("KaTeX_Main-Regular.woff2")
        );
    }

    #[test]
    fn write_shared_assets_removes_stale_katex_fonts_when_deck_has_no_math() {
        let fixture = WatchFixture::new("# Plain\n\nBody\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let out = fixture._dir.path().join("dist");
        fs::create_dir_all(out.join("katex-fonts")).unwrap();
        fs::write(out.join("katex-fonts/stale.woff2"), b"stale").unwrap();

        write_shared_assets(&out, &artifacts).unwrap();

        assert!(!out.join("katex-fonts").exists());
    }

    #[test]
    fn write_shared_assets_writes_theme_fonts_for_custom_css() {
        let fixture = WatchFixture::new("# Intro\n\nBody\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let out = fixture._dir.path().join("dist");

        write_shared_assets(&out, &artifacts).unwrap();

        assert!(fs::read_to_string(out.join("peitho.css"))
            .unwrap()
            .contains(".slot-title { font-weight: 700; }"));
        assert_theme_fonts_written(&out);
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
        let help = err.help().expect("help must be present").to_string();
        assert!(
            help.contains("only regular files and subdirectories"),
            "actual help: {help}"
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
        let help = err.help().expect("help must be present").to_string();
        assert!(
            help.contains("point fonts: at a regular file or a directory"),
            "actual help: {help}"
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
    fn resolve_watch_targets_tracks_referenced_images_and_includes() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let shared = dir.path().join("shared");
        let included = shared.join("intro.md");
        let image = dir.path().join("img/a.png");
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        fs::create_dir_all(&shared).unwrap();
        fs::write(&included, "# Included\n\n![x](img/a.png)\n").unwrap();
        fs::write(&image, b"image").unwrap();
        fs::write(&deck, "<!-- {\"include\":\"shared/intro.md\"} -->\n").unwrap();

        let snapshot = capture_input_snapshot(&resolve_watch_targets(&deck).unwrap());

        assert!(matches!(
            snapshot.get(&included),
            Some(InputFingerprint::Content(_))
        ));
        assert!(matches!(
            snapshot.get(&image),
            Some(InputFingerprint::Metadata { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn watch_targets_do_not_render_or_track_generated_mermaid_images() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let layouts = dir.path().join("layouts");
        let css = dir.path().join("css");
        let command = dir.path().join("svg-command.sh");
        let marker = PathBuf::from(format!("{}.ran", command.display()));
        let cache = dir.path().join(peitho_core::CODE_IMAGES_CACHE_DIR);
        let out = dir.path().join("dist");
        fs::create_dir_all(&layouts).unwrap();
        fs::create_dir_all(&css).unwrap();
        fs::write(layouts.join("image.html"), TEST_IMAGE_LAYOUT_HTML).unwrap();
        fs::write(layouts.join("title-body-code.html"), TEST_LAYOUT_HTML).unwrap();
        fs::write(css.join("base.css"), "section {}\n").unwrap();
        write_script(
            &command,
            "#!/bin/sh\ncat >/dev/null\nprintf ran > \"$0.ran\"\nprintf '<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"></svg>'\n",
        );
        fs::write(
            &deck,
            format!(
                "---\ncode_images:\n  mermaid: /bin/sh {}\n---\n# Diagram\n\n```mermaid\ngraph TD\n  A --> B\n```\n",
                command.display()
            ),
        )
        .unwrap();

        let targets = resolve_watch_targets(&deck).unwrap();
        let before = capture_input_snapshot(&targets);

        assert!(!marker.exists(), "target resolution ran the renderer");
        assert!(!cache.exists(), "target resolution created the image cache");
        assert!(!before.keys().any(|path| path.starts_with(&cache)));

        let options = BuildOptions { input: deck, out };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        rebuild_once_for_watch(&options, &mut stdout, &mut stderr).unwrap();

        assert!(marker.exists(), "the full rebuild did not run the renderer");
        assert!(cache.is_dir());
        assert_eq!(capture_input_snapshot(&targets), before);
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    }

    #[test]
    fn broken_included_source_keeps_include_and_assets_tracked_until_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared/intro.md");
        let layout = dir.path().join("layouts/title-body-code.html");
        let stylesheet = dir.path().join("css/base.css");
        let out = dir.path().join("dist");
        fs::create_dir_all(included.parent().unwrap()).unwrap();
        fs::create_dir_all(layout.parent().unwrap()).unwrap();
        fs::create_dir_all(stylesheet.parent().unwrap()).unwrap();
        fs::write(&layout, TEST_LAYOUT_HTML).unwrap();
        fs::write(&stylesheet, "section {}\n").unwrap();
        fs::write(
            &included,
            "# Broken include\n\n```nosuchlang\ncontent\n```\n",
        )
        .unwrap();
        fs::write(&deck, "<!-- {\"include\":\"shared/intro.md\"} -->\n").unwrap();
        let options = BuildOptions {
            input: deck.clone(),
            out,
        };
        let mut state = prepare_watch_loop(deck);
        let initial = capture_input_snapshot(&state.targets);
        assert!(matches!(
            initial.get(&included),
            Some(InputFingerprint::Content(_))
        ));
        assert!(matches!(
            initial.get(&layout),
            Some(InputFingerprint::Content(_))
        ));
        assert!(matches!(
            initial.get(&stylesheet),
            Some(InputFingerprint::Content(_))
        ));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&options, &mut stdout, &mut stderr).unwrap();
        assert!(String::from_utf8_lossy(&stderr).contains("build failed:"));
        stdout.clear();
        stderr.clear();

        fs::write(&included, "# Fixed include\n").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&options, stdout, stderr),
        );

        assert!(
            options.out.join("manifest.json").is_file(),
            "{}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    }

    #[test]
    fn referenced_image_set_refreshes_when_deck_references_change() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let first = dir.path().join("img/a.png");
        let second = dir.path().join("pics/b.png");
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::create_dir_all(second.parent().unwrap()).unwrap();
        fs::write(&first, b"first image").unwrap();
        fs::write(&second, b"second image").unwrap();
        fs::write(&deck, "# Intro\n\n![x](img/a.png)\n").unwrap();
        let mut state = prepare_watch_loop(deck.clone());
        assert!(state.input_snapshot.contains_key(&first));
        assert!(!state.input_snapshot.contains_key(&second));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        fs::write(&deck, "# Intro\n\n![x](pics/b.png)\n").unwrap();
        run_watch_ticks(
            &mut state,
            3,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 1);
        assert!(!state.input_snapshot.contains_key(&first));
        assert!(state.input_snapshot.contains_key(&second));
    }

    #[test]
    fn referenced_image_set_refreshes_after_highlighter_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let image = dir.path().join("img/a.png");
        let syntaxes = dir.path().join("syntaxes");
        let syntax = syntaxes.join("carina.sublime-syntax");
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        fs::create_dir_all(&syntaxes).unwrap();
        fs::write(&image, b"image").unwrap();
        fs::write(&deck, "# Intro\n\n![x](img/a.png)\n").unwrap();
        let mut state = prepare_watch_loop(deck);
        assert!(!state.input_snapshot.contains_key(&image));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        fs::write(&syntax, CARINA_SUBLIME_SYNTAX).unwrap();
        run_watch_ticks(
            &mut state,
            3,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 1);
        assert!(state.input_snapshot.contains_key(&image));
    }

    #[test]
    fn watch_rebuild_writes_distribution_without_preview_annotations() {
        let fixture = WatchFixture::new("# Intro\n\nBefore rebuild.\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
        fs::write(&fixture.options.input, "# Intro\n\nAfter rebuild.\n").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );

        let slide = fs::read_to_string(fixture.options.out.join("slides/000-intro.html")).unwrap();
        assert!(slide.contains("After rebuild."), "{slide}");
        assert!(!slide.contains("Before rebuild."), "{slide}");
        assert!(!slide.contains("data-peitho-src"), "{slide}");
        assert!(!slide.contains("data-peitho-md"), "{slide}");
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn watch_rebuild_reports_failure_without_stopping_the_loop() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        fs::write(
            &fixture.options.input,
            "# Intro\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```",
        )
        .unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );

        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stdout.is_empty());
        assert!(stderr.contains("build failed:"), "{stderr}");
        assert!(stderr.contains("slot 'code' got 2 item(s)"), "{stderr}");
    }

    #[test]
    fn watch_emit_failure_is_reported_and_a_later_change_still_rebuilds() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        fs::write(&fixture.options.out, "not a directory").unwrap();
        fs::write(&fixture.options.input, "# Emit failure\n").unwrap();

        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );

        assert!(stdout.is_empty());
        assert!(String::from_utf8_lossy(&stderr).contains("build failed:"));

        fs::remove_file(&fixture.options.out).unwrap();
        fs::write(&fixture.options.input, "# Recovered\n").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );

        let slide =
            fs::read_to_string(fixture.options.out.join("slides/000-recovered.html")).unwrap();
        assert!(slide.contains("Recovered"), "{slide}");
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
    }

    #[test]
    fn bad_frontmatter_asset_path_keeps_previous_targets_and_reports_build_failure() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let original_assets = state.targets.assets.clone();
        let original_snapshot = state.input_snapshot.clone();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        fs::write(
            &fixture.options.input,
            "---\nlayouts: ./missing-layouts\n---\n# Intro\n",
        )
        .unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );

        assert_eq!(state.targets.assets, original_assets);
        assert_ne!(state.input_snapshot, original_snapshot);
        assert!(stdout.is_empty());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("build failed:"), "{stderr}");
        assert!(stderr.contains("layouts path does not exist"), "{stderr}");
        assert!(!stderr.contains("watching new asset paths"), "{stderr}");
    }

    #[test]
    fn failed_watch_rebuild_is_followed_by_a_successful_fix_rebuild() {
        let fixture = WatchFixture::new("# Intro\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        fs::write(
            &fixture.options.input,
            "# Broken\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```\n",
        )
        .unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );
        assert!(stdout.is_empty());
        assert!(String::from_utf8_lossy(&stderr).contains("build failed:"));

        stderr.clear();
        fs::write(&fixture.options.input, "# Fixed\n").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );

        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
        assert!(fixture.options.out.join("slides/000-fixed.html").is_file());
    }

    #[test]
    fn markdown_and_include_changes_rebuild_the_final_content() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let shared = dir.path().join("shared");
        let included = shared.join("intro.md");
        let out = dir.path().join("dist");
        fs::create_dir_all(&shared).unwrap();
        fs::write(&included, "# Intro\n").unwrap();
        fs::write(&deck, "<!-- {\"include\":\"shared/intro.md\"} -->\n").unwrap();
        let options = BuildOptions {
            input: deck.clone(),
            out,
        };
        let mut state = prepare_watch_loop(deck.clone());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        fs::write(&included, "# Intro\n\n---\n# Included Details\n").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&options, stdout, stderr),
        );
        assert!(fs::read_to_string(options.out.join("manifest.json"))
            .unwrap()
            .contains(r#""slideCount": 2"#));

        stdout.clear();
        fs::write(&deck, "# Deck only\n").unwrap();
        run_watch_ticks(
            &mut state,
            3,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&options, stdout, stderr),
        );
        assert!(fs::read_to_string(options.out.join("manifest.json"))
            .unwrap()
            .contains(r#""slideCount": 1"#));
        assert!(stderr.is_empty());
    }

    #[test]
    fn removed_include_leaves_the_tracked_set_after_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared/intro.md");
        fs::create_dir_all(included.parent().unwrap()).unwrap();
        fs::write(&included, "# Included\n").unwrap();
        fs::write(&deck, "<!-- {\"include\":\"shared/intro.md\"} -->\n").unwrap();
        let mut state = prepare_watch_loop(deck.clone());
        assert!(state.input_snapshot.contains_key(&included));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        fs::write(&deck, "# Deck only\n").unwrap();
        run_watch_ticks(
            &mut state,
            3,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 1);
        assert!(!state.input_snapshot.contains_key(&included));
        assert!(!state
            .targets
            .roots
            .iter()
            .any(|root| same_watch_path(&root.path, &included)));

        fs::write(&included, "# No longer included\n").unwrap();
        run_watch_ticks(
            &mut state,
            3,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 1);
    }

    #[test]
    fn truncate_then_write_across_ticks_rebuilds_only_the_final_content() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Initial\n").unwrap();
        let mut state = prepare_watch_loop(deck.clone());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilt_sources = Vec::new();
        let mut failed_builds = 0;
        let fixed_mtime = std::time::UNIX_EPOCH + Duration::from_secs(1_000);

        fs::write(&deck, "").unwrap();
        fs::File::open(&deck)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(fixed_mtime))
            .unwrap();
        handle_watch_tick(
            &mut state,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                failed_builds += 1;
                Ok(())
            },
        )
        .unwrap();
        fs::write(&deck, "# Complete\n").unwrap();
        fs::File::open(&deck)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(fixed_mtime))
            .unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                let source = fs::read_to_string(&deck).unwrap();
                if source.is_empty() {
                    failed_builds += 1;
                } else {
                    rebuilt_sources.push(source);
                }
                Ok(())
            },
        );

        assert_eq!(failed_builds, 0);
        assert_eq!(rebuilt_sources, vec!["# Complete\n"]);
    }

    #[test]
    fn input_changing_on_every_tick_rebuilds_once_after_it_settles() {
        let (dir, mut state, fonts) = watch_state_with_fonts();
        let font = fonts.join("talk-font.woff2");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        for bytes in [
            b"font-1".as_slice(),
            b"font-version-2",
            b"font-version-three",
        ] {
            fs::write(&font, bytes).unwrap();
            handle_watch_tick(
                &mut state,
                &mut stdout,
                &mut stderr,
                &mut |_stdout, _stderr| {
                    rebuilds += 1;
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(rebuilds, 0);
        handle_watch_tick(
            &mut state,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 1);
        drop(dir);
    }

    #[test]
    fn single_write_rebuilds_after_two_identical_captures() {
        let fixture = WatchFixture::new("# Initial\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        fs::write(&fixture.options.input, "# Changed\n").unwrap();
        handle_watch_tick(
            &mut state,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(rebuilds, 0);
        handle_watch_tick(
            &mut state,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rebuilds, 1);
    }

    #[test]
    fn watch_build_snapshot_precedes_the_initial_build_action() {
        let fixture = WatchFixture::new("# Initial\n");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let (mut state, ()) =
            run_initial_action_after_watch_snapshot(fixture.options.input.clone(), || {
                rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr)?;
                fs::write(&fixture.options.input, "# Saved during initial build\n")
                    .into_diagnostic()?;
                Ok(())
            })
            .unwrap();
        stdout.clear();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );

        let slide = fs::read_to_string(
            fixture
                .options
                .out
                .join("slides/000-saved-during-initial-build.html"),
        )
        .unwrap();
        assert!(slide.contains("Saved during initial build"), "{slide}");
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    }

    #[test]
    fn preview_snapshot_precedes_the_initial_preview_build_action() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let cache = dir.path().join("preview-cache");
        fs::write(&deck, "# Initial\n").unwrap();
        let mut initial_stderr = Vec::new();

        let (mut state, root) = run_initial_action_after_watch_snapshot(deck.clone(), || {
            let root = emit_initial_preview_root(&deck, &cache, &mut initial_stderr)?;
            fs::write(&deck, "# Saved during initial preview build\n").into_diagnostic()?;
            Ok(root)
        })
        .unwrap();
        assert!(root.join("manifest.json").is_file());
        assert!(initial_stderr.is_empty());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilt_sources = Vec::new();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilt_sources.push(fs::read_to_string(&deck).unwrap());
                Ok(())
            },
        );

        assert_eq!(
            rebuilt_sources,
            vec!["# Saved during initial preview build\n"]
        );
    }

    #[test]
    fn write_during_rebuild_is_detected_after_it_settles() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Initial\n").unwrap();
        let mut state = prepare_watch_loop(deck.clone());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilt_sources = Vec::new();

        fs::write(&deck, "# First\n").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilt_sources.push(fs::read_to_string(&deck).unwrap());
                fs::write(&deck, "# Saved during rebuild\n").unwrap();
                Ok(())
            },
        );
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilt_sources.push(fs::read_to_string(&deck).unwrap());
                Ok(())
            },
        );

        assert_eq!(
            rebuilt_sources,
            vec!["# First\n", "# Saved during rebuild\n"]
        );
    }

    #[test]
    fn failed_rebuild_installs_the_stable_prebuild_snapshot() {
        let fixture = WatchFixture::new("# Initial\n");
        let mut state = watch_state_for_fixture(&fixture);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        fs::write(&fixture.options.input, "# Changed\n").unwrap();
        handle_watch_tick(
            &mut state,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| Ok(()),
        )
        .unwrap();
        let stable = capture_input_snapshot(&state.targets);

        let err = handle_watch_tick(
            &mut state,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                fs::write(&fixture.options.input, "# Saved during failed rebuild\n").unwrap();
                Err(miette::miette!("injected rebuild failure"))
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("injected rebuild failure"));
        assert_eq!(state.input_snapshot, stable);
        assert_ne!(capture_input_snapshot(&state.targets), state.input_snapshot);
    }

    #[test]
    fn atomic_css_save_and_nested_font_change_each_rebuild_once() {
        let fixture = WatchFixture::new("# Intro\n");
        let fonts = fixture._dir.path().join("fonts/noto");
        fs::create_dir_all(&fonts).unwrap();
        fs::write(fonts.join("talk.woff2"), b"font").unwrap();
        fs::write(
            &fixture.options.input,
            "---\nfonts: ./fonts\n---\n# Intro\n",
        )
        .unwrap();
        let mut state = prepare_watch_loop(fixture.options.input.clone());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        let css = fixture._dir.path().join("css/talk.css");
        let temp = fixture._dir.path().join("css/.talk.css.tmp");
        fs::write(&temp, "body { color: blue; }\n").unwrap();
        fs::rename(&temp, &css).unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );
        fs::write(fonts.join("talk.woff2"), b"longer font bytes").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 2);
    }

    #[test]
    fn deck_and_font_saved_together_rebuild_once() {
        let (dir, mut state, fonts) = watch_state_with_fonts();
        let deck = state.input.clone();
        let font = fonts.join("talk.woff2");
        fs::write(&font, b"font one").unwrap();
        state.input_snapshot = capture_input_snapshot(&state.targets);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        fs::write(&deck, "---\nfonts: ./fonts\n---\n# Changed\n").unwrap();
        fs::write(&font, b"font version two is longer").unwrap();
        run_watch_ticks(
            &mut state,
            3,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 1);
        drop(dir);
    }

    #[test]
    fn new_font_and_deleted_include_each_rebuild_once() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared/intro.md");
        let font = dir.path().join("fonts/new.woff2");
        fs::create_dir_all(included.parent().unwrap()).unwrap();
        fs::create_dir_all(font.parent().unwrap()).unwrap();
        fs::write(&included, "# Intro\n").unwrap();
        fs::write(
            &deck,
            "---\nfonts: ./fonts\n---\n<!-- {\"include\":\"shared/intro.md\"} -->\n",
        )
        .unwrap();
        let mut state = prepare_watch_loop(deck);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        fs::write(&font, b"font").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );
        fs::remove_file(&included).unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 2);
    }

    #[test]
    fn zero_config_css_directory_appearing_rebuilds_once_and_refreshes_targets() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let css = dir.path().join("css");
        fs::write(&deck, "# Initial\n").unwrap();
        let mut state = prepare_watch_loop(deck);
        assert_eq!(state.targets.assets.css, Provenance::Builtin);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        fs::create_dir(&css).unwrap();
        fs::write(css.join("talk.css"), "body {}\n").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 1);
        assert_eq!(state.targets.assets.css, Provenance::DeckAdjacent(css));
        assert!(String::from_utf8(stderr)
            .unwrap()
            .contains("watching new asset paths from frontmatter"));
    }

    #[test]
    fn referenced_image_change_rebuilds_distribution() {
        let fixture = WatchFixture::new("# Intro\n\n![x](img/a.png)\n");
        let image = fixture._dir.path().join("img/a.png");
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        fs::write(&image, b"old image").unwrap();
        fs::write(
            fixture._dir.path().join("layouts/title-body-code.html"),
            TEST_IMAGE_LAYOUT_HTML,
        )
        .unwrap();
        let mut state = prepare_watch_loop(fixture.options.input.clone());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
        stdout.clear();

        let new_bytes = b"new image bytes are longer";
        fs::write(&image, new_bytes).unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&fixture.options, stdout, stderr),
        );

        assert!(fixture
            .options
            .out
            .join(format!("assets/{}-a.png", short_sha256_hex(new_bytes, 16)))
            .exists());
        assert!(stderr.is_empty());
    }

    #[test]
    fn frontmatter_and_include_target_changes_are_refreshed() {
        let fixture = WatchFixture::new("# Intro\n");
        let alternate_layouts = fixture._dir.path().join("other-layouts");
        let included = fixture._dir.path().join("shared/intro.md");
        fs::create_dir_all(&alternate_layouts).unwrap();
        fs::create_dir_all(included.parent().unwrap()).unwrap();
        fs::write(
            alternate_layouts.join("title-body-code.html"),
            TEST_LAYOUT_HTML,
        )
        .unwrap();
        fs::write(&included, "# Included\n").unwrap();
        let mut state = watch_state_for_fixture(&fixture);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        fs::write(
            &fixture.options.input,
            "---\nlayouts: ./other-layouts\n---\n<!-- {\"include\":\"shared/intro.md\"} -->\n",
        )
        .unwrap();
        run_watch_ticks(
            &mut state,
            3,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );
        let refreshed = capture_input_snapshot(&state.targets);

        assert_eq!(rebuilds, 1);
        assert_eq!(
            state.targets.assets.layouts,
            Provenance::Explicit(alternate_layouts)
        );
        assert!(matches!(
            refreshed.get(&included),
            Some(InputFingerprint::Content(_))
        ));
        assert!(String::from_utf8(stderr)
            .unwrap()
            .contains("watching new asset paths from frontmatter"));
    }

    #[test]
    fn initial_resolution_failure_recovers_after_the_deck_is_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let out = dir.path().join("dist");
        fs::write(&deck, "---\ntime: [\n---\n# Broken\n").unwrap();
        let mut state = prepare_watch_loop(deck.clone());
        let options = BuildOptions {
            input: deck.clone(),
            out,
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        rebuild_once_for_watch(&options, &mut stdout, &mut stderr).unwrap();
        assert!(String::from_utf8_lossy(&stderr).contains("build failed:"));
        stdout.clear();
        stderr.clear();
        fs::write(&deck, "# Recovered\n").unwrap();
        run_watch_ticks(
            &mut state,
            2,
            &mut stdout,
            &mut stderr,
            &mut |stdout, stderr| rebuild_once_for_watch(&options, stdout, stderr),
        );

        assert!(options.out.join("manifest.json").exists());
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("built 1 slide(s)"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn output_and_hidden_file_churn_never_rebuilds() {
        let fixture = WatchFixture::new("# Intro\n");
        let fonts = fixture._dir.path().join("fonts");
        fs::create_dir_all(&fonts).unwrap();
        fs::write(
            &fixture.options.input,
            "---\nfonts: ./fonts\n---\n# Intro\n",
        )
        .unwrap();
        let mut state = prepare_watch_loop(fixture.options.input.clone());
        let preview_cache = fixture._dir.path().join(PREVIEW_CACHE);
        fs::create_dir_all(&fixture.options.out).unwrap();
        fs::create_dir_all(preview_cache.join("build-0")).unwrap();
        fs::write(fixture.options.out.join("index.html"), "output").unwrap();
        fs::write(preview_cache.join("build-0/index.html"), "preview").unwrap();
        fs::write(fonts.join(".copying"), b"partial font").unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut rebuilds = 0;

        run_watch_ticks(
            &mut state,
            4,
            &mut stdout,
            &mut stderr,
            &mut |_stdout, _stderr| {
                rebuilds += 1;
                Ok(())
            },
        );

        assert_eq!(rebuilds, 0);
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
                assert_eq!(host, Some(Some("100.64.0.5".parse().unwrap())));
            }
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
                panic!("expected present command");
            }
        }
    }

    #[test]
    fn present_command_accepts_bare_host_as_auto_select() {
        let cli = Cli::parse_from(["peitho", "present", "deck.md", "--host"]);

        assert_eq!(present_host_from_cli(cli), Some(None));
    }

    #[test]
    fn present_command_accepts_rehearsal_flag() {
        let cli = Cli::parse_from(["peitho", "present", "deck.md", "--rehearsal"]);

        assert!(present_rehearsal_from_cli(cli));
    }

    #[test]
    fn present_audio_requires_rehearsal_at_clap_boundary() {
        let err = Cli::try_parse_from(["peitho", "present", "deck.md", "--audio"]).unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        assert!(err.to_string().contains("--rehearsal"));
    }

    #[test]
    fn present_audio_conflicts_with_no_presenter_at_clap_boundary() {
        let err = Cli::try_parse_from([
            "peitho",
            "present",
            "deck.md",
            "--rehearsal",
            "--audio",
            "--no-presenter",
        ])
        .unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
        assert!(err.to_string().contains("--no-presenter"));
    }

    #[test]
    fn rehearsal_command_accepts_all_flag() {
        let cli = Cli::parse_from(["peitho", "rehearsal", "--all"]);

        match cli.command {
            Command::Rehearsal { all } => assert!(all),
            other => panic!("expected rehearsal command, got {other:?}"),
        }
    }

    #[test]
    fn present_command_without_port_preserves_unspecified_state() {
        let cli = Cli::parse_from(["peitho", "present", "deck.md", "--host"]);

        assert_eq!(present_port_from_cli(cli), None);
    }

    #[test]
    fn present_command_accepts_explicit_zero_port() {
        let cli = Cli::parse_from(["peitho", "present", "deck.md", "--host", "--port", "0"]);

        assert_eq!(present_port_from_cli(cli), Some(0));
    }

    #[test]
    fn present_port_resolution_matrix_keeps_explicit_ports_and_defaults_stable_origins() {
        let host = ResolvedPresentHost::Explicit("100.64.0.5".parse().unwrap());
        let auto_host = ResolvedPresentHost::Auto(AutoHostCandidate {
            address: "100.64.0.5".parse().unwrap(),
            label: Some(peitho::remote_url::RemoteUrlLabel::Vpn),
        });

        assert_eq!(
            resolve_present_port(Some(4321), &ResolvedPresentHost::None, true),
            ResolvedPresentPort {
                port: 4321,
                source: PresentPortSource::Explicit,
            }
        );
        assert_eq!(
            resolve_present_port(Some(6174), &host, true),
            ResolvedPresentPort {
                port: 6174,
                source: PresentPortSource::Explicit,
            }
        );
        assert_eq!(
            resolve_present_port(Some(0), &host, true),
            ResolvedPresentPort {
                port: 0,
                source: PresentPortSource::Explicit,
            }
        );
        assert_eq!(
            resolve_present_port(None, &host, false),
            ResolvedPresentPort {
                port: STABLE_PRESENT_PORT,
                source: PresentPortSource::StableDefault,
            }
        );
        assert_eq!(
            resolve_present_port(None, &auto_host, true),
            ResolvedPresentPort {
                port: STABLE_PRESENT_PORT,
                source: PresentPortSource::StableDefault,
            }
        );
        assert_eq!(
            resolve_present_port(None, &ResolvedPresentHost::None, true),
            ResolvedPresentPort {
                port: STABLE_PRESENT_PORT,
                source: PresentPortSource::StableDefault,
            }
        );
        assert_eq!(
            resolve_present_port(None, &ResolvedPresentHost::None, false),
            ResolvedPresentPort {
                port: 0,
                source: PresentPortSource::RandomDefault,
            }
        );
    }

    #[test]
    fn present_command_without_host_keeps_host_none() {
        let cli = Cli::parse_from(["peitho", "present", "deck.md"]);

        assert_eq!(present_host_from_cli(cli), None);
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
            port: None,
            no_open: true,
            no_serve: true,
            no_presenter: false,
            presenter_windowed: false,
            host: Some(Some("100.64.0.5".parse().unwrap())),
            rehearsal: false,
            audio: false,
        };

        let err = validate_present_options(&options).unwrap_err();

        assert!(err
            .to_string()
            .contains("--host requires the present server"));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("--no-serve"));
    }

    #[test]
    fn present_options_reject_loopback_host() {
        let options = PresentOptions {
            input: PathBuf::from("deck.md"),
            shell: None,
            port: None,
            no_open: true,
            no_serve: false,
            no_presenter: false,
            presenter_windowed: false,
            host: Some(Some("127.0.0.1".parse().unwrap())),
            rehearsal: false,
            audio: false,
        };

        let err = validate_present_options(&options).unwrap_err();

        assert!(err.to_string().contains("--host must be non-loopback"));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("use the default loopback server"));
    }

    #[test]
    fn present_options_reject_rehearsal_with_no_serve() {
        let options = PresentOptions {
            input: PathBuf::from("deck.md"),
            shell: None,
            port: None,
            no_open: true,
            no_serve: true,
            no_presenter: false,
            presenter_windowed: false,
            host: None,
            rehearsal: true,
            audio: false,
        };

        let err = validate_present_options(&options).unwrap_err();

        assert!(err
            .to_string()
            .contains("--rehearsal requires the present server"));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("--no-serve"));
    }

    #[test]
    fn rehearsal_mode_rejects_sectionless_deck() {
        let fixture = WatchFixture::new("# Intro\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();

        let err = validate_rehearsal_sections(&artifacts).unwrap_err();

        assert!(err
            .to_string()
            .contains("--rehearsal requires agenda sections"));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains(r#"declare {"section":...} page comments"#));
    }

    #[test]
    fn rehearsal_mode_accepts_deck_with_sections() {
        let fixture = WatchFixture::new(
            "---\ntime: 1m\n---\n<!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# Intro\n",
        );
        let artifacts = build_artifacts(&fixture.options.input).unwrap();

        validate_rehearsal_sections(&artifacts).unwrap();
    }

    #[test]
    fn rehearsal_slide_keys_follow_final_rendered_slide_order() {
        let fixture = WatchFixture::new(
            "---\ntime: 1m\n---\n<!-- {\"section\":\"Setup\",\"time\":\"1m\",\"key\":\"intro\"} -->\n# Intro\n\n---\n\n<!-- {\"draft\":true} -->\n# Removed\n\n---\n\n<!-- {\"key\":\"details\"} -->\n# Details\n",
        );
        let artifacts = build_artifacts(&fixture.options.input).unwrap();

        let keys = expected_rehearsal_slide_keys(&artifacts);

        assert_eq!(
            keys.iter().map(SlideKey::as_str).collect::<Vec<_>>(),
            ["intro", "details"]
        );
    }

    #[test]
    fn binding_busy_stable_default_port_reports_present_specific_help() {
        let listener = match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)) {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping bind-busy assertion: TCP bind is denied in this sandbox");
                return;
            }
            Err(err) => panic!("failed to reserve an ephemeral port for bind-busy test: {err}"),
        };
        let port = listener.local_addr().unwrap().port();
        let dir = tempfile::tempdir().unwrap();

        let err = bind_present_server(
            dir.path().to_path_buf(),
            ResolvedPresentPort {
                port,
                source: PresentPortSource::StableDefault,
            },
            "present.html",
            &ResolvedPresentHost::None,
        )
        .err()
        .unwrap();
        let err = err
            .downcast_ref::<PresentDefaultPortInUseError>()
            .expect("expected stable default port-in-use diagnostic");

        assert_eq!(err.port, port);
        assert_eq!(err.source_io_kind(), Some(std::io::ErrorKind::AddrInUse));
        let help = miette::Diagnostic::help(err).unwrap().to_string();
        assert!(help.contains("another `peitho present` is probably running"));
        assert!(help.contains("pass `--port`"));
    }

    #[test]
    fn stable_default_bind_help_is_limited_to_addr_in_use() {
        let stable_default = ResolvedPresentPort {
            port: STABLE_PRESENT_PORT,
            source: PresentPortSource::StableDefault,
        };

        let in_use = annotate_present_bind_error(
            stable_default,
            synthesized_bind_error(std::io::ErrorKind::AddrInUse),
        );
        let in_use = in_use
            .downcast_ref::<PresentDefaultPortInUseError>()
            .expect("expected stable default port-in-use diagnostic");
        assert_eq!(in_use.source_io_kind(), Some(std::io::ErrorKind::AddrInUse));

        let addr_not_available = annotate_present_bind_error(
            stable_default,
            synthesized_bind_error(std::io::ErrorKind::AddrNotAvailable),
        );
        let addr_not_available = addr_not_available
            .downcast_ref::<server::PresentServerBindError>()
            .expect("non-conflict bind failures should pass through unchanged");
        assert_eq!(
            addr_not_available.io_kind(),
            std::io::ErrorKind::AddrNotAvailable
        );
    }

    #[test]
    fn auto_host_candidate_selects_vpn_over_lan_default_route() {
        let candidate = auto_host_candidate_from_addresses(
            &[
                "192.168.1.20".parse().unwrap(),
                "100.100.10.5".parse().unwrap(),
            ],
            Some("192.168.1.20".parse().unwrap()),
        )
        .unwrap();

        assert_eq!(candidate.address, "100.100.10.5".parse::<IpAddr>().unwrap());
        assert_eq!(
            candidate.label,
            Some(peitho::remote_url::RemoteUrlLabel::Vpn)
        );
    }

    #[test]
    fn auto_host_candidate_selects_lan_default_route_without_vpn() {
        let candidate = auto_host_candidate_from_addresses(
            &[
                "192.168.1.20".parse().unwrap(),
                "10.0.0.15".parse().unwrap(),
            ],
            Some("10.0.0.15".parse().unwrap()),
        )
        .unwrap();

        assert_eq!(candidate.address, "10.0.0.15".parse::<IpAddr>().unwrap());
        assert_eq!(candidate.label, None);
    }

    #[test]
    fn auto_host_candidate_errors_without_non_loopback_addresses() {
        let err = auto_host_candidate_from_addresses(
            &[
                "127.0.0.1".parse().unwrap(),
                "169.254.10.20".parse().unwrap(),
                "::1".parse().unwrap(),
                "fe80::1".parse().unwrap(),
            ],
            None,
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("no non-loopback network address"));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("pass --host <IP>"));
        assert!(help.contains("check the network"));
    }

    #[test]
    fn remote_control_lines_label_specific_vpn_host() {
        let target = remote_control_target_for_host("100.64.0.5".parse().unwrap(), 3000);
        let output = remote_control_output(&target);

        assert_eq!(
            output.lines,
            vec!["remote control (VPN): http://100.64.0.5:3000/remote"]
        );
        assert_eq!(
            output.qr.as_ref().unwrap().url.as_str(),
            remote_url_from_control_line(&output.lines[0])
        );
        assert_eq!(
            output.qr.unwrap().caption,
            "scan to open (VPN): http://100.64.0.5:3000/remote"
        );
    }

    #[test]
    fn remote_control_lines_leave_specific_lan_host_unlabeled() {
        let target = remote_control_target_for_host("192.168.1.20".parse().unwrap(), 3000);
        let output = remote_control_output(&target);

        assert_eq!(
            output.lines,
            vec!["remote control: http://192.168.1.20:3000/remote"]
        );
        assert_eq!(
            output.qr.as_ref().unwrap().url.as_str(),
            remote_url_from_control_line(&output.lines[0])
        );
        assert_eq!(
            output.qr.unwrap().caption,
            "scan to open: http://192.168.1.20:3000/remote"
        );
    }

    #[test]
    fn remote_control_lines_bracket_specific_ipv6_host() {
        let target = remote_control_target_for_host("2001:db8::5".parse().unwrap(), 3000);
        let output = remote_control_output(&target);

        assert_eq!(
            output.lines,
            vec!["remote control: http://[2001:db8::5]:3000/remote"]
        );
        assert_eq!(
            output.qr.as_ref().unwrap().url.as_str(),
            "http://[2001:db8::5]:3000/remote"
        );
        assert_eq!(
            output.qr.as_ref().unwrap().url.as_str(),
            remote_url_from_control_line(&output.lines[0])
        );
        assert_eq!(
            output.qr.unwrap().caption,
            "scan to open: http://[2001:db8::5]:3000/remote"
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
        let output = remote_control_output(&target);

        assert_eq!(
            output.lines,
            vec![
                "remote control (VPN): http://100.100.10.5:3000/remote",
                "remote control: http://10.0.0.15:3000/remote",
                "remote control: http://192.168.1.20:3000/remote"
            ]
        );
        assert_eq!(
            output.qr.as_ref().unwrap().url.as_str(),
            remote_url_from_control_line(&output.lines[0])
        );
        assert_eq!(
            output.qr.unwrap().caption,
            "scan to open (VPN): http://100.100.10.5:3000/remote"
        );
    }

    #[test]
    fn remote_control_lines_show_when_unspecified_host_has_no_candidates() {
        let target = RemoteControlTarget::Candidates(Vec::new());
        let output = remote_control_output(&target);

        assert_eq!(
            output.lines,
            vec!["remote control: no non-loopback network addresses found"]
        );
        assert!(output.qr.is_none());
    }

    fn remote_url_from_control_line(line: &str) -> &str {
        let start = line
            .find("http://")
            .unwrap_or_else(|| panic!("remote control line did not contain a URL: {line}"));
        &line[start..]
    }

    fn present_host_from_cli(cli: Cli) -> Option<Option<IpAddr>> {
        match cli.command {
            Command::Present { host, .. } => host,
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
                panic!("expected present command");
            }
        }
    }

    fn present_rehearsal_from_cli(cli: Cli) -> bool {
        match cli.command {
            Command::Present { rehearsal, .. } => rehearsal,
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
                panic!("expected present command");
            }
        }
    }

    fn present_port_from_cli(cli: Cli) -> Option<u16> {
        match cli.command {
            Command::Present { port, .. } => port,
            Command::Build { .. }
            | Command::Lint { .. }
            | Command::New { .. }
            | Command::Layouts { .. }
            | Command::Doctor { .. }
            | Command::Preview { .. }
            | Command::Publish { .. }
            | Command::Export { .. }
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
                panic!("expected present command");
            }
        }
    }

    fn synthesized_bind_error(kind: std::io::ErrorKind) -> miette::Report {
        miette::Report::new(server::PresentServerBindError::new(
            std::net::SocketAddr::from(([127, 0, 0, 1], STABLE_PRESENT_PORT)),
            std::io::Error::from(kind),
        ))
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
                panic!("expected export pdf command");
            }
        }
    }

    #[test]
    fn docs_topic_lists_every_guide_slug_as_a_possible_value() {
        let cmd = Cli::command();
        let docs = cmd.find_subcommand("docs").unwrap();
        let topic = docs.get_positionals().next().unwrap();
        let values: Vec<String> = topic
            .get_possible_values()
            .iter()
            .map(|value| value.get_name().to_string())
            .collect();
        assert_eq!(values, docs::topic_slugs().collect::<Vec<_>>());
        assert!(values.contains(&"getting-started".to_string()));
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
            | Command::Export { .. }
            | Command::Docs { .. }
            | Command::Rehearsal { .. } => {
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
            OsString::from("--enable-logging=stderr"),
            OsString::from(format!("--user-data-dir={}", profile.display())),
            OsString::from(format!("--print-to-pdf={}", out.display())),
            OsString::from(url),
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn chrome_print_args_enable_stderr_console_logging_once() {
        let args = chrome_print_args(
            Path::new("/tmp/peitho-profile"),
            Path::new("/tmp/out.pdf"),
            "file:///tmp/pdf.html",
        );

        assert_eq!(
            args.iter()
                .filter(|arg| arg.to_string_lossy() == "--enable-logging=stderr")
                .count(),
            1
        );
    }

    #[test]
    fn chrome_export_args_start_blank_cdp_session_without_virtual_time_or_print_flag() {
        let profile = Path::new("/tmp/peitho-export/chrome-profile");

        let args = chrome_export_args(profile);

        assert_eq!(
            args,
            vec![
                OsString::from("--headless=new"),
                OsString::from("--disable-gpu"),
                OsString::from("--no-sandbox"),
                OsString::from("--remote-debugging-port=0"),
                OsString::from("--enable-logging=stderr"),
                OsString::from("--user-data-dir=/tmp/peitho-export/chrome-profile"),
                OsString::from("about:blank"),
            ]
        );
        assert!(!has_arg_prefix(&args, "--print-to-pdf="));
        assert!(!has_arg_prefix(&args, "--virtual-time-budget="));
    }

    #[test]
    fn chrome_one_shot_timeout_matches_pdf_flatten_readiness_budget_contract() {
        // Together with
        // render::tests::pdf_flatten_readiness_timeouts_fit_inside_chrome_deadline
        // in peitho-core, this fixes the cross-crate readiness/print budget.
        assert_eq!(CHROME_ONE_SHOT_TIMEOUT, Duration::from_secs(60));
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
        assert_theme_fonts_written(&workspace);
    }

    #[test]
    fn emit_pdf_workspace_omits_sources_json() {
        let fixture = WatchFixture::new("# Export\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let workspace = fixture._dir.path().join("pdf-workspace");

        emit_pdf_workspace(&workspace, &artifacts).unwrap();

        assert!(!workspace.join("sources.json").exists());
    }

    #[test]
    fn non_preview_rendered_documents_omit_preview_edit_annotations() {
        let fixture = WatchFixture::new("# Intro\n\nEditable **body**.\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let present_cache = fixture._dir.path().join("present-cache");
        let pdf_workspace = fixture._dir.path().join("pdf-workspace");

        emit_present_cache(&present_cache, &artifacts, None, false, false).unwrap();
        emit_pdf_workspace(&pdf_workspace, &artifacts).unwrap();

        let documents = [
            (
                "present cache slide",
                fs::read_to_string(present_cache.join("slides/000-intro.html")).unwrap(),
            ),
            (
                "PDF HTML",
                fs::read_to_string(pdf_workspace.join("pdf.html")).unwrap(),
            ),
            (
                "lint HTML",
                peitho_core::render_lint_document(&artifacts.rendered),
            ),
        ];
        for (name, document) in documents {
            for forbidden in ["data-peitho-src", "data-peitho-md"] {
                assert!(
                    !document.contains(forbidden),
                    "{forbidden} found in {name}: {document}"
                );
            }
        }
    }

    #[test]
    fn emit_pdf_workspace_writes_katex_fonts_for_math_deck() {
        let fixture = WatchFixture::new("# Math\n\n```math\n\\frac{1}{2}\n```\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let workspace = fixture._dir.path().join("pdf-workspace");

        emit_pdf_workspace(&workspace, &artifacts).unwrap();

        assert_eq!(
            fs::read(workspace.join("katex-fonts/KaTeX_Main-Regular.woff2")).unwrap(),
            katex_font_bytes("KaTeX_Main-Regular.woff2")
        );
        assert!(fs::read_to_string(workspace.join("peitho.css"))
            .unwrap()
            .contains("url(katex-fonts/"));
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
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("workspace kept at"), "actual help: {help}");
        assert!(
            help.contains(&workspace.display().to_string()),
            "actual help: {help}"
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
        let help = err.help().expect("help must be present").to_string();
        assert!(
            help.contains("PEITHO_CHROME_PATH=<absolute-path>"),
            "actual help: {help}"
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
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("retry lint"), "actual help: {help}");
        assert!(help.contains("lint workspace"), "actual help: {help}");
        assert!(help.contains("lint.html"), "actual help: {help}");
        assert!(!help.contains("chrome-stderr.log"), "actual help: {help}");
        assert!(!help.contains("retry export"), "actual help: {help}");
    }

    #[test]
    fn chrome_process_kill_error_does_not_use_install_hint() {
        let err = chrome_process_error(
            Path::new("/tmp/chrome"),
            ProcessRunError::Kill(std::io::Error::other("kill failed")),
            &ChromeCompletion::LintResultLogged,
        );
        let message = err.to_string();

        assert!(
            message.contains("failed to terminate Chrome: kill failed"),
            "actual error: {message}"
        );
        let help = err.help().expect("help must be present").to_string();
        assert!(
            help.contains("report the underlying io error"),
            "actual help: {help}"
        );
        assert!(
            !help.contains("install Google Chrome"),
            "actual help: {help}"
        );
    }

    #[test]
    fn stderr_excerpt_replaces_control_characters_before_truncation() {
        let excerpt = stderr_excerpt(b"\x1b]0;pwned\x07\nnormal stderr");

        assert_eq!(excerpt, "]0;pwned normal stderr");
        assert!(!excerpt.chars().any(char::is_control), "{excerpt:?}");
    }

    type OEmbedCurlCall = (OsString, Vec<OsString>, bool, Duration);

    struct FixtureOEmbedCurlInvoker {
        calls: RefCell<Vec<OEmbedCurlCall>>,
        result: RefCell<Option<Result<OEmbedCurlOutcome, ProcessRunError>>>,
    }

    impl FixtureOEmbedCurlInvoker {
        fn outcome(outcome: OEmbedCurlOutcome) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                result: RefCell::new(Some(Ok(outcome))),
            }
        }

        fn error(error: ProcessRunError) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                result: RefCell::new(Some(Err(error))),
            }
        }
    }

    impl OEmbedCurlInvoker for FixtureOEmbedCurlInvoker {
        fn invoke(
            &self,
            program: &OsStr,
            args: &[OsString],
            stdin: Option<&[u8]>,
            timeout: Duration,
        ) -> Result<OEmbedCurlOutcome, ProcessRunError> {
            self.calls.borrow_mut().push((
                program.to_os_string(),
                args.to_vec(),
                stdin.is_some(),
                timeout,
            ));
            self.result
                .borrow_mut()
                .take()
                .expect("fixture curl outcome is consumed once")
        }
    }

    fn successful_oembed_outcome(stdout: &[u8]) -> OEmbedCurlOutcome {
        OEmbedCurlOutcome::Exited {
            success: true,
            code: Some(0),
            status: "exit status: 0".to_owned(),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn oembed_curl_runner_targets_publish_x_without_redirect_following() {
        let invoker = FixtureOEmbedCurlInvoker::outcome(successful_oembed_outcome(b"{}"));

        fetch_oembed_with_invoker("https://x.com/A/status/1", &invoker).unwrap();

        let calls = invoker.calls.borrow();
        let args = &calls[0].1;
        let endpoint = args.last().unwrap().to_string_lossy();
        assert!(endpoint.starts_with("https://publish.x.com/oembed?"));
        assert!(!endpoint.contains("publish.twitter.com"));
        assert!(!has_arg(args, "-L"));
        assert!(!has_arg(args, "--location"));
    }

    #[test]
    fn x_oembed_curl_argv_remains_byte_identical_without_redirects() {
        let invoker = FixtureOEmbedCurlInvoker::outcome(successful_oembed_outcome(b"{}"));

        fetch_oembed_with_invoker("https://x.com/A/status/1", &invoker).unwrap();

        let calls = invoker.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, OsStr::new("curl"));
        assert_eq!(
            calls[0].1,
            [
                OsString::from("-fsS"),
                OsString::from("--max-time"),
                OsString::from("30"),
                OsString::from("--max-filesize"),
                OsString::from(peitho_core::MAX_OEMBED_RESPONSE_BYTES.to_string()),
                OsString::from(
                    "https://publish.x.com/oembed?url=https%3A%2F%2Fx.com%2FA%2Fstatus%2F1&omit_script=1&dnt=1&hide_thread=1"
                ),
            ]
        );
        assert!(!calls[0].2, "curl must receive no stdin pipe");
        assert_eq!(calls[0].3, Duration::from_secs(35));
    }

    fn assert_generic_curl_argv(
        operation: GenericOEmbedFetchOperation,
        url: &str,
        expected_cap: usize,
    ) {
        let invoker = FixtureOEmbedCurlInvoker::outcome(successful_oembed_outcome(b"body"));

        fetch_generic_oembed_with_invoker(url, operation, &invoker).unwrap();

        let calls = invoker.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, OsStr::new("curl"));
        assert_eq!(
            calls[0].1,
            [
                OsString::from("-fsS"),
                OsString::from("-L"),
                OsString::from("--max-redirs"),
                OsString::from("5"),
                OsString::from("--proto"),
                OsString::from("=http,https"),
                OsString::from("--proto-redir"),
                OsString::from("=http,https"),
                OsString::from("--max-time"),
                OsString::from("30"),
                OsString::from("--max-filesize"),
                OsString::from(expected_cap.to_string()),
                OsString::from(url),
            ]
        );
        assert!(!calls[0].2, "generic curl must receive no stdin pipe");
        assert_eq!(calls[0].3, Duration::from_secs(35));
    }

    #[test]
    fn generic_page_curl_uses_redirect_limit_protocol_guard_and_8_mib_cap() {
        assert_generic_curl_argv(
            GenericOEmbedFetchOperation::DiscoveryPage,
            "https://example.com/post",
            peitho_core::MAX_OEMBED_DISCOVERY_PAGE_BYTES,
        );
    }

    #[test]
    fn generic_endpoint_curl_uses_redirect_limit_and_1_mib_cap() {
        assert_generic_curl_argv(
            GenericOEmbedFetchOperation::DiscoveredEndpoint,
            "https://example.com/oembed?url=post",
            peitho_core::MAX_OEMBED_RESPONSE_BYTES,
        );
    }

    #[test]
    fn generic_thumbnail_curl_uses_redirect_limit_and_8_mib_cap() {
        assert_generic_curl_argv(
            GenericOEmbedFetchOperation::Thumbnail,
            "https://cdn.example.com/thumb.jpg",
            peitho_core::MAX_OEMBED_THUMBNAIL_BYTES,
        );
    }

    #[test]
    fn generic_curl_sends_no_stdin_and_returns_complete_bytes() {
        let response = b"\x00\xffcomplete generic bytes\n";
        let invoker = FixtureOEmbedCurlInvoker::outcome(successful_oembed_outcome(response));

        let bytes = fetch_generic_oembed_with_invoker(
            "https://example.com/post",
            GenericOEmbedFetchOperation::DiscoveryPage,
            &invoker,
        )
        .unwrap();

        assert_eq!(bytes, response);
        assert!(!invoker.calls.borrow()[0].2);
    }

    #[test]
    fn generic_curl_reports_redirect_http_exit_timeout_and_stderr() {
        for (code, stderr) in [
            (47, "curl: (47) Maximum (5) redirects followed"),
            (22, "curl: (22) HTTP 404"),
            (7, "curl: (7) Failed to connect"),
        ] {
            let invoker = FixtureOEmbedCurlInvoker::outcome(OEmbedCurlOutcome::Exited {
                success: false,
                code: Some(code),
                status: format!("exit status: {code}"),
                stdout: Vec::new(),
                stderr: stderr.as_bytes().to_vec(),
            });

            let err = fetch_generic_oembed_with_invoker(
                "https://example.com/post",
                GenericOEmbedFetchOperation::DiscoveryPage,
                &invoker,
            )
            .unwrap_err();

            assert_eq!(err.line, None);
            assert!(err.message.contains(&format!("exit status: {code}")));
            assert!(err.message.contains(stderr));
            assert!(err.help.contains("HTTP(S)"), "{}", err.help);
        }

        let invoker = FixtureOEmbedCurlInvoker::outcome(OEmbedCurlOutcome::TimedOut {
            stderr: b"provider timed out".to_vec(),
        });
        let err = fetch_generic_oembed_with_invoker(
            "https://example.com/post",
            GenericOEmbedFetchOperation::DiscoveryPage,
            &invoker,
        )
        .unwrap_err();
        assert!(
            err.message.contains("timed out after 35s"),
            "{}",
            err.message
        );
        assert!(
            err.message.contains("provider timed out"),
            "{}",
            err.message
        );
    }

    #[test]
    fn generic_curl_reports_missing_curl() {
        let invoker = FixtureOEmbedCurlInvoker::error(ProcessRunError::Spawn(io::Error::new(
            io::ErrorKind::NotFound,
            "curl not found",
        )));

        let err = fetch_generic_oembed_with_invoker(
            "https://example.com/post",
            GenericOEmbedFetchOperation::DiscoveryPage,
            &invoker,
        )
        .unwrap_err();

        assert_eq!(err.line, None);
        assert!(
            err.message.contains("failed to start curl"),
            "{}",
            err.message
        );
        assert!(err.message.contains("curl not found"), "{}", err.message);
        assert!(err.help.contains("install curl"), "{}", err.help);
    }

    #[test]
    fn oembed_curl_runner_returns_complete_stdout_after_successful_exit() {
        let response = b"{\"html\":\"complete\"}\n";
        let invoker = FixtureOEmbedCurlInvoker::outcome(successful_oembed_outcome(response));

        let raw = fetch_oembed_with_invoker("https://x.com/a/status/1", &invoker).unwrap();

        assert_eq!(raw.as_bytes(), response);
    }

    #[test]
    fn oembed_curl_runner_reports_exit_status_and_stderr() {
        let invoker = FixtureOEmbedCurlInvoker::outcome(OEmbedCurlOutcome::Exited {
            success: false,
            code: Some(7),
            status: "exit status: 7".to_owned(),
            stdout: Vec::new(),
            stderr: b"curl: (7) Failed to connect\n".to_vec(),
        });

        let err = fetch_oembed_with_invoker("https://x.com/a/status/1", &invoker).unwrap_err();

        assert_eq!(err.kind, peitho_core::error::ErrorKind::Asset);
        assert_eq!(err.line, None);
        assert!(err.message.contains("exit status: 7"), "{}", err.message);
        assert!(err.message.contains("Failed to connect"), "{}", err.message);
        assert_eq!(
            err.help,
            "check network access to publish.x.com and retry with curl installed"
        );
    }

    #[test]
    fn oembed_curl_runner_reports_deleted_or_private_post_for_http_failure() {
        let invoker = FixtureOEmbedCurlInvoker::outcome(OEmbedCurlOutcome::Exited {
            success: false,
            code: Some(22),
            status: "exit status: 22".to_owned(),
            stdout: Vec::new(),
            stderr: b"curl: (22) HTTP 404\n".to_vec(),
        });

        let err = fetch_oembed_with_invoker("https://x.com/a/status/1", &invoker).unwrap_err();

        assert!(
            err.help
                .starts_with("the X post may have been deleted or made private"),
            "{}",
            err.help
        );
        assert!(err.help.contains("network access"), "{}", err.help);
        assert!(err.help.contains("retry"), "{}", err.help);
    }

    #[test]
    fn oembed_curl_runner_reports_timeout_and_stderr() {
        let invoker = FixtureOEmbedCurlInvoker::outcome(OEmbedCurlOutcome::TimedOut {
            stderr: b"operation timed out".to_vec(),
        });

        let err = fetch_oembed_with_invoker("https://x.com/a/status/1", &invoker).unwrap_err();

        assert!(
            err.message.contains("timed out after 35s"),
            "{}",
            err.message
        );
        assert!(
            err.message.contains("operation timed out"),
            "{}",
            err.message
        );
        assert!(err.help.contains("network access"), "{}", err.help);
    }

    #[test]
    fn oembed_curl_runner_reports_missing_curl() {
        let invoker = FixtureOEmbedCurlInvoker::error(ProcessRunError::Spawn(io::Error::new(
            io::ErrorKind::NotFound,
            "curl not found",
        )));

        let err = fetch_oembed_with_invoker("https://x.com/a/status/1", &invoker).unwrap_err();

        assert_eq!(err.line, None);
        assert!(
            err.message.contains("failed to start curl"),
            "{}",
            err.message
        );
        assert!(err.message.contains("curl not found"), "{}", err.message);
        assert!(err.help.contains("install curl"), "{}", err.help);
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
    fn process_runner_sees_completion_signal_written_just_before_exit() {
        // Regression: the exit path used to drain for a fixed 100ms, so a
        // completion signal still in flight when the child exited was judged
        // missing. Chrome does exactly this: it prints the lint payload and
        // exits immediately after.
        let args = [
            OsString::from("-c"),
            OsString::from(
                "printf 'noise\\n' >&2; printf '%s\\n' \"$(head -c 200000 /dev/zero | tr '\\0' 'x')\" >&2; printf 'SIGNAL-HERE\\n' >&2; exit 0",
            ),
        ];
        let mut state = 0usize;
        let outcome = run_child_with_timeout(
            Path::new("/bin/sh"),
            &args,
            None,
            Duration::from_secs(10),
            |_, stderr| scan_for_needle(stderr, &mut state, b"SIGNAL-HERE"),
        )
        .unwrap();

        let stderr = match outcome {
            ProcessOutcome::Ready { stderr, .. } => stderr,
            ProcessOutcome::Exited { status, stderr, .. } => {
                assert!(status.success());
                stderr
            }
            other => panic!("expected ready or exited process, got {other:?}"),
        };
        assert!(
            String::from_utf8_lossy(&stderr).contains("SIGNAL-HERE"),
            "completion signal must survive the exit-path drain"
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_runner_drains_output_arriving_after_the_parent_exits() {
        // Chrome leaves grandchildren holding stderr open, so the reader
        // threads never see EOF and the exit-path drain cannot rely on
        // disconnection. A grandchild that writes the completion signal after
        // the parent has exited stands in for the CI-loaded case where the
        // signal is still in flight: a fixed short drain window misses it.
        let args = [
            OsString::from("-c"),
            OsString::from("(sleep 1; printf 'TAIL-MARKER\\n' >&2; sleep 30) & exit 0"),
        ];
        let mut state = 0usize;
        let outcome = run_child_with_timeout(
            Path::new("/bin/sh"),
            &args,
            None,
            Duration::from_secs(10),
            |_, stderr| scan_for_needle(stderr, &mut state, b"TAIL-MARKER"),
        )
        .unwrap();

        let stderr = match outcome {
            ProcessOutcome::Ready { stderr, .. } => stderr,
            ProcessOutcome::Exited { stderr, .. } => stderr,
            other => panic!("expected ready or exited process, got {other:?}"),
        };
        assert!(
            String::from_utf8_lossy(&stderr).contains("TAIL-MARKER"),
            "completion signal arriving after parent exit must still be observed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_runner_exit_drain_does_not_wait_out_the_whole_timeout() {
        // A caller whose predicate never fires must not be made to sit through
        // the full timeout just because a grandchild holds the pipes open.
        let args = [
            OsString::from("-c"),
            OsString::from("sleep 30 & printf 'done\\n' >&2; exit 0"),
        ];
        let started = std::time::Instant::now();
        let outcome = run_child_with_timeout(
            Path::new("/bin/sh"),
            &args,
            None,
            Duration::from_secs(60),
            |_, _| false,
        )
        .unwrap();

        assert!(
            started.elapsed() < POST_EXIT_DRAIN_WINDOW + Duration::from_secs(3),
            "exit drain took {:?}, expected to be bounded by POST_EXIT_DRAIN_WINDOW",
            started.elapsed()
        );
        match outcome {
            ProcessOutcome::Exited { status, .. } => assert!(status.success()),
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
        let help = err.help().expect("help must be present").to_string();
        assert!(
            help.contains("lint measurement payload"),
            "actual help: {help}"
        );
        assert!(
            message.contains("JavaScript exploded"),
            "actual error: {message}"
        );
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
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("retry lint"), "actual help: {help}");
        assert!(help.contains("lint workspace"), "actual help: {help}");
        assert!(help.contains("lint.html"), "actual help: {help}");
        assert!(!help.contains("chrome-stderr.log"), "actual help: {help}");
        assert!(!help.contains("retry export"), "actual help: {help}");
        assert!(!help.contains("export command"), "actual help: {help}");
    }

    #[cfg(unix)]
    #[test]
    fn cdp_export_port_timeout_reports_stderr_and_reaps_chrome() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("pdf.html"), "<!doctype html>").unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        let pid_file = dir.path().join("fake-chrome.pid");
        write_script(
            &fake_chrome,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$$\" > '{}'\nprintf 'fake chrome never published DevToolsActivePort\\n' >&2\nexec sleep 30\n",
                pid_file.display()
            ),
        );
        fs::set_permissions(&fake_chrome, fs::Permissions::from_mode(0o755)).unwrap();

        let started = Instant::now();
        let err = run_chrome_print_with_timeout(
            &fake_chrome,
            &workspace,
            &dir.path().join("out.pdf"),
            Duration::from_secs(2),
        )
        .unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(10));
        let message = err.to_string();
        assert!(
            message.contains("Chrome PDF export failed"),
            "actual error: {message}"
        );
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("pdf.html"), "actual help: {help}");
        assert!(
            !message.contains("before ordered printing completed"),
            "actual error: {message}"
        );
        assert!(
            !message.contains("PDF flattening readiness must appear"),
            "actual error: {message}"
        );
        assert!(
            message.contains("stderr: fake chrome never published DevToolsActivePort"),
            "actual error: {message}"
        );

        let pid = fs::read_to_string(pid_file).unwrap();
        let status = std::process::Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(
            !status.success(),
            "fake Chrome process {} still exists",
            pid.trim()
        );
    }

    #[cfg(unix)]
    #[test]
    fn cdp_export_exited_before_port_reports_exit_and_reaps_chrome() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("pdf.html"), "<!doctype html>").unwrap();
        let fake_chrome = dir.path().join("fake-chrome");
        let pid_file = dir.path().join("fake-chrome.pid");
        write_script(
            &fake_chrome,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$$\" > '{}'\nprintf 'fake chrome exited before DevToolsActivePort\\n' >&2\nexit 23\n",
                pid_file.display()
            ),
        );
        fs::set_permissions(&fake_chrome, fs::Permissions::from_mode(0o755)).unwrap();

        let err = run_chrome_print_with_timeout(
            &fake_chrome,
            &workspace,
            &dir.path().join("out.pdf"),
            Duration::from_secs(10),
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("before publishing DevToolsActivePort"),
            "actual error: {message}"
        );
        assert!(
            !message.contains("timed out waiting for Chrome DevToolsActivePort"),
            "dead Chrome produced the port-timeout error instead of the exited-before-port error: {message}"
        );
        assert!(
            message.contains("fake chrome exited before DevToolsActivePort"),
            "actual error: {message}"
        );

        let pid = fs::read_to_string(pid_file).unwrap();
        let status = std::process::Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(
            !status.success(),
            "fake Chrome process {} still exists",
            pid.trim()
        );
    }

    #[cfg(unix)]
    #[test]
    fn cdp_post_print_shutdown_grace_reaps_lingering_chrome_without_failing() {
        let args = [OsString::from("-c"), OsString::from("exec sleep 30")];
        let mut process = CdpChromeProcess::spawn(Path::new("/bin/sh"), &args).unwrap();
        let pid = process.child.id();

        let started = Instant::now();
        best_effort_cdp_shutdown(&mut process, Duration::from_millis(100), |_| Ok(()));

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(process.reaped, "lingering Chrome child was not reaped");
        let status = std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "fake Chrome process {pid} still exists");
    }

    #[test]
    fn png_completion_accepts_zero_byte_write_signal() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("embed.png");
        fs::write(&out, []).unwrap();
        let completion = ChromeCompletion::PngWritten;
        let stderr = b"0 bytes written to file embed.png\n";

        let mut running_state = ChromeCompletionState::default();
        assert!(completion.is_ready(&[], stderr, &mut running_state));

        let mut exited_state = ChromeCompletionState::default();
        assert!(completion.is_ready_after_successful_exit(&[], stderr, &mut exited_state));
    }

    #[test]
    fn emit_present_cache_writes_present_json() {
        let fixture = WatchFixture::new("# Intro\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();

        fs::create_dir_all(&fixture.options.out).unwrap();
        emit_present_cache(&fixture.options.out, &artifacts, None, true, true).unwrap();

        let json = fs::read_to_string(fixture.options.out.join("present.json")).unwrap();
        assert!(json.contains(r#""presenterOpen": true"#));
        assert!(json.contains(r#""rehearsalAudio": true"#));
        assert_theme_fonts_written(&fixture.options.out);
    }

    #[test]
    fn emit_present_cache_disables_audio_without_explicit_audio_intent() {
        let fixture = WatchFixture::new("# Intro\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();

        for presenter_open in [false, true] {
            fs::create_dir_all(&fixture.options.out).unwrap();
            emit_present_cache(
                &fixture.options.out,
                &artifacts,
                None,
                presenter_open,
                false,
            )
            .unwrap();
            let json = fs::read_to_string(fixture.options.out.join("present.json")).unwrap();
            assert!(json.contains(r#""rehearsalAudio": false"#));
        }
    }

    #[test]
    fn emit_present_cache_does_not_write_rehearsal_json() {
        let fixture = WatchFixture::new("# Intro\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();

        fs::create_dir_all(&fixture.options.out).unwrap();
        emit_present_cache(&fixture.options.out, &artifacts, None, false, false).unwrap();

        assert!(!fixture.options.out.join("rehearsal.json").exists());
    }

    #[test]
    fn emit_present_cache_omits_sources_json() {
        let fixture = WatchFixture::new("# Intro\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();

        emit_present_cache(&fixture.options.out, &artifacts, None, false, false).unwrap();

        assert!(!fixture.options.out.join("sources.json").exists());
    }

    #[test]
    fn distribution_excludes_all_rehearsal_and_present_only_artifacts() {
        let fixture = WatchFixture::new("# Intro\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let out = fixture.options.out.clone();

        emit_distribution(&out, &artifacts).unwrap();

        assert!(!out.join(".peitho").exists());
        assert!(!out.join("present.json").exists());
        let names = recursively_list_file_names(&out);
        assert!(!names
            .iter()
            .any(|name| name.starts_with("rehearsal-") || name.ends_with(".webm")));
    }

    #[test]
    fn latest_rehearsal_record_is_none_for_missing_or_empty_dir() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(
            latest_rehearsal_record(&dir.path().join("missing")).unwrap(),
            None
        );
        fs::create_dir_all(dir.path().join("empty")).unwrap();
        assert_eq!(
            latest_rehearsal_record(&dir.path().join("empty")).unwrap(),
            None
        );
    }

    #[test]
    fn rehearsal_record_entry_error_has_recovery_help() {
        let dir = Path::new("deck/.peitho/rehearsals");
        let err =
            rehearsal_record_paths_entry_error(dir, std::io::Error::other("injected read error"));
        let message = err.to_string();

        assert!(message.contains("failed to read rehearsal records in deck/.peitho/rehearsals"));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("check permissions or move the rehearsals directory"));
        assert!(message.contains("caused by: injected read error"));
    }

    #[test]
    fn rehearsal_command_prints_latest_record_golden() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-135241.json"),
            v1_rehearsal_record(
                local_recorded_at_ms(2026, 7, 19, 13, 52, 41),
                122_100,
                vec![
                    peitho_core::RehearsalSection::new("Setup", 60_000, 52_000),
                    peitho_core::RehearsalSection::new("Problem", 60_000, 70_100),
                ],
            ),
        );
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: rehearsals,
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "\
rehearsal-20260719-135241  (recorded 2026-07-19 13:52)

  section    planned   actual    delta
  Setup         1:00     0:52    -0:08
  Problem       1:00     1:10    +0:10
  total         2:00     2:02    +0:02
"
        );
    }

    #[test]
    fn rehearsal_command_prints_v2_slide_totals_with_revisits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rehearsal-20260918-120000.json");
        write_rehearsal_record(
            &path,
            v2_single_section_record(
                local_recorded_at_ms(2026, 9, 18, 12, 0, 0),
                "Setup",
                30_000,
                &[
                    ("intro", 0, 0),
                    ("details", 1, 10_000),
                    ("intro", 0, 25_000),
                ],
            ),
        );
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();

        let output = String::from_utf8(stdout).unwrap();
        assert!(!output.contains("audio"));
        assert_eq!(
            output,
            "\
rehearsal-20260918-120000  (recorded 2026-09-18 12:00)

  section    planned   actual    delta
  Setup         0:30     0:30    -0:00
  total         0:30     0:30    -0:00

  slide   key       entered   visits   total
  #1      intro        0:00        2    0:15
  #2      details      0:10        1    0:15
  total                                 0:30
"
        );
    }

    #[test]
    fn rehearsal_command_seeks_from_a_first_visit_that_overlaps_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rehearsal-20260918-120000.json");
        let record = v2_single_section_record_with_audio(
            local_recorded_at_ms(2026, 9, 18, 12, 0, 0),
            "Setup",
            76_102,
            &[("cover", 0, 62_056), ("problem", 1, 70_054)],
            Some(peitho_core::RehearsalAudio::new(
                "rehearsal-20260918-120000.webm".to_owned(),
                62_159,
            )),
        );
        write_rehearsal_record(&path, record);
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();

        let stdout = String::from_utf8(stdout).unwrap();
        let audio_path = dir.path().join("rehearsal-20260918-120000.webm");
        assert_eq!(
            stdout,
            format!(
                "\
rehearsal-20260918-120000  (recorded 2026-09-18 12:00)

  section    planned   actual    delta
  Setup         1:16     1:16    -0:00
  total         1:16     1:16    -0:00

  slide   key       entered      seek   visits   total
  (before first entry)              -             1:02
  #1      cover        1:02      0:00        1    0:08
  #2      problem      1:10      0:08        1    0:06
  total                                           1:16
  audio   {}
  offset  1:02
note: audio file is missing: {}
",
                audio_path.display(),
                audio_path.display()
            )
        );
    }

    #[test]
    fn rehearsal_command_has_no_seek_when_the_first_visit_ended_before_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rehearsal-20260918-120000.json");
        let record = v2_single_section_record_with_audio(
            local_recorded_at_ms(2026, 9, 18, 12, 0, 0),
            "Setup",
            76_102,
            &[
                ("prelude", 0, 60_000),
                ("cover", 1, 62_056),
                ("problem", 2, 70_054),
                ("prelude", 0, 75_000),
            ],
            Some(peitho_core::RehearsalAudio::new(
                "rehearsal-20260918-120000.webm".to_owned(),
                62_159,
            )),
        );
        write_rehearsal_record(&path, record);
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();

        let stdout = String::from_utf8(stdout).unwrap();
        assert!(stdout.contains("  #1      prelude      1:00         -        2    0:03\n"));
        assert!(stdout.contains("  #2      cover        1:02      0:00        1    0:08\n"));
    }

    #[test]
    fn rehearsal_command_prints_only_offsets_that_round_to_at_least_one_second() {
        for (start_ms, expected_offset) in [(1, None), (499, None), (500, Some("0:01"))] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("rehearsal-20260918-120000.json");
            let record = v2_single_section_record_with_audio(
                local_recorded_at_ms(2026, 9, 18, 12, 0, 0),
                "Setup",
                30_000,
                &[("intro", 0, 0)],
                Some(peitho_core::RehearsalAudio::new(
                    "rehearsal-20260918-120000.webm".to_owned(),
                    start_ms,
                )),
            );
            write_rehearsal_record(&path, record);
            let mut stdout = Vec::new();

            run_rehearsal(
                RehearsalOptions {
                    all: false,
                    rehearsals_dir: dir.path().to_path_buf(),
                },
                &mut stdout,
                LabelStyle::PLAIN,
            )
            .unwrap();

            let stdout = String::from_utf8(stdout).unwrap();
            match expected_offset {
                Some(offset) => {
                    assert!(stdout.contains(&format!("  offset  {offset}\n")));
                    assert!(stdout.contains("   seek   "));
                }
                None => {
                    assert!(!stdout.contains("  offset  "));
                    assert!(!stdout.contains("   seek   "));
                }
            }
        }
    }

    #[test]
    fn rehearsal_command_rejects_audio_that_is_not_the_selected_records_sibling() {
        for audio in [
            "rehearsal-20260918-120001.webm",
            "../rehearsal-20260918-120000.webm",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("rehearsal-20260918-120000.json");
            let json = format!(
                r#"{{"version":2,"recordedAtMs":10,"elapsedMs":1000,"sections":[{{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}}],"timeline":[{{"key":"intro","index":0,"atMs":0}}],"audio":{{"file":"{audio}","startMs":0}}}}"#
            );
            fs::write(&path, json).unwrap();

            let err = read_rehearsal_record(&path).unwrap_err();
            let message = err.to_string();
            assert!(message.contains(&path.display().to_string()));
            assert!(message.contains("does not match its session filename"));
            assert!(err
                .help()
                .expect("help must be present")
                .to_string()
                .contains("delete or move"));
        }
    }

    #[test]
    fn rehearsal_command_accounts_for_leading_gap_and_three_visits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rehearsal-20260918-120000.json");
        let record = v2_single_section_record(
            local_recorded_at_ms(2026, 9, 18, 12, 0, 0),
            "Setup",
            40_000,
            &[
                ("intro", 0, 5_000),
                ("architecture", 1, 10_000),
                ("intro", 0, 20_000),
                ("architecture", 1, 25_000),
                ("intro", 0, 30_000),
            ],
        );
        let peitho_core::RehearsalRecord::V2(v2) = &record else {
            panic!("expected v2");
        };
        let totals = rehearsal_slide_totals(v2);
        let row_total =
            v2.timeline()[0].at_ms() + totals.iter().map(|total| total.total_ms).sum::<u64>();

        assert_eq!(totals[0].visits, 3);
        assert_eq!(row_total, v2.elapsed_ms());
        write_rehearsal_record(&path, record);
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "\
rehearsal-20260918-120000  (recorded 2026-09-18 12:00)

  section    planned   actual    delta
  Setup         0:40     0:40    -0:00
  total         0:40     0:40    -0:00

  slide   key            entered   visits   total
  (before first entry)                       0:05
  #1      intro             0:05        3    0:20
  #2      architecture      0:10        2    0:15
  total                                      0:40
"
        );
    }

    #[test]
    fn rehearsal_command_prints_zero_duration_and_empty_v2_timelines() {
        let dir = tempfile::tempdir().unwrap();
        let zero_path = dir.path().join("rehearsal-20260918-120000.json");
        write_rehearsal_record(
            &zero_path,
            v2_single_section_record(
                local_recorded_at_ms(2026, 9, 18, 12, 0, 0),
                "Zero",
                0,
                &[("intro", 0, 0), ("details", 1, 0)],
            ),
        );
        let reset_path = dir.path().join("rehearsal-20260918-120001.json");
        write_rehearsal_record(
            &reset_path,
            v2_single_section_record(local_recorded_at_ms(2026, 9, 18, 12, 0, 1), "Reset", 0, &[]),
        );
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: true,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();
        let stdout = String::from_utf8(stdout).unwrap();

        assert!(stdout.contains("  #1      intro        0:00        1    0:00\n"));
        assert!(stdout.contains("  #2      details      0:00        1    0:00\n"));
        assert!(stdout.contains("  total                                 0:00\n"));
        assert!(stdout.contains("  (no slide entries)\n"));
    }

    #[test]
    fn rehearsal_command_errors_on_invalid_v2_timeline_with_path() {
        let cases = [
            (
                r#"{"version":2,"recordedAtMs":10,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":500},{"key":"details","index":1,"atMs":499}]}"#,
                "rehearsal timeline positions must be non-decreasing",
            ),
            (
                r#"{"version":2,"recordedAtMs":10,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":1001}]}"#,
                "rehearsal timeline position 1001 exceeds elapsed time 1000",
            ),
        ];

        for (index, (json, expected)) in cases.into_iter().enumerate() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir
                .path()
                .join(format!("rehearsal-20260918-12000{index}.json"));
            fs::write(&path, json).unwrap();
            let mut stdout = Vec::new();

            let err = run_rehearsal(
                RehearsalOptions {
                    all: false,
                    rehearsals_dir: dir.path().to_path_buf(),
                },
                &mut stdout,
                LabelStyle::PLAIN,
            )
            .unwrap_err();
            let message = err.to_string();

            assert!(message.contains(expected), "actual error: {message}");
            assert!(message.contains(&path.display().to_string()));
            assert!(err
                .help()
                .expect("help must be present")
                .to_string()
                .contains("delete or move"));
        }
    }

    #[test]
    fn rehearsal_command_all_prints_mixed_v1_and_v2_records_in_order() {
        let dir = tempfile::tempdir().unwrap();
        write_rehearsal_record(
            &dir.path().join("rehearsal-20260918-120000.json"),
            single_section_record((2026, 9, 18, 12, 0, 0), "Legacy", 1_000),
        );
        write_rehearsal_record(
            &dir.path().join("rehearsal-20260918-120001.json"),
            v2_single_section_record(
                local_recorded_at_ms(2026, 9, 18, 12, 0, 1),
                "Current",
                2_000,
                &[("intro", 0, 0)],
            ),
        );
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: true,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();
        let stdout = String::from_utf8(stdout).unwrap();
        let legacy = stdout.find("rehearsal-20260918-120000").unwrap();
        let current = stdout.find("rehearsal-20260918-120001").unwrap();

        assert!(legacy < current);
        assert!(!stdout[legacy..current].contains("  slide"));
        assert!(stdout[current..].contains("  slide"));
    }

    #[test]
    fn rehearsal_record_selection_ignores_webm_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("rehearsal-20260918-120000.json");
        write_rehearsal_record(
            &json,
            single_section_record((2026, 9, 18, 12, 0, 0), "Setup", 1_000),
        );
        fs::write(
            dir.path().join("rehearsal-20260918-120000.webm"),
            b"not a record",
        )
        .unwrap();

        assert_eq!(
            rehearsal_record_paths_by_name(dir.path()).unwrap(),
            vec![json]
        );
    }

    #[test]
    fn rehearsal_command_prints_no_records_message() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: missing.clone(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            format!(
                "no rehearsal records in {}\nhelp: run peitho present --rehearsal deck.md to record one\n",
                missing.display()
            )
        );
    }

    #[test]
    fn rehearsal_command_default_uses_latest_record() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-090000.json"),
            single_section_record((2026, 7, 19, 9, 0, 0), "Old", 1_000),
        );
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-100000.json"),
            single_section_record((2026, 7, 19, 10, 0, 0), "New", 2_000),
        );
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: rehearsals,
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();
        let stdout = String::from_utf8(stdout).unwrap();

        assert!(stdout.contains("rehearsal-20260719-100000"));
        assert!(stdout.contains("New"));
        assert!(!stdout.contains("Old"));
    }

    #[test]
    fn rehearsal_command_all_prints_records_oldest_to_newest_with_blank_line() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-100000.json"),
            single_section_record((2026, 7, 19, 10, 0, 0), "New", 2_000),
        );
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-090000.json"),
            single_section_record((2026, 7, 19, 9, 0, 0), "Old", 1_000),
        );
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: true,
                rehearsals_dir: rehearsals,
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();
        let stdout = String::from_utf8(stdout).unwrap();

        assert!(
            stdout.find("rehearsal-20260719-090000").unwrap()
                < stdout.find("rehearsal-20260719-100000").unwrap()
        );
        assert!(stdout.contains("Old"));
        assert!(stdout.contains("\n\nrehearsal-20260719-100000"));
    }

    #[test]
    fn rehearsal_command_errors_on_corrupt_latest_record() {
        let dir = tempfile::tempdir().unwrap();
        write_rehearsal_record(
            &dir.path().join("rehearsal-20260719-090000.json"),
            single_section_record((2026, 7, 19, 9, 0, 0), "Old", 1_000),
        );
        let corrupt = dir.path().join("rehearsal-20260719-100000.json");
        fs::write(&corrupt, "not json").unwrap();
        let mut stdout = Vec::new();

        let err = run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("failed to parse rehearsal record"));
        assert!(message.contains(&corrupt.display().to_string()));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("delete or move"));
    }

    #[test]
    fn rehearsal_command_errors_on_invalid_v1_record_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let invalid = dir.path().join("rehearsal-20260719-100000.json");
        fs::write(
            &invalid,
            r#"{"version":1,"recordedAtMs":10,"elapsedMs":0,"sections":[]}"#,
        )
        .unwrap();
        let mut stdout = Vec::new();

        let err = run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("failed to parse rehearsal record"));
        assert!(message.contains("rehearsal sections must not be empty"));
        assert!(message.contains(&invalid.display().to_string()));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("delete or move"));
    }

    #[test]
    fn rehearsal_command_errors_on_out_of_range_recorded_timestamp_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rehearsal-20260719-135241.json");
        write_rehearsal_record(
            &path,
            v1_rehearsal_record(
                u64::try_from(i64::MAX).unwrap() + 1,
                1_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 1_000)],
            ),
        );
        let mut stdout = Vec::new();

        let err = run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("rehearsal record timestamp is outside the supported range"));
        assert!(message.contains(&path.display().to_string()));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains(&format!(
            "delete or move {} and run `peitho rehearsal` again",
            path.display()
        )));
    }

    #[test]
    fn rehearsal_command_formats_delta_rounding_edges() {
        let dir = tempfile::tempdir().unwrap();
        write_rehearsal_record(
            &dir.path().join("rehearsal-20260719-135241.json"),
            v1_rehearsal_record(
                local_recorded_at_ms(2026, 7, 19, 13, 52, 41),
                240_998,
                vec![
                    peitho_core::RehearsalSection::new("over-small", 60_000, 60_499),
                    peitho_core::RehearsalSection::new("under-small", 60_000, 59_900),
                    peitho_core::RehearsalSection::new("under-half", 60_000, 59_500),
                    peitho_core::RehearsalSection::new("over-half", 60_000, 60_500),
                    peitho_core::RehearsalSection::new("equal", 60_000, 60_000),
                ],
            ),
        );
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();
        let stdout = String::from_utf8(stdout).unwrap();
        let rows = stdout
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>())
            .collect::<Vec<_>>();

        assert!(rows.contains(&vec!["over-small", "1:00", "1:00", "-0:00"]));
        assert!(rows.contains(&vec!["under-small", "1:00", "1:00", "-0:00"]));
        assert!(rows.contains(&vec!["under-half", "1:00", "1:00", "-0:00"]));
        assert!(rows.contains(&vec!["over-half", "1:00", "1:01", "+0:01"]));
        assert!(rows.contains(&vec!["equal", "1:00", "1:00", "-0:00"]));
    }

    #[test]
    fn rehearsal_command_widens_section_name_column() {
        let dir = tempfile::tempdir().unwrap();
        write_rehearsal_record(
            &dir.path().join("rehearsal-20260719-135241.json"),
            v1_rehearsal_record(
                local_recorded_at_ms(2026, 7, 19, 13, 52, 41),
                60_000,
                vec![peitho_core::RehearsalSection::new(
                    "Very long section",
                    60_000,
                    60_000,
                )],
            ),
        );
        let mut stdout = Vec::new();

        run_rehearsal(
            RehearsalOptions {
                all: false,
                rehearsals_dir: dir.path().to_path_buf(),
            },
            &mut stdout,
            LabelStyle::PLAIN,
        )
        .unwrap();

        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("  Very long section       1:00     1:00    -0:00"));
    }

    #[test]
    fn latest_rehearsal_record_uses_latest_record_by_filename() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-090000.json"),
            v1_rehearsal_record(
                1_783_000_000_000,
                1_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 1_000)],
            ),
        );
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-100000.json"),
            v1_rehearsal_record(
                1_783_003_600_000,
                2_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 2_000)],
            ),
        );

        let (_, record) = latest_rehearsal_record(&rehearsals).unwrap().unwrap();

        assert_eq!(record.elapsed_ms(), 2_000);
    }

    #[test]
    fn latest_rehearsal_record_ignores_stray_json_files() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-090000.json"),
            v1_rehearsal_record(
                1_783_000_000_000,
                1_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 1_000)],
            ),
        );
        fs::write(rehearsals.join("zzz-notes.json"), "not json").unwrap();

        let (_, record) = latest_rehearsal_record(&rehearsals).unwrap().unwrap();

        assert_eq!(record.elapsed_ms(), 1_000);
    }

    #[test]
    fn latest_rehearsal_record_orders_unsuffixed_and_suffixed_same_stamp_numerically() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-090507.json"),
            v1_rehearsal_record(
                1_783_000_000_000,
                1_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 1_000)],
            ),
        );
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-090507-2.json"),
            v1_rehearsal_record(
                1_783_000_000_000,
                2_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 2_000)],
            ),
        );

        let (_, record) = latest_rehearsal_record(&rehearsals).unwrap().unwrap();

        assert_eq!(record.elapsed_ms(), 2_000);
    }

    #[test]
    fn latest_rehearsal_record_orders_collision_suffixes_numerically() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-090507-2.json"),
            v1_rehearsal_record(
                1_783_000_000_000,
                2_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 2_000)],
            ),
        );
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-090507-10.json"),
            v1_rehearsal_record(
                1_783_000_000_000,
                10_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 10_000)],
            ),
        );

        let (_, record) = latest_rehearsal_record(&rehearsals).unwrap().unwrap();

        assert_eq!(record.elapsed_ms(), 10_000);
    }

    #[test]
    fn latest_rehearsal_record_ignores_corrupt_older_record_when_latest_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        fs::write(
            rehearsals.join("rehearsal-20260719-090000.json"),
            "not json",
        )
        .unwrap();
        write_rehearsal_record(
            &rehearsals.join("rehearsal-20260719-100000.json"),
            v1_rehearsal_record(
                1_783_003_600_000,
                2_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 2_000)],
            ),
        );

        let (_, record) = latest_rehearsal_record(&rehearsals).unwrap().unwrap();

        assert_eq!(record.elapsed_ms(), 2_000);
    }

    #[test]
    fn latest_rehearsal_record_errors_on_corrupt_latest_record_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let older = dir.path().join("rehearsal-20260719-090000.json");
        write_rehearsal_record(
            &older,
            v1_rehearsal_record(
                1_783_000_000_000,
                1_000,
                vec![peitho_core::RehearsalSection::new("Setup", 60_000, 1_000)],
            ),
        );
        let path = dir.path().join("rehearsal-20260719-100000.json");
        fs::write(&path, "not json").unwrap();

        let err = latest_rehearsal_record(dir.path()).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("failed to parse rehearsal record"));
        assert!(message.contains(&path.display().to_string()));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("delete or move"));
    }

    #[test]
    fn latest_rehearsal_record_errors_on_future_version_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rehearsal-20260719-090000.json");
        fs::write(
            &path,
            r#"{"version":3,"recordedAtMs":1783000000000,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[]}"#,
        )
        .unwrap();

        let err = latest_rehearsal_record(dir.path()).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("unsupported rehearsal version"));
        assert!(message.contains(&path.display().to_string()));
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("delete or move"));
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
        emit_present_cache(&cache, &artifacts, None, false, false).unwrap();

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
        emit_present_cache(&cache, &artifacts, None, false, false).unwrap();

        assert_eq!(
            fs::read(cache.join("fonts/deck-font.woff2")).unwrap(),
            b"font bytes"
        );
    }

    #[test]
    fn present_cache_writes_katex_fonts_for_math_deck() {
        let fixture = WatchFixture::new("# Math\n\n```math\n\\frac{1}{2}\n```\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let cache = fixture._dir.path().join("present-cache");

        fs::create_dir_all(&cache).unwrap();
        emit_present_cache(&cache, &artifacts, None, false, false).unwrap();

        assert_eq!(
            fs::read(cache.join("katex-fonts/KaTeX_Main-Regular.woff2")).unwrap(),
            katex_font_bytes("KaTeX_Main-Regular.woff2")
        );
        assert!(fs::read_to_string(cache.join("peitho.css"))
            .unwrap()
            .contains("url(katex-fonts/"));
    }

    #[test]
    fn emit_preview_cache_writes_preview_only_files_in_generation_dir() {
        let fixture = WatchFixture::new(
            "<!-- {\"key\":\"intro\"} -->\n# Intro\n\nBody\n\n<!-- first note -->\n\n<!-- second note -->\n",
        );
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let cache = fixture._dir.path().join(".peitho/preview-cache");

        let generation_dir = emit_preview_cache_generation(&cache, 0, &artifacts).unwrap();

        assert_eq!(generation_dir, cache.join("build-0"));
        assert!(generation_dir.join("index.html").is_file());
        assert!(generation_dir.join("preview.js").is_file());
        assert!(generation_dir.join("peitho.css").is_file());
        assert!(generation_dir.join("manifest.json").is_file());
        assert!(generation_dir.join("slides/000-intro.html").is_file());
        assert!(fs::read_to_string(generation_dir.join("notes.json"))
            .unwrap()
            .contains("first note\\n\\nsecond note"));
        let sources: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(generation_dir.join("sources.json")).unwrap())
                .unwrap();
        assert_eq!(sources["version"], 1);
        assert_eq!(sources["sources"]["intro"], "# Intro\n\nBody");
        assert_eq!(sources["unavailable"], serde_json::json!({}));
        assert!(!generation_dir.join("present.html").exists());
        assert!(!generation_dir.join("presenter.html").exists());
        assert!(!generation_dir.join("present.json").exists());
        assert!(!generation_dir.join("shell.js").exists());
        assert!(!generation_dir.join("remote.html").exists());
        assert!(!generation_dir.join("remote.js").exists());
        assert_theme_fonts_written(&generation_dir);

        let index = fs::read_to_string(generation_dir.join("index.html")).unwrap();
        assert!(index.contains("./preview.js"));
        assert!(index.contains("mountPreviewShell"));
        assert!(index.contains("installPreviewKeyboard"));
        assert!(index.contains("installPreviewReload"));

        let unavailable_fixture = WatchFixture::new(
            "<!-- {\"key\":\"first\"} -->\n# First\n\nBody\rTail\n\n---\n\n<!-- {\"key\":\"second\"} -->\n# Second\n",
        );
        let unavailable_artifacts = build_artifacts(&unavailable_fixture.options.input).unwrap();
        let unavailable_cache = unavailable_fixture
            ._dir
            .path()
            .join(".peitho/preview-cache");

        let unavailable_generation =
            emit_preview_cache_generation(&unavailable_cache, 0, &unavailable_artifacts).unwrap();

        let unavailable: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(unavailable_generation.join("sources.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(unavailable["sources"], serde_json::json!({}));
        let unavailable = unavailable["unavailable"].as_object().unwrap();
        assert_eq!(unavailable.len(), 2);
        for key in ["first", "second"] {
            let reason = unavailable[key].as_str().unwrap();
            assert!(reason.contains("bare CR"), "{key}: {reason}");
            assert!(reason.contains("LF or CRLF"), "{key}: {reason}");
        }
        assert!(unavailable_generation
            .join("slides/000-first.html")
            .is_file());
        assert!(unavailable_generation
            .join("slides/001-second.html")
            .is_file());
    }

    #[test]
    fn preview_notes_json_omits_same_line_comment_delimiters() {
        let fixture = WatchFixture::new("# T\n\n<!-- a --> <!-- b -->\n<!-- note -->\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let cache = fixture._dir.path().join(".peitho/preview-cache");

        let generation_dir = emit_preview_cache_generation(&cache, 0, &artifacts).unwrap();
        let notes = fs::read_to_string(generation_dir.join("notes.json")).unwrap();

        assert!(!notes.contains("-->"), "{notes}");
    }

    #[test]
    fn preview_rejects_mixed_comment_text_before_writing_notes_json() {
        for source in [
            "# T\n\n<!-- a --> mid <!-- b -->\n",
            "# T\n\n<!-- a -->x<!-- b -->\n",
            "# T\n\n<!-- a --> trailing -->\n",
        ] {
            let fixture = WatchFixture::new(source);
            let error = match build_artifacts(&fixture.options.input) {
                Ok(_) => panic!("mixed comment text must not build"),
                Err(error) => error,
            };

            assert!(plain_diagnostic_text(&error).contains("unsupported construct 'html'"));
        }
    }

    #[test]
    fn build_artifacts_compute_slide_sources_for_both_annotation_modes() {
        let fixture = WatchFixture::new(
            "<!-- {\"key\":\"first\"} -->\n# First\n\nBody\rTail\n\n---\n\n<!-- {\"key\":\"second\"} -->\n# Second\n",
        );
        let off = build_artifacts_with_services(
            &fixture.options.input,
            &DeterministicSvgRunner,
            &DeterministicEmbedRenderer,
            &DeterministicOEmbedFetcher,
            peitho_core::EditAnnotations::Off,
        )
        .unwrap();
        let on = build_artifacts_with_services(
            &fixture.options.input,
            &DeterministicSvgRunner,
            &DeterministicEmbedRenderer,
            &DeterministicOEmbedFetcher,
            peitho_core::EditAnnotations::On,
        )
        .unwrap();

        assert_eq!(off.slide_sources_json, on.slide_sources_json);
    }

    #[test]
    fn slide_sources_keys_match_manifest_for_includes_draft_and_skip() {
        use std::collections::BTreeSet;

        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            dir.path().join("included.md"),
            "<!-- {\"key\":\"included\"} -->\n# Included\n",
        )
        .unwrap();
        fs::write(
            &deck,
            "<!-- {\"key\":\"intro\"} -->\n# Intro\n\n---\n\n<!-- {\"include\":\"included.md\"} -->\n\n---\n\n<!-- {\"key\":\"draft\",\"draft\":true} -->\n# Draft\n\n---\n\n<!-- {\"key\":\"skipped\",\"skip\":true} -->\n# Skipped\n",
        )
        .unwrap();
        let artifacts = build_artifacts(&deck).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&artifacts.manifest_json).unwrap();
        let sources: serde_json::Value =
            serde_json::from_str(&artifacts.slide_sources_json).unwrap();
        let manifest_keys = manifest["slides"]
            .as_array()
            .unwrap()
            .iter()
            .map(|slide| slide["key"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>();
        let source_keys = sources["sources"]
            .as_object()
            .unwrap()
            .keys()
            .chain(sources["unavailable"].as_object().unwrap().keys())
            .cloned()
            .collect::<BTreeSet<_>>();

        assert_eq!(source_keys, manifest_keys);
        assert_eq!(sources["unavailable"], serde_json::json!({}));
        assert_eq!(sources["sources"]["included"], "# Included");
        assert_eq!(sources["sources"]["skipped"], "# Skipped");
        assert_eq!(
            manifest_keys,
            BTreeSet::from([
                "included".to_owned(),
                "intro".to_owned(),
                "skipped".to_owned(),
            ])
        );
    }

    #[test]
    fn preview_cache_slide_fragments_contain_edit_annotations() {
        let fixture = WatchFixture::new("# Intro\n\nEditable **body**.\n");
        let cache = fixture._dir.path().join(".peitho/preview-cache");
        let mut stderr = Vec::new();

        let root = emit_initial_preview_root(&fixture.options.input, &cache, &mut stderr).unwrap();

        assert_eq!(root, cache.join("build-0"));
        assert!(stderr.is_empty());
        let slide = fs::read_to_string(root.join("slides/000-intro.html")).unwrap();
        for annotation in ["data-peitho-src", "data-peitho-md"] {
            assert!(
                slide.contains(annotation),
                "{annotation} missing from {slide}"
            );
        }
    }

    #[test]
    fn preview_cache_generation_writes_katex_fonts_for_math_deck() {
        let fixture = WatchFixture::new("# Math\n\n```math\n\\frac{1}{2}\n```\n");
        let artifacts = build_artifacts(&fixture.options.input).unwrap();
        let cache = fixture._dir.path().join(".peitho/preview-cache");

        let generation_dir = emit_preview_cache_generation(&cache, 0, &artifacts).unwrap();

        assert_eq!(
            fs::read(generation_dir.join("katex-fonts/KaTeX_Main-Regular.woff2")).unwrap(),
            katex_font_bytes("KaTeX_Main-Regular.woff2")
        );
        assert!(fs::read_to_string(generation_dir.join("peitho.css"))
            .unwrap()
            .contains("url(katex-fonts/"));
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
        let fixture = WatchFixture::new("# Intro\n\nEditable **body**.\n");
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
        let slide = fs::read_to_string(current_root.join("slides/000-intro.html")).unwrap();
        for annotation in ["data-peitho-src", "data-peitho-md"] {
            assert!(
                slide.contains(annotation),
                "{annotation} missing from {slide}"
            );
        }
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("rebuilt 1 slide(s)"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn preview_watch_rebuild_failure_keeps_root_generation_and_reports_build_error() {
        let fixture =
            WatchFixture::new("# Intro\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```");
        let cache = fixture._dir.path().join(".peitho/preview-cache");
        let existing_root = cache.join("build-4");
        fs::create_dir_all(&existing_root).unwrap();
        fs::write(existing_root.join("last-good.txt"), "last good generation").unwrap();
        let expected_diagnostic = match build_preview_artifacts(&fixture.options.input) {
            Ok(_) => panic!("fixture must fail to build"),
            Err(error) => plain_diagnostic_text(&error),
        };
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
        assert_eq!(
            &*server.events.borrow(),
            &[PreviewReloadEvent::BuildError(expected_diagnostic.clone())]
        );
        assert_eq!(
            server.build_error.borrow().as_deref(),
            Some(expected_diagnostic.as_str())
        );
        assert!(existing_root.is_dir());
        assert_eq!(
            fs::read_to_string(existing_root.join("last-good.txt")).unwrap(),
            "last good generation"
        );
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
    fn preview_watch_success_after_failure_swaps_broadcasts_and_clears_error() {
        let fixture = WatchFixture::new("# Intro\n\nRecovered body.\n");
        let cache = fixture._dir.path().join(".peitho/preview-cache");
        let previous_root = cache.join("build-4");
        fs::create_dir_all(&previous_root).unwrap();
        let server = RecordingPreviewReloadTarget::with_build_error(4, "previous failure");
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

        let current_root = cache.join("build-5");
        assert_eq!(
            &*server.events.borrow(),
            &[
                PreviewReloadEvent::Swap(current_root.clone()),
                PreviewReloadEvent::Broadcast
            ]
        );
        assert_eq!(server.generation(), 5);
        assert_eq!(*server.build_error.borrow(), None);
        assert!(current_root.join("index.html").is_file());
        assert!(current_root.join("manifest.json").is_file());
        assert!(current_root.join("slides/000-intro.html").is_file());
        assert!(previous_root.is_dir());
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("rebuilt 1 slide(s)"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn render_diagnostic_formats_deck_errors_with_prefix_and_help_block() {
        let build_error = peitho_core::BuildError::new(
            peitho_core::error::ErrorKind::Parse,
            None,
            "deck.md:3: broken deck".to_owned(),
            "fix the frontmatter".to_owned(),
        );
        let err = miette::Report::new(DeckDiagnostic::new(build_error));

        let rendered = render_diagnostic(&err);

        // The renderer lays the diagnostic out as an error block plus a help
        // block; plain `Display` on the report would collapse it into the
        // headline alone.
        assert!(
            rendered.contains("error: deck.md:3: broken deck"),
            "actual: {rendered}"
        );
        assert!(
            rendered.contains("help: fix the frontmatter"),
            "actual: {rendered}"
        );
        assert!(rendered.lines().count() > 1, "actual: {rendered}");
    }

    #[test]
    fn preview_error_page_keeps_ansi_escapes_out_of_html() {
        let fixture =
            WatchFixture::new("# Intro\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```");
        let cache = fixture._dir.path().join(".peitho/preview-cache");
        let mut stderr = Vec::new();

        emit_initial_preview_root(&fixture.options.input, &cache, &mut stderr).unwrap();

        // The terminal sink renders the full diagnostic, but the HTML sink must
        // receive the flat message so escape codes never reach the browser.
        let html =
            fs::read_to_string(preview_generation_dir(&cache, 0).join("index.html")).unwrap();
        assert!(!html.contains('\u{1b}'), "actual html: {html}");
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

        open_preview_browser_or_warn(
            "http://127.0.0.1:4321/",
            &mut stderr,
            LabelStyle::PLAIN,
            |_url| Err(miette::miette!("open failed")),
        )
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
    fn rehearsal_command_is_listed_in_help() {
        let mut command = Cli::command();
        let stdout = command.render_long_help().to_string();
        let lists_rehearsal_subcommand = stdout.lines().any(|line| {
            line.strip_prefix("  rehearsal")
                .and_then(|rest| rest.chars().next())
                .is_some_and(char::is_whitespace)
        });
        assert!(lists_rehearsal_subcommand, "actual stdout: {stdout}");
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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

        let message = err.to_string();
        assert!(
            message.contains("no-such-deck.md"),
            "actual error: {message}"
        );
        let help = err.help().expect("help must be present").to_string();
        assert!(help.contains("defaults to deck.md"), "actual help: {help}");
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
            | Command::Docs { .. }
            | Command::Completions { .. }
            | Command::Rehearsal { .. } => {
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
        assert!(
            stdout.contains("- footnotes") && stdout.contains("accepts=blocks arity=0..1"),
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
        assert!(json["layouts"][0]["slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|slot| slot["name"] == "footnotes"
                && slot["accepts"] == "blocks"
                && slot["arity"] == "0..1"));
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
        fs::write(layouts.join("image.html"), TEST_IMAGE_LAYOUT_HTML).unwrap();
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

    #[derive(Debug, PartialEq, Eq)]
    enum PreviewReloadEvent {
        Swap(PathBuf),
        Broadcast,
        BuildError(String),
    }

    struct RecordingPreviewReloadTarget {
        generation: Cell<u64>,
        build_error: RefCell<Option<String>>,
        active_root: RefCell<Option<PathBuf>>,
        events: RefCell<Vec<PreviewReloadEvent>>,
    }

    impl RecordingPreviewReloadTarget {
        fn new(generation: u64) -> Self {
            Self {
                generation: Cell::new(generation),
                build_error: RefCell::new(None),
                active_root: RefCell::new(None),
                events: RefCell::new(Vec::new()),
            }
        }

        fn with_build_error(generation: u64, error: &str) -> Self {
            Self {
                generation: Cell::new(generation),
                build_error: RefCell::new(Some(error.to_owned())),
                active_root: RefCell::new(None),
                events: RefCell::new(Vec::new()),
            }
        }
    }

    impl PreviewReloadTarget for RecordingPreviewReloadTarget {
        fn generation(&self) -> u64 {
            self.generation.get()
        }

        fn swap_root(&self, root: PathBuf) {
            *self.active_root.borrow_mut() = Some(root.clone());
            self.events
                .borrow_mut()
                .push(PreviewReloadEvent::Swap(root));
        }

        fn broadcast_reload(&self) -> u64 {
            let active_root = self.active_root.borrow();
            let root = active_root.as_ref().expect("root swapped before broadcast");
            assert!(root.join("index.html").is_file());
            assert!(root.join("manifest.json").is_file());
            assert!(root.join("slides").is_dir());
            let generation = self.generation.get() + 1;
            self.generation.set(generation);
            *self.build_error.borrow_mut() = None;
            self.events.borrow_mut().push(PreviewReloadEvent::Broadcast);
            generation
        }

        fn report_build_error(&self, error: String) -> u64 {
            *self.build_error.borrow_mut() = Some(error.clone());
            self.events
                .borrow_mut()
                .push(PreviewReloadEvent::BuildError(error));
            self.generation.get()
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

    fn katex_font_bytes(file_name: &str) -> &'static [u8] {
        peitho_core::MathAssets::katex()
            .fonts()
            .iter()
            .find(|font| font.file_name() == file_name)
            .map(peitho_core::MathFontAsset::bytes)
            .expect("expected embedded KaTeX font")
    }

    fn assert_theme_fonts_written(out: &Path) {
        for font in peitho_core::theme_fonts() {
            assert_eq!(
                fs::read(out.join("theme-fonts").join(font.file_name())).unwrap(),
                font.bytes()
            );
        }
    }

    fn write_rehearsal_record(path: &Path, record: peitho_core::RehearsalRecord) {
        fs::write(path, peitho_core::rehearsal_record_json(&record).unwrap()).unwrap();
    }

    fn recursively_list_file_names(root: &Path) -> Vec<String> {
        fn visit(dir: &Path, names: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    visit(&path, names);
                } else {
                    names.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }

        let mut names = Vec::new();
        visit(root, &mut names);
        names.sort();
        names
    }

    fn v1_rehearsal_record(
        recorded_at_ms: u64,
        elapsed_ms: u64,
        sections: Vec<peitho_core::RehearsalSection>,
    ) -> peitho_core::RehearsalRecord {
        peitho_core::RehearsalRecord::V1(
            peitho_core::RehearsalRecordV1::new(recorded_at_ms, elapsed_ms, sections).unwrap(),
        )
    }

    fn v2_single_section_record(
        recorded_at_ms: u64,
        name: &str,
        elapsed_ms: u64,
        timeline: &[(&str, u32, u64)],
    ) -> peitho_core::RehearsalRecord {
        v2_single_section_record_with_audio(recorded_at_ms, name, elapsed_ms, timeline, None)
    }

    fn v2_single_section_record_with_audio(
        recorded_at_ms: u64,
        name: &str,
        elapsed_ms: u64,
        timeline: &[(&str, u32, u64)],
        audio: Option<peitho_core::RehearsalAudio>,
    ) -> peitho_core::RehearsalRecord {
        let snapshot: peitho_core::RehearsalSnapshot = serde_json::from_value(serde_json::json!({
            "version": 2,
            "elapsedMs": elapsed_ms,
            "sections": [{
                "name": name,
                "plannedDurationMs": elapsed_ms,
                "actualMs": elapsed_ms
            }],
            "timeline": timeline
                .iter()
                .map(|(key, index, at_ms)| serde_json::json!({
                    "key": key,
                    "index": index,
                    "atMs": at_ms
                }))
                .collect::<Vec<_>>()
        }))
        .unwrap();
        peitho_core::RehearsalRecord::V2(
            peitho_core::RehearsalRecordV2::from_snapshot(recorded_at_ms, &snapshot, audio)
                .unwrap(),
        )
    }

    fn local_recorded_at_ms(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> u64 {
        chrono::Local
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .unwrap()
            .timestamp_millis()
            .try_into()
            .unwrap()
    }

    fn single_section_record(
        recorded_at: (i32, u32, u32, u32, u32, u32),
        name: &str,
        actual_ms: u64,
    ) -> peitho_core::RehearsalRecord {
        let (year, month, day, hour, minute, second) = recorded_at;
        v1_rehearsal_record(
            local_recorded_at_ms(year, month, day, hour, minute, second),
            actual_ms,
            vec![peitho_core::RehearsalSection::new(name, 60_000, actual_ms)],
        )
    }

    fn watch_state_with_fonts() -> (tempfile::TempDir, WatchState, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let deck = root.join("deck.md");
        let fonts = root.join("fonts");
        fs::create_dir_all(&fonts).unwrap();
        fs::write(&deck, "---\nfonts: ./fonts\n---\n# Intro\n").unwrap();
        let targets = resolve_watch_targets(&deck).unwrap();
        (
            dir,
            WatchState::new(deck, targets, LabelStyle::PLAIN),
            fonts,
        )
    }

    fn watch_state_for_fixture(fixture: &WatchFixture) -> WatchState {
        WatchState::new(
            fixture.options.input.clone(),
            fixture.targets.clone(),
            LabelStyle::PLAIN,
        )
    }

    fn run_watch_ticks<F>(
        state: &mut WatchState,
        count: usize,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
        rebuild: &mut F,
    ) where
        F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
    {
        for _ in 0..count {
            handle_watch_tick(state, stdout, stderr, rebuild).unwrap();
        }
    }

    fn empty_assets() -> ResolvedAssets {
        ResolvedAssets {
            layouts: Provenance::Builtin,
            css: Provenance::Builtin,
            overrides: Provenance::Absent,
            syntaxes: Provenance::Builtin,
            fonts: Provenance::Absent,
        }
    }

    fn workspace_root_for_tests() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .to_path_buf()
    }
}
