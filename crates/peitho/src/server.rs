use std::{
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex, OnceLock, RwLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{Local, NaiveDateTime};
use peitho_core::{
    domain::SlideKey, rehearsal_record_json, RehearsalRecord, RehearsalRecordV2, RehearsalSection,
    RehearsalSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::labels::LabelStyle;

static SERVER_CLOCK_START: OnceLock<Instant> = OnceLock::new();
static SYNC_SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);
const REMOTE_WEBMANIFEST: &str = r##"{"name":"Peitho Remote","short_name":"Remote","start_url":"/remote","display":"standalone","background_color":"#101216","theme_color":"#101216","icons":[{"src":"remote-icon.png","sizes":"180x180","type":"image/png"}]}"##;
const REMOTE_ICON_PNG: &[u8] = include_bytes!("../assets/remote-icon.png");
const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";

fn new_sync_session() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let count = SYNC_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{millis:x}-{count:x}")
}

#[derive(Debug)]
pub struct PresentServerBindError {
    addr: SocketAddr,
    source: io::Error,
}

impl PresentServerBindError {
    pub fn new(addr: SocketAddr, source: io::Error) -> Self {
        Self { addr, source }
    }

    fn from_boxed(addr: SocketAddr, source: Box<dyn Error + Send + Sync + 'static>) -> Self {
        match source.downcast::<io::Error>() {
            Ok(source) => Self::new(addr, *source),
            Err(source) => Self::new(addr, io::Error::other(source.to_string())),
        }
    }

    pub fn io_kind(&self) -> io::ErrorKind {
        self.source.kind()
    }
}

impl fmt::Display for PresentServerBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to bind present server at {}", self.addr)
    }
}

impl Error for PresentServerBindError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

impl miette::Diagnostic for PresentServerBindError {}

#[derive(Clone, Default)]
pub(crate) struct SyncHub {
    state: Arc<(Mutex<SyncState>, Condvar)>,
}

struct SyncState {
    seq: u64,
    latest: Option<String>,
    index: Option<usize>,
    step: Option<usize>,
    swapped: bool,
    timer: Option<TimerSyncState>,
    generation: u64,
    session: String,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            seq: 0,
            latest: None,
            index: None,
            step: None,
            swapped: false,
            timer: None,
            generation: 0,
            session: new_sync_session(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimerSyncState {
    running: bool,
    elapsed_ms: u64,
    at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncPoll {
    snapshot: SyncSnapshot,
    message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncSnapshot {
    seq: u64,
    index: Option<usize>,
    step: Option<usize>,
    swapped: bool,
    timer: Option<TimerSyncState>,
    generation: u64,
    session: String,
}

impl SyncHub {
    fn broadcast_sync_message(&self, message: &SyncMessage) -> u64 {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().expect("sync hub mutex");
        match message {
            SyncMessage::Index(message) => {
                state.index = Some(message.index);
                state.step = Some(message.step);
            }
            SyncMessage::Swap(message) => state.swapped = message.swapped,
            SyncMessage::Timer(message) => {
                state.timer = Some(TimerSyncState {
                    running: message.timer.running,
                    elapsed_ms: message.timer.elapsed_ms,
                    at_ms: server_clock_ms(),
                });
            }
            SyncMessage::Close(_) => {}
        }
        let json = serde_json::to_string(message).expect("SyncMessage serializes");
        state.seq += 1;
        state.latest = Some(json);
        let seq = state.seq;
        cvar.notify_all();
        seq
    }

    fn broadcast_reload(&self) -> u64 {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().expect("sync hub mutex");
        state.generation += 1;
        state.seq += 1;
        state.latest = None;
        let seq = state.seq;
        cvar.notify_all();
        seq
    }

    fn wait_after(&self, seq: u64, timeout: Duration) -> Option<SyncPoll> {
        let (lock, cvar) = &*self.state;
        let state = lock.lock().expect("sync hub mutex");
        let (state, _) = cvar
            .wait_timeout_while(state, timeout, |state| state.seq <= seq)
            .expect("sync hub mutex");
        if state.seq <= seq {
            return None;
        }
        Some(SyncPoll {
            snapshot: SyncSnapshot {
                seq: state.seq,
                index: state.index,
                step: state.step,
                swapped: state.swapped,
                timer: state.timer,
                generation: state.generation,
                session: state.session.clone(),
            },
            message: state.latest.clone(),
        })
    }

    fn snapshot(&self) -> SyncSnapshot {
        let (lock, _) = &*self.state;
        let state = lock.lock().expect("sync hub mutex");
        SyncSnapshot {
            seq: state.seq,
            index: state.index,
            step: state.step,
            swapped: state.swapped,
            timer: state.timer,
            generation: state.generation,
            session: state.session.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct PointerHub {
    state: Arc<(Mutex<PointerState>, Condvar)>,
}

#[derive(Debug, Clone, PartialEq)]
struct PointerState {
    seq: u64,
    event: PointerEvent,
    session: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PointerEvent {
    Move { x: f64, y: f64 },
    Up,
}

#[derive(Debug, Clone, PartialEq)]
struct PointerSnapshot {
    seq: u64,
    event: PointerEvent,
    session: String,
}

impl PointerHub {
    fn new(session: String) -> Self {
        Self {
            state: Arc::new((
                Mutex::new(PointerState {
                    seq: 0,
                    event: PointerEvent::Up,
                    session,
                }),
                Condvar::new(),
            )),
        }
    }

    fn reset_session(&self, session: String) -> u64 {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().expect("pointer hub mutex");
        if state.session == session {
            return state.seq;
        }
        state.seq += 1;
        state.event = PointerEvent::Up;
        state.session = session;
        let seq = state.seq;
        cvar.notify_all();
        seq
    }

    fn broadcast(&self, event: PointerEvent) -> u64 {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().expect("pointer hub mutex");
        state.seq += 1;
        state.event = event;
        let seq = state.seq;
        cvar.notify_all();
        seq
    }

    fn wait_after(&self, seq: u64, timeout: Duration) -> Option<PointerSnapshot> {
        let (lock, cvar) = &*self.state;
        let state = lock.lock().expect("pointer hub mutex");
        let (state, _) = cvar
            .wait_timeout_while(state, timeout, |state| state.seq <= seq)
            .expect("pointer hub mutex");
        if state.seq <= seq {
            return None;
        }
        Some(PointerSnapshot {
            seq: state.seq,
            event: state.event,
            session: state.session.clone(),
        })
    }

    fn snapshot(&self) -> PointerSnapshot {
        let (lock, _) = &*self.state;
        let state = lock.lock().expect("pointer hub mutex");
        PointerSnapshot {
            seq: state.seq,
            event: state.event,
            session: state.session.clone(),
        }
    }
}

pub(crate) fn resolve_request_path(
    root: &Path,
    url: &str,
    default_document: &str,
) -> Option<PathBuf> {
    let path = url.split('?').next().unwrap_or(url);
    if path.contains("://") {
        return None;
    }
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(root.join(default_document));
    }
    // Extensionless aliases keeping Chrome app names dot-free so app window
    // placement is saved and restored (see browser::presenter_url).
    match trimmed {
        "presenter" | "presenter-swapped" => return Some(root.join("presenter.html")),
        "present-swapped" => return Some(root.join("present.html")),
        "remote" => return Some(root.join("remote.html")),
        _ => {}
    }

    let mut out = root.to_path_buf();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

pub(crate) fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

/// Font directories are the only cacheable responses.
///
/// `peitho preview` reloads the whole document on every rebuild, and a new document starts
/// with an empty font set. Without a freshness header Chrome refetches every face over HTTP
/// before it can paint text in the real font, which reads as a stutter after the slides land.
/// Fonts are the one asset class a rebuild does not change, so they are safe to cache; every
/// other response (manifest, slide fragments, CSS, JS) MUST stay uncached or the watch loop
/// would serve stale content after a rebuild.
///
/// The filenames are author-controlled, not content-hashed, so this deliberately stops short
/// of `immutable`: a replaced font file must still be picked up without restarting preview.
/// A few minutes keeps every reload in an editing session off the network while bounding how
/// long a swapped face can be stale, and the reload is a normal navigation, so the cached
/// entry is used rather than force-revalidated.
const FONT_ASSET_DIRECTORIES: [&str; 3] = ["fonts", "theme-fonts", "katex-fonts"];
const FONT_FILE_EXTENSIONS: [&str; 4] = ["woff2", "woff", "ttf", "otf"];

/// Cache a font file anywhere under a font directory, at any depth.
///
/// Depth is not a property of a font. `fonts:` copies a directory verbatim, so how deeply a
/// face sits is decided by whatever the author vendored: fontsource ships subsetted families
/// as `fonts/<family>/files/<subset>.woff2`, three levels down, while a hand-placed face sits
/// directly in `theme-fonts/`. Matching only the flat shape left exactly the decks that need
/// caching most — a CJK deck refetching megabytes of `unicode-range` subsets on every
/// rebuild reload — without the header, and the refetch is what makes text visibly settle.
///
/// The extension check keeps the directory's non-font neighbours (`index.css`, licenses)
/// uncached, so the watch loop still serves fresh CSS.
fn cache_control(request_url: &str) -> Option<&'static str> {
    let path = request_url.split(['?', '#']).next().unwrap_or_default();
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    if !FONT_ASSET_DIRECTORIES.contains(&segments.next()?) {
        return None;
    }
    let file = segments.next_back()?;
    let (stem, extension) = file.rsplit_once('.')?;
    if stem.is_empty() || !FONT_FILE_EXTENSIONS.contains(&extension) {
        return None;
    }
    Some("public, max-age=300")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindPlan {
    LoopbackOnly,
    WildcardOnly(IpAddr),
    LoopbackPlusExtra(IpAddr),
}

fn bind_plan(host: Option<IpAddr>) -> BindPlan {
    match host {
        None => BindPlan::LoopbackOnly,
        Some(host) if host.is_unspecified() => BindPlan::WildcardOnly(host),
        Some(host) => BindPlan::LoopbackPlusExtra(host),
    }
}

#[derive(Clone)]
pub struct PresentServer {
    root: Arc<RwLock<PathBuf>>,
    default_document: String,
    serve_remote_assets: bool,
    rehearsal_sink: Option<Arc<RehearsalSink>>,
    notes_writer: Option<Arc<Mutex<NotesWriter>>>,
    server: Arc<Server>,
    listeners: Arc<Mutex<Vec<Arc<Server>>>>,
    sync: SyncHub,
    pointer: PointerHub,
}

pub type NotesWriter =
    Box<dyn FnMut(&SlideKey, &str) -> Result<(), NotesWriteError> + Send + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotesWriteError {
    Conflict(String),
    Unprocessable(String),
    Io(String),
}

#[derive(Debug)]
pub struct RehearsalSink {
    dir: PathBuf,
    expected: Vec<(String, u64)>,
    expected_slide_keys: Vec<SlideKey>,
    state: Mutex<RehearsalSinkState>,
}

#[derive(Debug, Default)]
struct RehearsalSinkState {
    session: Option<RehearsalSession>,
    last_rejection: Option<String>,
}

#[derive(Debug, Clone)]
struct RehearsalSession {
    path: PathBuf,
    recorded_at_ms: u64,
}

#[derive(Debug)]
struct ReservedRehearsalFile {
    path: PathBuf,
    file: fs::File,
}

#[derive(Debug)]
enum RehearsalWriteError {
    SectionMismatch,
    InvalidTimeline(String),
    Io(io::Error),
    Serialize(String),
}

impl fmt::Display for RehearsalWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SectionMismatch => write!(f, "rehearsal sections do not match this deck"),
            Self::InvalidTimeline(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Serialize(err) => write!(f, "{err}"),
        }
    }
}

impl RehearsalSink {
    pub fn new(
        dir: PathBuf,
        expected: Vec<(String, u64)>,
        expected_slide_keys: Vec<SlideKey>,
    ) -> Self {
        Self {
            dir,
            expected,
            expected_slide_keys,
            state: Mutex::new(RehearsalSinkState::default()),
        }
    }

    fn write_snapshot(
        &self,
        state: &mut RehearsalSinkState,
        snapshot: &RehearsalSnapshot,
    ) -> Result<(), RehearsalWriteError> {
        if !snapshot_matches_expected(snapshot.sections(), &self.expected) {
            return Err(RehearsalWriteError::SectionMismatch);
        }
        snapshot
            .validate_timeline()
            .map_err(RehearsalWriteError::InvalidTimeline)?;
        for entry in snapshot.timeline() {
            let Some(expected_key) = self.expected_slide_keys.get(entry.index() as usize) else {
                return Err(RehearsalWriteError::InvalidTimeline(format!(
                    "rehearsal timeline index {} is outside this deck",
                    entry.index()
                )));
            };
            if entry.key() != expected_key {
                return Err(RehearsalWriteError::InvalidTimeline(format!(
                    "rehearsal timeline key {} does not match slide {} key {}",
                    entry.key().as_str(),
                    entry.index(),
                    expected_key.as_str()
                )));
            }
        }

        if let Some(session) = state.session.as_ref() {
            let record = RehearsalRecord::V2(
                RehearsalRecordV2::from_snapshot(session.recorded_at_ms, snapshot)
                    .map_err(RehearsalWriteError::InvalidTimeline)?,
            );
            let json = rehearsal_record_json(&record)
                .map_err(|err| RehearsalWriteError::Serialize(err.to_string()))?;

            // Keep the sink mutex held through the atomic rewrite; it also
            // serializes concurrent POSTs that share this session path and temp file.
            return write_atomic(&session.path, json.as_bytes()).map_err(RehearsalWriteError::Io);
        }

        let recorded_at_ms = epoch_ms_now();
        let record = RehearsalRecord::V2(
            RehearsalRecordV2::from_snapshot(recorded_at_ms, snapshot)
                .map_err(RehearsalWriteError::InvalidTimeline)?,
        );
        let json = rehearsal_record_json(&record)
            .map_err(|err| RehearsalWriteError::Serialize(err.to_string()))?;
        let reserved = reserve_rehearsal_path(&self.dir, Local::now().naive_local())
            .map_err(RehearsalWriteError::Io)?;
        let path = match write_first_rehearsal_record(reserved, json.as_bytes(), |file, bytes| {
            file.write_all(bytes)
        }) {
            Ok(path) => path,
            Err(err) => return Err(RehearsalWriteError::Io(err)),
        };
        state.session = Some(RehearsalSession {
            path,
            recorded_at_ms,
        });
        Ok(())
    }
}

impl PresentServer {
    pub fn bind(root: PathBuf, port: u16, default_document: &'static str) -> miette::Result<Self> {
        Self::bind_with_remote_assets(root, port, default_document, None, false)
    }

    pub fn bind_with_remote_assets(
        root: PathBuf,
        port: u16,
        default_document: &'static str,
        host: Option<IpAddr>,
        serve_remote_assets: bool,
    ) -> miette::Result<Self> {
        match bind_plan(host) {
            BindPlan::LoopbackOnly => Self::bind_addr(
                root,
                SocketAddr::from(([127, 0, 0, 1], port)),
                default_document,
                serve_remote_assets,
            ),
            BindPlan::WildcardOnly(host) => Self::bind_addr(
                root,
                SocketAddr::new(host, port),
                default_document,
                serve_remote_assets,
            ),
            BindPlan::LoopbackPlusExtra(host) => {
                let server = Self::bind_addr(
                    root,
                    SocketAddr::from(([127, 0, 0, 1], port)),
                    default_document,
                    serve_remote_assets,
                )?;
                server.add_listener(host)?;
                Ok(server)
            }
        }
    }

    fn bind_addr(
        root: PathBuf,
        addr: SocketAddr,
        default_document: &'static str,
        serve_remote_assets: bool,
    ) -> miette::Result<Self> {
        let server = Server::http(addr)
            .map_err(|err| miette::Report::new(PresentServerBindError::from_boxed(addr, err)))?;
        let server = Arc::new(server);
        let sync = SyncHub::default();
        let pointer = PointerHub::new(sync.snapshot().session);
        Ok(Self {
            root: Arc::new(RwLock::new(root)),
            default_document: default_document.to_owned(),
            serve_remote_assets,
            rehearsal_sink: None,
            notes_writer: None,
            server: server.clone(),
            listeners: Arc::new(Mutex::new(vec![server])),
            sync,
            pointer,
        })
    }

    pub fn with_rehearsal_sink(mut self, sink: RehearsalSink) -> Self {
        self.rehearsal_sink = Some(Arc::new(sink));
        self
    }

    pub fn with_notes_writer(mut self, writer: NotesWriter) -> Self {
        self.notes_writer = Some(Arc::new(Mutex::new(writer)));
        self
    }

    fn add_listener(&self, host: IpAddr) -> miette::Result<SocketAddr> {
        validate_extra_listener_host(host)?;
        let addr = SocketAddr::new(host, self.addr().port());
        let server = Server::http(addr)
            .map_err(|err| miette::Report::new(PresentServerBindError::from_boxed(addr, err)))?;
        let server = Arc::new(server);
        let bound_addr = server
            .server_addr()
            .to_ip()
            .expect("present server binds TCP");
        self.listeners
            .lock()
            .expect("present server listeners mutex")
            .push(server);
        Ok(bound_addr)
    }

    pub fn addr(&self) -> SocketAddr {
        self.server
            .server_addr()
            .to_ip()
            .expect("present server binds TCP")
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/present.html", self.addr().port())
    }

    pub fn preview_url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.addr().port())
    }

    pub fn broadcast_reload(&self) -> u64 {
        self.sync.broadcast_reload()
    }

    pub fn generation(&self) -> u64 {
        self.sync.snapshot().generation
    }

    pub fn swap_root(&self, root: PathBuf) {
        *self.root.write().expect("present server root rwlock") = root;
    }

    pub fn serve_forever(self) -> miette::Result<()> {
        let listeners = self
            .listeners
            .lock()
            .expect("present server listeners mutex")
            .clone();
        let mut handles = Vec::new();
        for listener in listeners.iter().skip(1).cloned() {
            let server = self.clone();
            handles.push(thread::spawn(move || server.serve_listener(listener)));
        }
        self.serve_listener(self.server.clone());
        let mut listener_panicked = false;
        for handle in handles {
            if handle.join().is_err() {
                listener_panicked = true;
            }
        }
        if listener_panicked {
            return Err(miette::miette!("present server listener panicked"));
        }
        let _ = writeln!(std::io::stdout(), "presentation ended");
        Ok(())
    }

    pub fn handle_one(&self) {
        if let Some(request) = self.server.incoming_requests().next() {
            self.respond(request, None);
        }
    }

    fn serve_listener(&self, server: Arc<Server>) {
        for request in server.incoming_requests() {
            self.respond(request, Some(ShutdownHandle::new(self.listeners.clone())));
        }
    }

    fn respond(&self, request: tiny_http::Request, shutdown: Option<ShutdownHandle>) {
        let path = request.url().split('?').next().unwrap_or(request.url());
        match (request.method(), path) {
            (&Method::Get, "/sync") => {
                self.respond_sync_get(request);
                return;
            }
            (&Method::Post, "/sync") => {
                self.respond_sync_post(request, shutdown);
                return;
            }
            (&Method::Get, "/pointer") => {
                self.respond_pointer_get(request);
                return;
            }
            (&Method::Post, "/pointer") => {
                self.respond_pointer_post(request);
                return;
            }
            (&Method::Post, "/rehearsal") => {
                self.respond_rehearsal_post(request);
                return;
            }
            (&Method::Post, "/notes") => {
                self.respond_notes_post(request);
                return;
            }
            _ => {}
        }

        if request.method() != &Method::Get {
            send_response(request, Response::empty(StatusCode(405)));
            return;
        }
        if self.serve_remote_assets {
            match path {
                "/remote.webmanifest" => {
                    send_remote_webmanifest_response(request);
                    return;
                }
                "/remote-icon.png" => {
                    send_remote_icon_response(request);
                    return;
                }
                _ => {}
            }
        }
        self.respond_static(request);
    }

    fn respond_sync_get(&self, request: tiny_http::Request) {
        let Some(sync_get) = sync_get(request.url()) else {
            send_response(
                request,
                Response::from_string("invalid sync seq\n").with_status_code(StatusCode(400)),
            );
            return;
        };
        let SyncGet::Poll(seq) = sync_get else {
            send_json_response(request, sync_response_body(self.sync.snapshot(), None));
            return;
        };
        let sync = self.sync.clone();
        thread::spawn(
            move || match sync.wait_after(seq, Duration::from_secs(30)) {
                Some(event) => {
                    send_json_response(
                        request,
                        sync_response_body(event.snapshot, event.message.as_deref()),
                    );
                }
                None => send_response(request, Response::empty(StatusCode(204))),
            },
        );
    }

    fn respond_sync_post(&self, mut request: tiny_http::Request, shutdown: Option<ShutdownHandle>) {
        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            send_response(
                request,
                Response::from_string("invalid sync body\n").with_status_code(StatusCode(400)),
            );
            return;
        }
        let Ok(message) = serde_json::from_str::<SyncMessage>(&body) else {
            send_response(
                request,
                Response::from_string("invalid sync body\n").with_status_code(StatusCode(400)),
            );
            return;
        };
        if !message.is_valid() {
            send_response(
                request,
                Response::from_string("invalid sync body\n").with_status_code(StatusCode(400)),
            );
            return;
        }
        let seq = self.sync.broadcast_sync_message(&message);
        send_json_response(request, sync_post_response_body(seq));
        if matches!(message, SyncMessage::Close(_)) {
            if let Some(shutdown) = shutdown {
                shutdown.start();
            }
        }
    }

    fn respond_pointer_get(&self, request: tiny_http::Request) {
        let Some(pointer_get) = pointer_get(request.url()) else {
            send_response(
                request,
                Response::from_string("invalid pointer seq\n").with_status_code(StatusCode(400)),
            );
            return;
        };
        self.pointer.reset_session(self.sync.snapshot().session);
        let PointerGet::Poll(seq) = pointer_get else {
            send_json_response(
                request,
                pointer_handshake_response_body(self.pointer.snapshot()),
            );
            return;
        };
        let pointer = self.pointer.clone();
        thread::spawn(
            move || match pointer.wait_after(seq, Duration::from_secs(5)) {
                Some(snapshot) => send_json_response(request, pointer_poll_response_body(snapshot)),
                None => send_response(request, Response::empty(StatusCode(204))),
            },
        );
    }

    fn respond_pointer_post(&self, mut request: tiny_http::Request) {
        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            send_response(
                request,
                Response::from_string("invalid pointer body\n").with_status_code(StatusCode(400)),
            );
            return;
        }
        let Some(event) = pointer_event_from_body(&body) else {
            send_response(
                request,
                Response::from_string("invalid pointer body\n").with_status_code(StatusCode(400)),
            );
            return;
        };
        self.pointer.reset_session(self.sync.snapshot().session);
        let seq = self.pointer.broadcast(event);
        send_json_response(request, pointer_post_response_body(seq));
    }

    fn respond_rehearsal_post(&self, mut request: tiny_http::Request) {
        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            send_response(
                request,
                Response::from_string("invalid rehearsal body\n").with_status_code(StatusCode(400)),
            );
            return;
        }
        let outcome = rehearsal_post_outcome(self.rehearsal_sink.as_deref(), &body);
        if outcome.json {
            send_json_response(request, outcome.body);
            return;
        }
        send_response(
            request,
            Response::from_string(outcome.body).with_status_code(StatusCode(outcome.status)),
        );
    }

    fn respond_notes_post(&self, request: tiny_http::Request) {
        let Some(writer) = self.notes_writer.as_ref() else {
            send_response(
                request,
                Response::from_string("404\n").with_status_code(StatusCode(404)),
            );
            return;
        };
        if !notes_request_has_json_content_type(&request) {
            send_response(
                request,
                Response::from_string("invalid notes content type\n")
                    .with_status_code(StatusCode(400)),
            );
            return;
        }

        let writer = Arc::clone(writer);
        thread::spawn(move || {
            let mut request = request;
            let mut body = String::new();
            let notes = request
                .as_reader()
                .read_to_string(&mut body)
                .ok()
                .and_then(|_| serde_json::from_str::<NotesRequest>(&body).ok());
            let Some(notes) = notes else {
                send_response(
                    request,
                    Response::from_string("invalid notes body\n").with_status_code(StatusCode(400)),
                );
                return;
            };

            let result = {
                let mut writer = writer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                writer(&notes.key, &notes.text)
            };
            match result {
                Ok(()) => {
                    send_json_response(request, serde_json::json!({ "saved": true }).to_string())
                }
                Err(err) => {
                    let (status, message) = match err {
                        NotesWriteError::Conflict(message) => (409, message),
                        NotesWriteError::Unprocessable(message) => (422, message),
                        NotesWriteError::Io(message) => {
                            eprintln!("warning: failed to write speaker note: {message}");
                            (500, message)
                        }
                    };
                    send_json_response_with_status(
                        request,
                        status,
                        serde_json::json!({ "error": message }).to_string(),
                    );
                }
            }
        });
    }

    fn respond_static(&self, request: tiny_http::Request) {
        let root = self
            .root
            .read()
            .expect("present server root rwlock")
            .clone();
        let Some(path) = resolve_request_path(&root, request.url(), &self.default_document) else {
            send_response(
                request,
                Response::from_string("404\n").with_status_code(StatusCode(404)),
            );
            return;
        };
        match fs::read(&path) {
            Ok(bytes) => {
                let Ok(header) = Header::from_bytes("Content-Type", content_type(&path)) else {
                    eprintln!("warning: failed to build Content-Type header");
                    return;
                };
                let mut response = Response::from_data(bytes).with_header(header);
                if let Some(directive) = cache_control(request.url()) {
                    match Header::from_bytes("Cache-Control", directive) {
                        Ok(header) => response = response.with_header(header),
                        // A missing freshness header only costs a refetch, so serve it anyway.
                        Err(_) => eprintln!("warning: failed to build Cache-Control header"),
                    }
                }
                send_response(request, response);
            }
            Err(_) => {
                send_response(
                    request,
                    Response::from_string("404\n").with_status_code(StatusCode(404)),
                );
            }
        }
    }
}

fn notes_request_has_json_content_type(request: &tiny_http::Request) -> bool {
    request.headers().iter().any(|header| {
        header.field.equiv("Content-Type")
            && header
                .value
                .as_str()
                .split(';')
                .next()
                .is_some_and(|media_type| {
                    media_type.trim().eq_ignore_ascii_case("application/json")
                })
    })
}

fn validate_extra_listener_host(host: IpAddr) -> miette::Result<()> {
    if host.is_unspecified() {
        return Err(miette::miette!(
            help = "bind the wildcard as the primary listener",
            "extra listener must be specific"
        ));
    }
    Ok(())
}

struct ShutdownHandle {
    listeners: Arc<Mutex<Vec<Arc<Server>>>>,
}

impl ShutdownHandle {
    fn new(listeners: Arc<Mutex<Vec<Arc<Server>>>>) -> Self {
        Self { listeners }
    }

    fn start(self) {
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            let listeners = self
                .listeners
                .lock()
                .expect("present server listeners mutex")
                .clone();
            for server in listeners {
                server.unblock();
            }
        });
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum SyncMessage {
    Index(SyncIndexMessage),
    Swap(SyncSwapMessage),
    Timer(SyncTimerMessage),
    Close(SyncCloseMessage),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotesRequest {
    key: SlideKey,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncIndexMessage {
    index: usize,
    step: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncSwapMessage {
    swapped: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncTimerMessage {
    timer: SyncTimerPayload,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SyncTimerPayload {
    running: bool,
    elapsed_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncCloseMessage {
    close: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PointerMessage {
    Move(PointerMoveMessage),
    Up(PointerUpMessage),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerMoveMessage {
    #[serde(rename = "move")]
    move_: PointerMovePayload,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerMovePayload {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerUpMessage {
    up: bool,
}

impl SyncMessage {
    fn is_valid(&self) -> bool {
        match self {
            Self::Close(message) => message.close,
            Self::Index(_) | Self::Swap(_) | Self::Timer(_) => true,
        }
    }
}

impl PointerMessage {
    fn into_event(self) -> Option<PointerEvent> {
        match self {
            Self::Move(message) => {
                if !is_valid_pointer_coordinate(message.move_.x)
                    || !is_valid_pointer_coordinate(message.move_.y)
                {
                    return None;
                }
                Some(PointerEvent::Move {
                    x: message.move_.x,
                    y: message.move_.y,
                })
            }
            Self::Up(message) => message.up.then_some(PointerEvent::Up),
        }
    }
}

fn is_valid_pointer_coordinate(value: f64) -> bool {
    (0.0..=1.0).contains(&value)
}

fn pointer_event_from_body(body: &str) -> Option<PointerEvent> {
    serde_json::from_str::<PointerMessage>(body)
        .ok()?
        .into_event()
}

enum SyncGet {
    Handshake,
    Poll(u64),
}

enum PointerGet {
    Handshake,
    Poll(u64),
}

fn sync_get(url: &str) -> Option<SyncGet> {
    let Some((_, query)) = url.split_once('?') else {
        return Some(SyncGet::Handshake);
    };
    if query.is_empty() {
        return Some(SyncGet::Handshake);
    }
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "seq" {
            if value.is_empty() {
                return Some(SyncGet::Handshake);
            }
            return value.parse().ok().map(SyncGet::Poll);
        }
    }
    Some(SyncGet::Handshake)
}

fn pointer_get(url: &str) -> Option<PointerGet> {
    let Some((_, query)) = url.split_once('?') else {
        return Some(PointerGet::Handshake);
    };
    if query.is_empty() {
        return Some(PointerGet::Handshake);
    }
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "seq" {
            if value.is_empty() {
                return Some(PointerGet::Handshake);
            }
            return value.parse().ok().map(PointerGet::Poll);
        }
    }
    Some(PointerGet::Handshake)
}

fn sync_response_body(snapshot: SyncSnapshot, message: Option<&str>) -> String {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SyncResponseBody {
        seq: u64,
        message: Option<Value>,
        index: Option<usize>,
        step: Option<usize>,
        swapped: bool,
        generation: u64,
        session: String,
        timer: Option<TimerSyncState>,
        now_ms: u64,
    }

    let message = message
        .map(|message| serde_json::from_str(message).expect("sync message is serialized JSON"));
    serde_json::to_string(&SyncResponseBody {
        seq: snapshot.seq,
        message,
        index: snapshot.index,
        step: snapshot.step,
        swapped: snapshot.swapped,
        generation: snapshot.generation,
        session: snapshot.session,
        timer: snapshot.timer,
        now_ms: server_clock_ms(),
    })
    .expect("sync response serializes")
}

fn sync_post_response_body(seq: u64) -> String {
    #[derive(Serialize)]
    struct SyncPostResponseBody {
        seq: u64,
    }

    serde_json::to_string(&SyncPostResponseBody { seq }).expect("sync post response serializes")
}

fn pointer_handshake_response_body(snapshot: PointerSnapshot) -> String {
    #[derive(Serialize)]
    struct PointerHandshakeResponseBody {
        seq: u64,
        session: String,
    }

    serde_json::to_string(&PointerHandshakeResponseBody {
        seq: snapshot.seq,
        session: snapshot.session,
    })
    .expect("pointer handshake response serializes")
}

fn pointer_poll_response_body(snapshot: PointerSnapshot) -> String {
    #[derive(Serialize)]
    struct PointerPollResponseBody {
        seq: u64,
        event: Value,
        session: String,
    }

    let event = match snapshot.event {
        PointerEvent::Move { x, y } => serde_json::json!({ "move": { "x": x, "y": y } }),
        PointerEvent::Up => serde_json::json!({ "up": true }),
    };
    serde_json::to_string(&PointerPollResponseBody {
        seq: snapshot.seq,
        event,
        session: snapshot.session,
    })
    .expect("pointer poll response serializes")
}

fn pointer_post_response_body(seq: u64) -> String {
    #[derive(Serialize)]
    struct PointerPostResponseBody {
        seq: u64,
    }

    serde_json::to_string(&PointerPostResponseBody { seq })
        .expect("pointer post response serializes")
}

fn rehearsal_post_response_body(recorded: bool) -> String {
    #[derive(Serialize)]
    struct RehearsalPostResponseBody {
        recorded: bool,
    }

    serde_json::to_string(&RehearsalPostResponseBody { recorded })
        .expect("rehearsal post response serializes")
}

struct RehearsalPostOutcome {
    status: u16,
    body: String,
    json: bool,
}

fn rehearsal_post_outcome(sink: Option<&RehearsalSink>, body: &str) -> RehearsalPostOutcome {
    let style = LabelStyle::for_stderr();
    let mut stderr = io::stderr().lock();
    rehearsal_post_outcome_with_warnings(sink, body, &mut stderr, style)
}

fn rehearsal_post_outcome_with_warnings(
    sink: Option<&RehearsalSink>,
    body: &str,
    warnings: &mut dyn Write,
    style: LabelStyle,
) -> RehearsalPostOutcome {
    // Serialize validation, warning deduplication, and writes as one sink operation.
    let mut state = sink.map(|sink| sink.state.lock().expect("rehearsal sink mutex"));
    let snapshot = match serde_json::from_str::<RehearsalSnapshot>(body) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return rejected_rehearsal_outcome(
                state.as_deref_mut(),
                warnings,
                style,
                400,
                format!("invalid rehearsal body: {err}"),
            );
        }
    };
    if let Err(message) = snapshot.validate() {
        return rejected_rehearsal_outcome(
            state.as_deref_mut(),
            warnings,
            style,
            400,
            format!("invalid rehearsal snapshot: {message}"),
        );
    }
    let Some(sink) = sink else {
        return json_rehearsal_outcome(rehearsal_post_response_body(false));
    };
    let state = state.as_deref_mut().expect("rehearsal sink state");
    match sink.write_snapshot(state, &snapshot) {
        Ok(()) => state.last_rejection = None,
        Err(RehearsalWriteError::SectionMismatch) => {
            return rejected_rehearsal_outcome(
                Some(state),
                warnings,
                style,
                422,
                "rehearsal sections do not match this deck",
            );
        }
        Err(RehearsalWriteError::InvalidTimeline(message)) => {
            return rejected_rehearsal_outcome(Some(state), warnings, style, 422, message);
        }
        Err(err) => {
            write_rehearsal_rejection_warning(
                Some(state),
                warnings,
                style,
                &format!("failed to write rehearsal snapshot: {err}"),
            );
            return text_rehearsal_outcome(500, "failed to write rehearsal snapshot\n");
        }
    }
    json_rehearsal_outcome(rehearsal_post_response_body(true))
}

fn rejected_rehearsal_outcome(
    state: Option<&mut RehearsalSinkState>,
    warnings: &mut dyn Write,
    style: LabelStyle,
    status: u16,
    reason: impl Into<String>,
) -> RehearsalPostOutcome {
    let reason = reason.into();
    write_rehearsal_rejection_warning(
        state,
        warnings,
        style,
        &format!("rejected rehearsal snapshot: {reason}"),
    );
    text_rehearsal_outcome(status, format!("{reason}\n"))
}

fn write_rehearsal_rejection_warning(
    state: Option<&mut RehearsalSinkState>,
    warnings: &mut dyn Write,
    style: LabelStyle,
    message: &str,
) {
    let Some(state) = state else {
        return;
    };
    if state.last_rejection.as_deref() == Some(message) {
        return;
    }
    let _ = writeln!(warnings, "{}{message}", style.warning());
    state.last_rejection = Some(message.to_owned());
}

fn json_rehearsal_outcome(body: String) -> RehearsalPostOutcome {
    RehearsalPostOutcome {
        status: 200,
        body,
        json: true,
    }
}

fn text_rehearsal_outcome(status: u16, body: impl Into<String>) -> RehearsalPostOutcome {
    RehearsalPostOutcome {
        status,
        body: body.into(),
        json: false,
    }
}

fn snapshot_matches_expected(sections: &[RehearsalSection], expected: &[(String, u64)]) -> bool {
    sections.len() == expected.len()
        && sections
            .iter()
            .zip(expected)
            .all(|(section, (name, planned_duration_ms))| {
                section.name() == name && section.planned_duration_ms() == *planned_duration_ms
            })
}

fn format_rehearsal_filename(local: NaiveDateTime, suffix: u32) -> String {
    let stamp = local.format("%Y%m%d-%H%M%S");
    if suffix <= 1 {
        format!("rehearsal-{stamp}.json")
    } else {
        format!("rehearsal-{stamp}-{suffix}.json")
    }
}

/// Parses Peitho rehearsal record filenames for the CLI's baseline selection.
pub fn parse_rehearsal_filename(name: &str) -> Option<(String, u32)> {
    let stem = name.strip_prefix("rehearsal-")?.strip_suffix(".json")?;
    if is_rehearsal_stamp(stem) {
        return Some((stem.to_owned(), 1));
    }
    let (stamp, suffix) = stem.rsplit_once('-')?;
    if !is_rehearsal_stamp(stamp) || suffix.starts_with('0') {
        return None;
    }
    let suffix = suffix.parse::<u32>().ok()?;
    if suffix <= 1 {
        return None;
    }
    Some((stamp.to_owned(), suffix))
}

fn is_rehearsal_stamp(stamp: &str) -> bool {
    let bytes = stamp.as_bytes();
    bytes.len() == 15
        && bytes[0..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'-'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
}

fn reserve_rehearsal_path(dir: &Path, local: NaiveDateTime) -> io::Result<ReservedRehearsalFile> {
    fs::create_dir_all(dir)?;
    let mut suffix = 1;
    loop {
        let path = dir.join(format_rehearsal_filename(local, suffix));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok(ReservedRehearsalFile { path, file }),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                suffix += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

fn write_first_rehearsal_record<F>(
    reserved: ReservedRehearsalFile,
    bytes: &[u8],
    write_bytes: F,
) -> io::Result<PathBuf>
where
    F: FnOnce(&mut fs::File, &[u8]) -> io::Result<()>,
{
    let ReservedRehearsalFile { path, mut file } = reserved;
    let result = write_bytes(&mut file, bytes).and_then(|()| file.flush());
    if let Err(err) = result {
        drop(file);
        let _ = fs::remove_file(&path);
        // A SIGKILL during this first direct write can still leave a partial
        // file; that residual risk is accepted because startup validation names
        // the file to delete or move.
        return Err(first_rehearsal_write_error(&path, err));
    }
    Ok(path)
}

fn first_rehearsal_write_error(path: &Path, err: io::Error) -> io::Error {
    io::Error::new(
        err.kind(),
        format!(
            "failed to write first rehearsal record {}; if a partial file exists, delete or move it and run `peitho present` again; caused by: {err}",
            path.display()
        ),
    )
}

fn atomic_tmp_path(path: &Path) -> PathBuf {
    let mut file_name = path.file_name().unwrap_or_default().to_os_string();
    file_name.push(".tmp");
    path.with_file_name(file_name)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let (target, permissions) = if path.exists() {
        let target = fs::canonicalize(path)?;
        let permissions = fs::metadata(&target)?.permissions();
        (target, Some(permissions))
    } else {
        (path.to_path_buf(), None)
    };
    // Stage as `<file name>.tmp` next to the target. Orphaned tmp files (a crash
    // between write and rename) are not swept because sweeping could race an
    // in-flight rename; deck.md.tmp does not match the .md watch roots, and
    // rehearsal-X.json.tmp does not match the record scheme.
    let tmp = atomic_tmp_path(&target);
    let result = (|| {
        fs::write(&tmp, bytes)?;
        if let Some(permissions) = permissions {
            fs::set_permissions(&tmp, permissions)?;
        }
        fs::rename(&tmp, &target)
    })();
    if let Err(err) = result {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

fn epoch_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn server_clock_ms() -> u64 {
    let duration = SERVER_CLOCK_START.get_or_init(Instant::now).elapsed();
    u64::try_from(duration.as_millis()).expect("monotonic milliseconds fit in u64")
}

fn send_json_response(request: tiny_http::Request, body: String) {
    send_json_response_with_status(request, 200, body);
}

fn send_json_response_with_status(request: tiny_http::Request, status: u16, body: String) {
    send_bytes_response(request, status, JSON_CONTENT_TYPE, body.as_bytes());
}

fn send_remote_webmanifest_response(request: tiny_http::Request) {
    send_bytes_response(
        request,
        200,
        "application/manifest+json",
        REMOTE_WEBMANIFEST.as_bytes(),
    );
}

fn send_remote_icon_response(request: tiny_http::Request) {
    send_bytes_response(request, 200, "image/png", REMOTE_ICON_PNG);
}

fn send_bytes_response(request: tiny_http::Request, status: u16, content_type: &str, body: &[u8]) {
    let Ok(header) = Header::from_bytes("Content-Type", content_type) else {
        eprintln!("warning: failed to build Content-Type header");
        return;
    };
    send_response(
        request,
        Response::from_data(body.to_vec())
            .with_status_code(StatusCode(status))
            .with_header(header),
    );
}

fn send_response<R>(request: tiny_http::Request, response: Response<R>)
where
    R: Read + Send + 'static,
{
    if let Err(err) = request.respond(response) {
        eprintln!("warning: failed to send present server response: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{Shutdown, TcpStream},
        path::Path,
        sync::atomic::AtomicUsize,
        time::Duration,
    };

    #[test]
    fn resolves_root_to_configured_default_document() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/", "present.html").unwrap(),
            Path::new("/cache").join("present.html")
        );
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/", "index.html").unwrap(),
            Path::new("/cache").join("index.html")
        );
    }

    #[test]
    fn resolves_extensionless_presenter_route() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/presenter", "present.html").unwrap(),
            Path::new("/cache").join("presenter.html")
        );
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/presenter?seq=1", "present.html").unwrap(),
            Path::new("/cache").join("presenter.html")
        );
    }

    #[test]
    fn resolves_extensionless_remote_route() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/remote", "present.html").unwrap(),
            Path::new("/cache").join("remote.html")
        );
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/remote?seq=1", "present.html").unwrap(),
            Path::new("/cache").join("remote.html")
        );
    }

    #[test]
    fn bind_plan_defaults_to_loopback_only() {
        assert_eq!(bind_plan(None), BindPlan::LoopbackOnly);
    }

    #[test]
    fn bind_plan_uses_wildcard_only_for_unspecified_host() {
        assert_eq!(
            bind_plan(Some("0.0.0.0".parse().unwrap())),
            BindPlan::WildcardOnly("0.0.0.0".parse().unwrap())
        );
        assert_eq!(
            bind_plan(Some("::".parse().unwrap())),
            BindPlan::WildcardOnly("::".parse().unwrap())
        );
    }

    #[test]
    fn bind_plan_uses_loopback_plus_extra_for_specific_host() {
        assert_eq!(
            bind_plan(Some("100.64.0.5".parse().unwrap())),
            BindPlan::LoopbackPlusExtra("100.64.0.5".parse().unwrap())
        );
        assert_eq!(
            bind_plan(Some("::1".parse().unwrap())),
            BindPlan::LoopbackPlusExtra("::1".parse().unwrap())
        );
    }

    #[test]
    fn extra_listener_guard_rejects_unspecified_host() {
        let err = validate_extra_listener_host("0.0.0.0".parse().unwrap()).unwrap_err();

        assert!(err.to_string().contains("extra listener must be specific"));
        assert!(err
            .help()
            .expect("help must be present")
            .to_string()
            .contains("bind the wildcard as the primary listener"));
    }

    #[derive(Debug)]
    struct NonIoBindError;

    impl fmt::Display for NonIoBindError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "non-io bind failure")
        }
    }

    impl Error for NonIoBindError {}

    #[test]
    fn bind_error_fallback_preserves_message_without_chaining_same_error() {
        let err = PresentServerBindError::from_boxed(
            SocketAddr::from(([127, 0, 0, 1], 6173)),
            Box::new(NonIoBindError),
        );
        let source = err.source().unwrap();

        assert_eq!(err.io_kind(), io::ErrorKind::Other);
        assert_eq!(source.to_string(), "non-io bind failure");
        assert!(source.source().is_none());
    }

    #[test]
    fn resolves_swapped_routes_to_role_pages() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/present-swapped", "present.html").unwrap(),
            Path::new("/cache").join("present.html")
        );
        assert_eq!(
            resolve_request_path(
                Path::new("/cache"),
                "/present-swapped?seq=1",
                "present.html"
            )
            .unwrap(),
            Path::new("/cache").join("present.html")
        );
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/presenter-swapped", "present.html")
                .unwrap(),
            Path::new("/cache").join("presenter.html")
        );
        assert_eq!(
            resolve_request_path(
                Path::new("/cache"),
                "/presenter-swapped?seq=1",
                "present.html"
            )
            .unwrap(),
            Path::new("/cache").join("presenter.html")
        );
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(
            resolve_request_path(Path::new("/cache"), "/../manifest.json", "present.html")
                .is_none()
        );
        assert!(
            resolve_request_path(Path::new("/cache"), "/slides/../../secret", "present.html")
                .is_none()
        );
        assert!(resolve_request_path(
            Path::new("/cache"),
            "http://x/manifest.json",
            "present.html"
        )
        .is_none());
    }

    #[test]
    fn caches_font_files_at_any_depth() {
        // Fonts are the one asset a rebuild does not change, so only these may be cached.
        for url in [
            "/fonts/Custom.woff2",
            "/theme-fonts/Inter-Regular.woff2",
            "/katex-fonts/KaTeX_Main-Regular.woff2",
            "/theme-fonts/Inter-Regular.woff2?v=2",
            "/fonts/Custom.woff",
            "/fonts/Custom.ttf",
            "/fonts/Custom.otf",
            // How deeply a face sits is the author's vendoring, not a property of the font:
            // fontsource ships subsetted CJK families exactly like this.
            "/fonts/noto-sans-jp/files/noto-sans-jp-34-wght-normal.woff2",
            "/fonts/nested/Custom.woff2",
        ] {
            assert_eq!(cache_control(url), Some("public, max-age=300"), "{url}");
        }

        // Everything the watch loop must re-read after a rebuild stays uncached,
        // including a font directory's own non-font neighbours.
        for url in [
            "/manifest.json",
            "/notes.json",
            "/peitho.css",
            "/preview.js",
            "/index.html",
            "/slides/slide-1.html",
            "/",
            "/fonts",
            "/fonts/",
            "/fonts/noto-sans-jp/index.css",
            "/fonts/LICENSE",
            "/fonts/.woff2",
            "/img/theme-fonts/Inter-Regular.woff2",
        ] {
            assert_eq!(cache_control(url), None, "{url}");
        }
    }

    #[test]
    fn maps_font_content_types() {
        assert_eq!(
            content_type(Path::new("theme-fonts/Inter.woff2")),
            "font/woff2"
        );
        assert_eq!(content_type(Path::new("fonts/Custom.woff")), "font/woff");
        assert_eq!(content_type(Path::new("fonts/Custom.ttf")), "font/ttf");
        assert_eq!(content_type(Path::new("fonts/Custom.otf")), "font/otf");
    }

    #[test]
    fn maps_content_types() {
        assert_eq!(
            content_type(Path::new("present.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("peitho.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("shell.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("manifest.json")),
            "application/json; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("assets/diagram.svg")),
            "image/svg+xml"
        );
        assert_eq!(content_type(Path::new("assets/photo.png")), "image/png");
        assert_eq!(content_type(Path::new("assets/photo.jpg")), "image/jpeg");
        assert_eq!(content_type(Path::new("assets/photo.jpeg")), "image/jpeg");
        assert_eq!(content_type(Path::new("assets/animation.gif")), "image/gif");
        assert_eq!(content_type(Path::new("assets/diagram.webp")), "image/webp");
        assert_eq!(
            content_type(Path::new("slide.bin")),
            "application/octet-stream"
        );
    }

    #[test]
    fn sync_hub_returns_latest_message_after_requested_sequence() {
        let hub = SyncHub::default();
        let session = hub.snapshot().session;

        let message = SyncMessage::Index(SyncIndexMessage { index: 2, step: 0 });
        let seq = hub.broadcast_sync_message(&message);

        assert_eq!(seq, 1);
        assert_eq!(
            hub.wait_after(0, Duration::from_secs(1)).unwrap(),
            SyncPoll {
                snapshot: SyncSnapshot {
                    seq: 1,
                    index: Some(2),
                    step: Some(0),
                    swapped: false,
                    timer: None,
                    generation: 0,
                    session
                },
                message: Some(r#"{"index":2,"step":0}"#.to_owned())
            }
        );
        assert!(hub.wait_after(1, Duration::from_millis(1)).is_none());
    }

    #[test]
    fn sync_hub_stores_index_and_step_atomically() {
        let hub = SyncHub::default();
        let session = hub.snapshot().session;

        let message = SyncMessage::Index(SyncIndexMessage { index: 2, step: 3 });
        let seq = hub.broadcast_sync_message(&message);

        assert_eq!(seq, 1);
        assert_eq!(
            hub.wait_after(0, Duration::from_secs(1)).unwrap(),
            SyncPoll {
                snapshot: SyncSnapshot {
                    seq: 1,
                    index: Some(2),
                    step: Some(3),
                    swapped: false,
                    timer: None,
                    generation: 0,
                    session
                },
                message: Some(r#"{"index":2,"step":3}"#.to_owned())
            }
        );
    }

    #[test]
    fn sync_hub_stores_and_replays_timer_state() {
        let hub = SyncHub::default();

        let message = SyncMessage::Timer(SyncTimerMessage {
            timer: SyncTimerPayload {
                running: true,
                elapsed_ms: 12_345,
            },
        });
        let seq = hub.broadcast_sync_message(&message);

        assert_eq!(seq, 1);
        let poll = hub.wait_after(0, Duration::from_secs(1)).unwrap();
        assert_eq!(
            poll.snapshot
                .timer
                .map(|timer| (timer.running, timer.elapsed_ms)),
            Some((true, 12_345))
        );
        assert!(poll.snapshot.timer.unwrap().at_ms <= server_clock_ms());
        assert_eq!(
            poll.message,
            Some(r#"{"timer":{"running":true,"elapsedMs":12345}}"#.to_owned())
        );
        assert_eq!(
            hub.snapshot()
                .timer
                .map(|timer| (timer.running, timer.elapsed_ms)),
            Some((true, 12_345))
        );
    }

    #[test]
    fn sync_hub_coalesces_to_latest_absolute_timer_state() {
        let hub = SyncHub::default();

        hub.broadcast_sync_message(&SyncMessage::Timer(SyncTimerMessage {
            timer: SyncTimerPayload {
                running: true,
                elapsed_ms: 1_000,
            },
        }));
        hub.broadcast_sync_message(&SyncMessage::Timer(SyncTimerMessage {
            timer: SyncTimerPayload {
                running: false,
                elapsed_ms: 4_000,
            },
        }));

        let poll = hub.wait_after(0, Duration::from_secs(1)).unwrap();
        assert_eq!(poll.snapshot.seq, 2);
        assert_eq!(
            poll.snapshot
                .timer
                .map(|timer| (timer.running, timer.elapsed_ms)),
            Some((false, 4_000))
        );
        assert_eq!(
            poll.message,
            Some(r#"{"timer":{"running":false,"elapsedMs":4000}}"#.to_owned())
        );
    }

    #[test]
    fn sync_hub_broadcast_reload_advances_generation_without_transient_message() {
        let hub = SyncHub::default();
        let session = hub.snapshot().session;

        let seq = hub.broadcast_reload();

        assert_eq!(seq, 1);
        assert_eq!(
            hub.wait_after(0, Duration::from_secs(1)).unwrap(),
            SyncPoll {
                snapshot: SyncSnapshot {
                    seq: 1,
                    index: None,
                    step: None,
                    swapped: false,
                    timer: None,
                    generation: 1,
                    session: session.clone()
                },
                message: None
            }
        );
        assert_eq!(
            hub.snapshot(),
            SyncSnapshot {
                seq: 1,
                index: None,
                step: None,
                swapped: false,
                timer: None,
                generation: 1,
                session
            }
        );
    }

    #[test]
    fn sync_hub_session_is_stable_across_snapshots_and_polls() {
        let hub = SyncHub::default();
        let session = hub.snapshot().session;

        hub.broadcast_sync_message(&SyncMessage::Index(SyncIndexMessage { index: 1, step: 0 }));
        let poll = hub.wait_after(0, Duration::from_secs(1)).unwrap();

        assert_eq!(poll.snapshot.session, session);
        assert_eq!(hub.snapshot().session, session);
    }

    #[test]
    fn sync_hub_generates_distinct_sessions_for_distinct_hubs() {
        let first = SyncHub::default();
        let second = SyncHub::default();

        assert_ne!(first.snapshot().session, second.snapshot().session);
    }

    #[test]
    fn pointer_hub_sequence_is_monotonic() {
        let hub = PointerHub::new("session-a".to_owned());

        assert_eq!(hub.broadcast(PointerEvent::Move { x: 0.25, y: 0.5 }), 1);
        assert_eq!(hub.broadcast(PointerEvent::Up), 2);
        assert_eq!(hub.broadcast(PointerEvent::Move { x: 1.0, y: 0.0 }), 3);
        assert_eq!(hub.snapshot().seq, 3);
    }

    #[test]
    fn pointer_hub_up_is_sticky_until_next_move() {
        let hub = PointerHub::new("session-a".to_owned());

        assert_eq!(hub.snapshot().event, PointerEvent::Up);
        hub.broadcast(PointerEvent::Move { x: 0.2, y: 0.8 });
        hub.broadcast(PointerEvent::Up);

        assert_eq!(hub.snapshot().event, PointerEvent::Up);
        assert_eq!(
            hub.wait_after(1, Duration::from_secs(1)).unwrap(),
            PointerSnapshot {
                seq: 2,
                event: PointerEvent::Up,
                session: "session-a".to_owned()
            }
        );

        hub.broadcast(PointerEvent::Move { x: 0.4, y: 0.6 });
        assert_eq!(hub.snapshot().event, PointerEvent::Move { x: 0.4, y: 0.6 });
    }

    #[test]
    fn pointer_hub_new_session_resets_state_to_up() {
        let hub = PointerHub::new("session-a".to_owned());
        hub.broadcast(PointerEvent::Move { x: 0.2, y: 0.8 });

        assert_eq!(hub.reset_session("session-b".to_owned()), 2);

        assert_eq!(
            hub.snapshot(),
            PointerSnapshot {
                seq: 2,
                event: PointerEvent::Up,
                session: "session-b".to_owned()
            }
        );
        assert_eq!(
            hub.wait_after(1, Duration::from_secs(1)).unwrap().event,
            PointerEvent::Up
        );
    }

    #[test]
    fn pointer_coordinate_validation_accepts_only_unit_range() {
        assert_eq!(
            pointer_event_from_body(r#"{"move":{"x":0,"y":1}}"#).unwrap(),
            PointerEvent::Move { x: 0.0, y: 1.0 }
        );
        assert_eq!(
            pointer_event_from_body(r#"{"move":{"x":1,"y":0}}"#).unwrap(),
            PointerEvent::Move { x: 1.0, y: 0.0 }
        );
        assert!(pointer_event_from_body(r#"{"move":{"x":-0.01,"y":0.5}}"#).is_none());
        assert!(pointer_event_from_body(r#"{"move":{"x":0.5,"y":1.01}}"#).is_none());
    }

    #[test]
    fn pointer_unknown_json_is_rejected() {
        assert!(pointer_event_from_body("{}").is_none());
        assert!(pointer_event_from_body(r#"{"draw":true}"#).is_none());
        assert!(pointer_event_from_body(r#"{"up":false}"#).is_none());
        assert!(pointer_event_from_body(r#"{"move":{"x":0.5,"y":0.5},"up":true}"#).is_none());
        assert!(pointer_event_from_body(r#"{"move":{"x":0.5,"y":0.5,"z":0}}"#).is_none());
    }

    #[test]
    fn pointer_wait_after_returns_immediately_when_sequence_advanced() {
        let hub = PointerHub::new("session-a".to_owned());
        hub.broadcast(PointerEvent::Move { x: 0.2, y: 0.8 });

        let poll = hub.wait_after(0, Duration::from_secs(1)).unwrap();

        assert_eq!(poll.seq, 1);
        assert_eq!(poll.event, PointerEvent::Move { x: 0.2, y: 0.8 });
    }

    #[test]
    fn pointer_wait_after_times_out_when_nothing_changes() {
        let hub = PointerHub::new("session-a".to_owned());

        assert!(hub.wait_after(0, Duration::from_millis(1)).is_none());
    }

    #[test]
    fn sync_response_body_always_includes_generation() {
        let body = sync_response_body(
            SyncSnapshot {
                seq: 4,
                index: Some(2),
                step: Some(1),
                swapped: true,
                timer: Some(TimerSyncState {
                    running: true,
                    elapsed_ms: 12_000,
                    at_ms: 98_000,
                }),
                generation: 9,
                session: "session-a".to_owned(),
            },
            None,
        );

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(body.contains(r#""generation":9"#));
        assert!(body.contains(r#""session":"session-a""#));
        assert!(body.contains(r#""message":null"#));
        assert!(body.contains(r#""index":2"#));
        assert!(body.contains(r#""step":1"#));
        assert!(body.contains(r#""swapped":true"#));
        assert_eq!(json["timer"]["running"], true);
        assert_eq!(json["timer"]["elapsedMs"], 12_000);
        assert_eq!(json["timer"]["atMs"], 98_000);
        assert!(json["nowMs"].as_u64().unwrap() < 24 * 60 * 60 * 1000);
    }

    #[test]
    fn sync_response_body_includes_step() {
        let body = sync_response_body(
            SyncSnapshot {
                seq: 4,
                index: Some(2),
                step: Some(1),
                swapped: false,
                timer: None,
                generation: 0,
                session: "session-a".to_owned(),
            },
            None,
        );

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["index"], 2);
        assert_eq!(json["step"], 1);
    }

    #[test]
    fn sync_response_body_includes_session() {
        let body = sync_response_body(
            SyncSnapshot {
                seq: 4,
                index: None,
                step: None,
                swapped: false,
                timer: None,
                generation: 0,
                session: "session-body".to_owned(),
            },
            None,
        );

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["session"], "session-body");
    }

    #[test]
    fn sync_response_body_preserves_message_as_json() {
        let body = sync_response_body(
            SyncSnapshot {
                seq: 7,
                index: Some(2),
                step: Some(0),
                swapped: false,
                timer: None,
                generation: 0,
                session: "session-a".to_owned(),
            },
            Some(r#"{"close":true}"#),
        );

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["message"], serde_json::json!({"close": true}));
    }

    #[test]
    fn sync_post_response_body_carries_assigned_sequence() {
        let body = sync_post_response_body(42);

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["seq"], 42);
    }

    #[test]
    fn pointer_response_bodies_match_transport_contract() {
        let handshake = pointer_handshake_response_body(PointerSnapshot {
            seq: 4,
            event: PointerEvent::Up,
            session: "session-a".to_owned(),
        });
        let moved = pointer_poll_response_body(PointerSnapshot {
            seq: 5,
            event: PointerEvent::Move { x: 0.42, y: 0.71 },
            session: "session-a".to_owned(),
        });
        let up = pointer_poll_response_body(PointerSnapshot {
            seq: 6,
            event: PointerEvent::Up,
            session: "session-a".to_owned(),
        });

        assert_eq!(
            serde_json::from_str::<Value>(&handshake).unwrap(),
            serde_json::json!({"seq":4,"session":"session-a"})
        );
        assert_eq!(
            serde_json::from_str::<Value>(&moved).unwrap(),
            serde_json::json!({"seq":5,"event":{"move":{"x":0.42,"y":0.71}},"session":"session-a"})
        );
        assert_eq!(
            serde_json::from_str::<Value>(&up).unwrap(),
            serde_json::json!({"seq":6,"event":{"up":true},"session":"session-a"})
        );
    }

    #[test]
    fn reload_is_not_accepted_as_a_posted_sync_message() {
        assert!(serde_json::from_str::<SyncMessage>(r#"{"reload":true}"#).is_err());
    }

    #[test]
    fn bare_index_sync_messages_are_rejected() {
        assert!(serde_json::from_str::<SyncMessage>(r#"{"index":2}"#).is_err());
    }

    #[test]
    fn invalid_timer_sync_messages_are_rejected() {
        assert!(serde_json::from_str::<SyncMessage>(r#"{"timer":{"running":true}}"#).is_err());
        assert!(serde_json::from_str::<SyncMessage>(
            r#"{"timer":{"running":true,"elapsedMs":1000,"extra":1}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<SyncMessage>(
            r#"{"timer":{"running":true,"elapsedMs":-1}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<SyncMessage>(
            r#"{"timer":{"running":true,"elapsedMs":1000},"index":1}"#
        )
        .is_err());
    }

    #[test]
    fn parses_sync_get_query() {
        assert!(matches!(sync_get("/sync"), Some(SyncGet::Handshake)));
        assert!(matches!(sync_get("/sync?seq="), Some(SyncGet::Handshake)));
        assert!(sync_get("/sync?seq=now").is_none());
        assert!(matches!(sync_get("/sync?seq=42"), Some(SyncGet::Poll(42))));
        assert!(matches!(
            sync_get("/sync?other=x&seq=7"),
            Some(SyncGet::Poll(7))
        ));
        assert!(sync_get("/sync?seq=nope").is_none());
    }

    #[test]
    fn parses_pointer_get_query() {
        assert!(matches!(
            pointer_get("/pointer"),
            Some(PointerGet::Handshake)
        ));
        assert!(matches!(
            pointer_get("/pointer?seq="),
            Some(PointerGet::Handshake)
        ));
        assert!(pointer_get("/pointer?seq=now").is_none());
        assert!(matches!(
            pointer_get("/pointer?seq=42"),
            Some(PointerGet::Poll(42))
        ));
        assert!(matches!(
            pointer_get("/pointer?other=x&seq=7"),
            Some(PointerGet::Poll(7))
        ));
        assert!(pointer_get("/pointer?seq=nope").is_none());
    }

    #[test]
    fn formats_rehearsal_filename_from_local_time() {
        let local = chrono::NaiveDate::from_ymd_opt(2026, 7, 19)
            .unwrap()
            .and_hms_opt(9, 5, 7)
            .unwrap();

        assert_eq!(
            format_rehearsal_filename(local, 1),
            "rehearsal-20260719-090507.json"
        );
        assert_eq!(
            format_rehearsal_filename(local, 2),
            "rehearsal-20260719-090507-2.json"
        );
    }

    #[test]
    fn reserves_collision_suffixes_without_overwriting_existing_sessions() {
        let local = chrono::NaiveDate::from_ymd_opt(2026, 7, 19)
            .unwrap()
            .and_hms_opt(9, 5, 7)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("rehearsal-20260719-090507.json"),
            "existing",
        )
        .unwrap();
        let second = reserve_rehearsal_path(dir.path(), local).unwrap();
        drop(second.file);
        fs::write(&second.path, "second").unwrap();
        let third = reserve_rehearsal_path(dir.path(), local).unwrap();

        assert_eq!(
            second.path.file_name().and_then(|name| name.to_str()),
            Some("rehearsal-20260719-090507-2.json")
        );
        assert_eq!(
            third.path.file_name().and_then(|name| name.to_str()),
            Some("rehearsal-20260719-090507-3.json")
        );
        assert!(
            parse_rehearsal_filename("rehearsal-20260719-090507.json")
                < parse_rehearsal_filename("rehearsal-20260719-090507-2.json")
        );
        assert!(
            parse_rehearsal_filename("rehearsal-20260719-090507-2.json")
                < parse_rehearsal_filename("rehearsal-20260719-090507-3.json")
        );
    }

    #[test]
    fn parses_only_rehearsal_filename_scheme() {
        assert_eq!(
            parse_rehearsal_filename("rehearsal-20260719-090507.json"),
            Some(("20260719-090507".to_owned(), 1))
        );
        assert_eq!(
            parse_rehearsal_filename("rehearsal-20260719-090507-10.json"),
            Some(("20260719-090507".to_owned(), 10))
        );
        assert_eq!(parse_rehearsal_filename("zzz-notes.json"), None);
        assert_eq!(
            parse_rehearsal_filename("rehearsal-20260719-090507-0.json"),
            None
        );
        assert_eq!(
            parse_rehearsal_filename("rehearsal-20260719-090507-01.json"),
            None
        );
    }

    #[test]
    fn first_write_failure_removes_reserved_rehearsal_file() {
        let dir = tempfile::tempdir().unwrap();
        let local = chrono::NaiveDate::from_ymd_opt(2026, 7, 19)
            .unwrap()
            .and_hms_opt(9, 5, 7)
            .unwrap();
        let reserved = reserve_rehearsal_path(dir.path(), local).unwrap();
        let path = reserved.path.clone();

        let error = write_first_rehearsal_record(reserved, b"{}", |file, _bytes| {
            file.write_all(b"{")?;
            Err(std::io::Error::other("injected first-write failure"))
        })
        .unwrap_err();

        assert!(!path.exists());
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
        assert!(error.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn write_atomic_appends_tmp_to_the_full_file_name() {
        assert_eq!(
            atomic_tmp_path(Path::new("deck.md")),
            PathBuf::from("deck.md.tmp")
        );
        assert_eq!(
            atomic_tmp_path(Path::new("rehearsal-X.json")),
            PathBuf::from("rehearsal-X.json.tmp")
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deck.md");
        write_atomic(&path, b"new contents").unwrap();

        assert!(path.exists());
        assert_eq!(fs::read(&path).unwrap(), b"new contents");
        assert!(!atomic_tmp_path(&path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_follows_symlinks_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deck.md");
        let link = dir.path().join("linked-deck.md");
        fs::write(&target, b"old contents").unwrap();
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&target, permissions).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_atomic(&link, b"new contents").unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&target).unwrap(), b"new contents");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn write_atomic_removes_tmp_when_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deck.md");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"keep").unwrap();
        let canonical_target = fs::canonicalize(&target).unwrap();
        let tmp = atomic_tmp_path(&canonical_target);

        assert!(write_atomic(&target, b"new contents").is_err());

        assert!(!tmp.exists());
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"keep");
    }

    fn rehearsal_sink(dir: PathBuf) -> RehearsalSink {
        RehearsalSink::new(
            dir,
            vec![("Setup".to_owned(), 60_000)],
            vec![
                SlideKey::new("intro").unwrap(),
                SlideKey::new("details").unwrap(),
            ],
        )
    }

    #[test]
    fn non_rehearsal_server_discards_rehearsal_reports() {
        let response = rehearsal_post_outcome(
            None,
            r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
        );

        assert_eq!(response.status, 200);
        assert_eq!(response.body, r#"{"recorded":false}"#);
        assert!(response.json);
    }

    #[test]
    fn rehearsal_server_writes_and_rewrites_one_session_record() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = rehearsal_sink(rehearsals.clone());

        let first = rehearsal_post_outcome(
            Some(&sink),
            r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
        );
        assert_eq!(first.status, 200);
        assert_eq!(first.body, r#"{"recorded":true}"#);
        let first_path = single_rehearsal_file(&rehearsals);
        assert!(fs::metadata(&first_path).unwrap().len() > 0);
        assert!(fs::read_dir(&rehearsals)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .all(|path| path.extension().and_then(|ext| ext.to_str()) != Some("tmp")));
        let first_record: peitho_core::RehearsalRecord =
            serde_json::from_str(&fs::read_to_string(&first_path).unwrap()).unwrap();
        assert_eq!(first_record.elapsed_ms(), 1_000);
        assert_eq!(first_record.sections()[0].actual_ms(), 1_000);

        let second = rehearsal_post_outcome(
            Some(&sink),
            r#"{"version":2,"elapsedMs":2000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":2000}],"timeline":[{"key":"intro","index":0,"atMs":0},{"key":"details","index":1,"atMs":1000}]}"#,
        );
        assert_eq!(second.status, 200);
        assert_eq!(second.body, r#"{"recorded":true}"#);
        let second_path = single_rehearsal_file(&rehearsals);
        let second_record: peitho_core::RehearsalRecord =
            serde_json::from_str(&fs::read_to_string(&second_path).unwrap()).unwrap();

        assert_eq!(second_path, first_path);
        assert_eq!(second_record.elapsed_ms(), 2_000);
        assert_eq!(second_record.sections()[0].actual_ms(), 2_000);
        assert_eq!(
            second_record.recorded_at_ms(),
            first_record.recorded_at_ms()
        );
        let peitho_core::RehearsalRecord::V2(second_record) = second_record else {
            panic!("expected v2");
        };
        assert_eq!(second_record.timeline().len(), 2);

        let reset = rehearsal_post_outcome(
            Some(&sink),
            r#"{"version":2,"elapsedMs":0,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":0}],"timeline":[]}"#,
        );
        assert_eq!(reset.status, 200);
        let reset_record: peitho_core::RehearsalRecord =
            serde_json::from_str(&fs::read_to_string(&second_path).unwrap()).unwrap();
        assert_eq!(reset_record.recorded_at_ms(), first_record.recorded_at_ms());
        let peitho_core::RehearsalRecord::V2(reset_record) = reset_record else {
            panic!("expected v2");
        };
        assert!(reset_record.timeline().is_empty());
        assert!(fs::read_dir(&rehearsals)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .all(|path| path.extension().and_then(|ext| ext.to_str()) != Some("tmp")));
    }

    #[test]
    fn rehearsal_server_rejects_garbage_body() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = rehearsal_sink(rehearsals.clone());

        let response = rehearsal_post_outcome(Some(&sink), "not json");

        assert_eq!(response.status, 400);
        assert!(!response.json);
        assert!(fs::read_dir(rehearsals).unwrap().next().is_none());
    }

    #[test]
    fn rehearsal_server_rejects_v1_snapshot_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = rehearsal_sink(rehearsals);

        let response = rehearsal_post_outcome(
            Some(&sink),
            r#"{"version":1,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}]}"#,
        );

        assert_eq!(response.status, 400);
        assert!(response.body.contains("unsupported rehearsal version 1"));
    }

    #[test]
    fn rehearsal_server_warns_for_every_rejection_class_through_label_style() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = rehearsal_sink(rehearsals);
        let mut warnings = Vec::new();
        let cases = [
            ("not json", 400, "invalid rehearsal body"),
            (
                r#"{"version":1,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}]}"#,
                400,
                "unsupported rehearsal version 1",
            ),
            (
                r#"{"version":2,"elapsedMs":0,"sections":[],"timeline":[]}"#,
                400,
                "rehearsal sections must not be empty",
            ),
            (
                r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":61000,"actualMs":1000}],"timeline":[]}"#,
                422,
                "rehearsal sections do not match this deck",
            ),
            (
                r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":1001}]}"#,
                422,
                "rehearsal timeline position 1001 exceeds elapsed time 1000",
            ),
        ];

        for (body, status, reason) in cases {
            let warning_start = warnings.len();
            let response = rehearsal_post_outcome_with_warnings(
                Some(&sink),
                body,
                &mut warnings,
                crate::labels::LabelStyle::COLORED,
            );
            assert_eq!(response.status, status);
            let warning = String::from_utf8_lossy(&warnings[warning_start..]);
            assert!(warning.starts_with("\x1b[1;33mwarning:\x1b[0m rejected rehearsal snapshot:"));
            assert!(
                warning.contains(reason),
                "expected {reason:?} in {warning:?}"
            );
        }

        let blocker = dir.path().join("not-a-directory");
        fs::write(&blocker, b"file").unwrap();
        let missing_sink = rehearsal_sink(blocker.join("rehearsals"));
        let response = rehearsal_post_outcome_with_warnings(
            Some(&missing_sink),
            r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
            &mut warnings,
            crate::labels::LabelStyle::COLORED,
        );

        assert_eq!(response.status, 500);
        assert!(String::from_utf8_lossy(&warnings)
            .contains("\x1b[1;33mwarning:\x1b[0m failed to write rehearsal snapshot:"));
    }

    #[test]
    fn rehearsal_server_deduplicates_rejection_warnings_until_success() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = rehearsal_sink(rehearsals);
        let section_mismatch = r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":61000,"actualMs":1000}],"timeline":[]}"#;
        let invalid_timeline = r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":1001}]}"#;
        let valid = r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":0}]}"#;
        let mut warnings = Vec::new();

        for _ in 0..2 {
            rehearsal_post_outcome_with_warnings(
                Some(&sink),
                section_mismatch,
                &mut warnings,
                crate::labels::LabelStyle::PLAIN,
            );
        }
        for _ in 0..2 {
            rehearsal_post_outcome_with_warnings(
                Some(&sink),
                invalid_timeline,
                &mut warnings,
                crate::labels::LabelStyle::PLAIN,
            );
        }
        assert_eq!(
            String::from_utf8_lossy(&warnings),
            "warning: rejected rehearsal snapshot: rehearsal sections do not match this deck\n\
warning: rejected rehearsal snapshot: rehearsal timeline position 1001 exceeds elapsed time 1000\n"
        );

        assert_eq!(
            rehearsal_post_outcome_with_warnings(
                Some(&sink),
                valid,
                &mut warnings,
                crate::labels::LabelStyle::PLAIN,
            )
            .status,
            200
        );
        rehearsal_post_outcome_with_warnings(
            Some(&sink),
            invalid_timeline,
            &mut warnings,
            crate::labels::LabelStyle::PLAIN,
        );

        assert_eq!(
            String::from_utf8_lossy(&warnings)
                .matches("rehearsal timeline position 1001 exceeds elapsed time 1000")
                .count(),
            2
        );
    }

    #[test]
    fn non_rehearsal_server_does_not_warn_for_rejected_reports() {
        let mut warnings = Vec::new();

        let response = rehearsal_post_outcome_with_warnings(
            None,
            "not json",
            &mut warnings,
            crate::labels::LabelStyle::PLAIN,
        );

        assert_eq!(response.status, 400);
        assert!(warnings.is_empty());
    }

    #[test]
    fn rehearsal_server_rejects_section_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = rehearsal_sink(rehearsals.clone());

        let response = rehearsal_post_outcome(
            Some(&sink),
            r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":61000,"actualMs":1000}],"timeline":[]}"#,
        );

        assert_eq!(response.status, 422);
        assert!(!response.json);
        assert!(fs::read_dir(rehearsals).unwrap().next().is_none());
    }

    #[test]
    fn rehearsal_server_rejects_invalid_timelines_without_changing_record_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = rehearsal_sink(rehearsals.clone());
        let valid = r#"{"version":2,"elapsedMs":2000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":2000}],"timeline":[{"key":"intro","index":0,"atMs":0},{"key":"details","index":1,"atMs":1000}]}"#;
        assert_eq!(rehearsal_post_outcome(Some(&sink), valid).status, 200);
        let path = single_rehearsal_file(&rehearsals);
        let original = fs::read(&path).unwrap();

        let cases = [
            (
                r#"{"version":2,"elapsedMs":2000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":2000}],"timeline":[{"key":"intro","index":0,"atMs":0},{"key":"details","index":1,"atMs":1000},{"key":"intro","index":0,"atMs":500}]}"#,
                "rehearsal timeline positions must be non-decreasing\n",
            ),
            (
                r#"{"version":2,"elapsedMs":2000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":2000}],"timeline":[{"key":"intro","index":0,"atMs":0},{"key":"details","index":1,"atMs":2001}]}"#,
                "rehearsal timeline position 2001 exceeds elapsed time 2000\n",
            ),
            (
                r#"{"version":2,"elapsedMs":2000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":2000}],"timeline":[{"key":"intro","index":0,"atMs":0},{"key":"details","index":2,"atMs":1000}]}"#,
                "rehearsal timeline index 2 is outside this deck\n",
            ),
            (
                r#"{"version":2,"elapsedMs":2000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":2000}],"timeline":[{"key":"intro","index":0,"atMs":0},{"key":"intro","index":1,"atMs":1000}]}"#,
                "rehearsal timeline key intro does not match slide 1 key details\n",
            ),
        ];

        for (body, expected_message) in cases {
            let response = rehearsal_post_outcome(Some(&sink), body);
            assert_eq!(response.status, 422);
            assert_eq!(response.body, expected_message);
            assert!(!response.json);
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn rehearsal_route_without_sink_returns_recorded_false_over_http() {
        let dir = tempfile::tempdir().unwrap();
        let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

        let response = http_request(
            &server,
            "POST",
            "/rehearsal",
            r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}],"timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
        );

        assert_eq!(response.status, 200);
        assert_eq!(response.body, r#"{"recorded":false}"#);
    }

    #[test]
    fn notes_route_saves_with_writer() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_by_writer = Arc::clone(&captured);
        let server = notes_server(Box::new(move |key, text| {
            captured_by_writer
                .lock()
                .unwrap()
                .push((key.as_str().to_owned(), text.to_owned()));
            Ok(())
        }));

        let saved = json_http_request(
            &server,
            "POST",
            "/notes",
            r#"{"key":"intro","text":"new note"}"#,
        );

        assert_eq!(saved.status, 200);
        assert_eq!(saved.body, r#"{"saved":true}"#);
        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured
                .first()
                .map(|(key, text)| (key.as_str(), text.as_str())),
            Some(("intro", "new note")),
        );
    }

    #[test]
    fn notes_route_rejects_invalid_shapes() {
        let calls = Arc::new(Mutex::new(0));
        let calls_by_writer = Arc::clone(&calls);
        let server = notes_server(Box::new(move |_, _| {
            *calls_by_writer.lock().unwrap() += 1;
            Ok(())
        }));

        let malformed = json_http_request(&server, "POST", "/notes", "{");
        let missing_field = json_http_request(&server, "POST", "/notes", r#"{"key":"intro"}"#);
        let unknown_field = json_http_request(
            &server,
            "POST",
            "/notes",
            r#"{"key":"intro","text":"new note","extra":true}"#,
        );
        let malformed_key =
            json_http_request(&server, "POST", "/notes", r#"{"key":"Bad Key","text":"x"}"#);

        assert_eq!(malformed.status, 400);
        assert_eq!(malformed.body, "invalid notes body\n");
        assert_eq!(missing_field.status, 400);
        assert_eq!(missing_field.body, "invalid notes body\n");
        assert_eq!(unknown_field.status, 400);
        assert_eq!(unknown_field.body, "invalid notes body\n");
        assert_eq!(malformed_key.status, 400);
        assert_eq!(malformed_key.body, "invalid notes body\n");
        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[test]
    fn notes_route_without_writer_returns_404() {
        let dir = tempfile::tempdir().unwrap();
        let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

        let without_writer = http_request(&server, "POST", "/notes", "{");

        assert_eq!(without_writer.status, 404);
        assert_eq!(without_writer.body, "404\n");
    }

    #[test]
    fn notes_route_maps_conflict_to_409() {
        let server = notes_server(Box::new(|_, _| {
            Err(NotesWriteError::Conflict("note target changed".to_owned()))
        }));

        let conflict = json_http_request(
            &server,
            "POST",
            "/notes",
            r#"{"key":"intro","text":"new note"}"#,
        );

        assert_eq!(conflict.status, 409);
        assert_eq!(conflict.body, r#"{"error":"note target changed"}"#);
    }

    #[test]
    fn notes_route_maps_unprocessable_to_422() {
        const MESSAGE: &str = "line 3: speaker note cannot contain '-->'\n  = help: remove or rewrite '-->' because it closes the HTML comment";

        let server = notes_server(Box::new(|_, _| {
            Err(NotesWriteError::Unprocessable(MESSAGE.to_owned()))
        }));

        let unprocessable = json_http_request(
            &server,
            "POST",
            "/notes",
            r#"{"key":"intro","text":"new note"}"#,
        );

        assert_eq!(unprocessable.status, 422);
        assert_eq!(
            serde_json::from_str::<Value>(&unprocessable.body).unwrap()["error"],
            MESSAGE
        );
    }

    #[test]
    fn notes_route_maps_io_to_500() {
        let server = notes_server(Box::new(|_, _| {
            Err(NotesWriteError::Io("failed to write deck.md".to_owned()))
        }));

        let io_failure = json_http_request(
            &server,
            "POST",
            "/notes",
            r#"{"key":"intro","text":"new note"}"#,
        );

        assert_eq!(io_failure.status, 500);
        assert_eq!(io_failure.body, r#"{"error":"failed to write deck.md"}"#);
    }

    #[test]
    fn notes_route_requires_json_content_type() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_by_writer = Arc::clone(&calls);
        let server = notes_server(Box::new(move |_, _| {
            calls_by_writer.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let body = r#"{"key":"intro","text":"new note"}"#;

        let text_plain =
            http_request_with_content_type(&server, "POST", "/notes", body, Some("text/plain"));
        let missing = http_request_with_content_type(&server, "POST", "/notes", body, None);

        assert_eq!(text_plain.status, 400);
        assert_eq!(text_plain.body, "invalid notes content type\n");
        assert_eq!(missing.status, 400);
        assert_eq!(missing.body, "invalid notes content type\n");
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let with_parameters = http_request_with_content_type(
            &server,
            "POST",
            "/notes",
            body,
            Some("application/json; charset=utf-8"),
        );

        assert_eq!(with_parameters.status, 200);
        assert_eq!(with_parameters.body, r#"{"saved":true}"#);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn notes_route_serializes_concurrent_saves() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let in_flight_by_writer = Arc::clone(&in_flight);
        let max_in_flight_by_writer = Arc::clone(&max_in_flight);
        let server = notes_server(Box::new(move |_, _| {
            let current = in_flight_by_writer.fetch_add(1, Ordering::SeqCst) + 1;
            max_in_flight_by_writer.fetch_max(current, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(50));
            in_flight_by_writer.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }));

        let handles = (0..4)
            .map(|index| {
                let server = server.clone();
                thread::spawn(move || {
                    json_http_request(
                        &server,
                        "POST",
                        "/notes",
                        &format!(r#"{{"key":"slide-{index}","text":"new note"}}"#),
                    )
                })
            })
            .collect::<Vec<_>>();
        let responses = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        assert!(responses.iter().all(|response| response.status == 200));
        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn get_rehearsal_is_not_a_route() {
        let dir = tempfile::tempdir().unwrap();
        let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

        let response = http_request(&server, "GET", "/rehearsal", "");

        assert_eq!(response.status, 404);
    }

    #[test]
    fn post_to_static_paths_still_returns_405() {
        let dir = tempfile::tempdir().unwrap();
        let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

        let response = http_request(&server, "POST", "/not-rehearsal", "{}");

        assert_eq!(response.status, 405);
    }

    #[test]
    fn sync_endpoint_is_unchanged_by_rehearsal_route() {
        let dir = tempfile::tempdir().unwrap();
        let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

        let post = http_request(&server, "POST", "/sync", r#"{"index":2,"step":0}"#);
        let get = http_request(&server, "GET", "/sync", "");

        assert_eq!(post.status, 200);
        assert_eq!(serde_json::from_str::<Value>(&post.body).unwrap()["seq"], 1);
        assert_eq!(get.status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&get.body).unwrap()["index"],
            2
        );
        assert_eq!(serde_json::from_str::<Value>(&get.body).unwrap()["step"], 0);
    }

    #[test]
    fn sync_endpoint_accepts_index_step_body() {
        let dir = tempfile::tempdir().unwrap();
        let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

        let post = http_request(&server, "POST", "/sync", r#"{"index":2,"step":1}"#);
        let get = http_request(&server, "GET", "/sync", "");

        assert_eq!(post.status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&get.body).unwrap()["index"],
            2
        );
        assert_eq!(serde_json::from_str::<Value>(&get.body).unwrap()["step"], 1);
    }

    #[test]
    fn sync_endpoint_rejects_bare_index_body() {
        let dir = tempfile::tempdir().unwrap();
        let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

        let response = http_request(&server, "POST", "/sync", r#"{"index":2}"#);

        assert_eq!(response.status, 400);
        assert_eq!(response.body, "invalid sync body\n");
    }

    #[derive(Debug)]
    struct TestHttpResponse {
        status: u16,
        body: String,
    }

    fn notes_server(writer: NotesWriter) -> PresentServer {
        PresentServer::bind(PathBuf::new(), 0, "present.html")
            .unwrap()
            .with_notes_writer(writer)
    }

    fn http_request(
        server: &PresentServer,
        method: &str,
        path: &str,
        body: &str,
    ) -> TestHttpResponse {
        http_request_with_content_type(server, method, path, body, None)
    }

    fn json_http_request(
        server: &PresentServer,
        method: &str,
        path: &str,
        body: &str,
    ) -> TestHttpResponse {
        http_request_with_content_type(server, method, path, body, Some("application/json"))
    }

    fn http_request_with_content_type(
        server: &PresentServer,
        method: &str,
        path: &str,
        body: &str,
        content_type: Option<&str>,
    ) -> TestHttpResponse {
        let addr = server.addr();
        let server_for_request = server.clone();
        let handle = thread::spawn(move || server_for_request.handle_one());
        let mut stream = TcpStream::connect(addr).unwrap();
        let content_type_header = content_type
            .map(|content_type| format!("Content-Type: {content_type}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n{content_type_header}Connection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();

        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        handle.join().unwrap();

        parse_http_response(&raw)
    }

    fn parse_http_response(raw: &str) -> TestHttpResponse {
        let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap();
        TestHttpResponse {
            status,
            body: body.to_owned(),
        }
    }

    fn single_rehearsal_file(dir: &Path) -> PathBuf {
        let files = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        files[0].clone()
    }
}
