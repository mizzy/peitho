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
use peitho_core::{rehearsal_record_json, RehearsalRecord, RehearsalSection, RehearsalSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tiny_http::{Header, Method, Response, Server, StatusCode};

static SERVER_CLOCK_START: OnceLock<Instant> = OnceLock::new();
static SYNC_SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);
const REMOTE_WEBMANIFEST: &str = r##"{"name":"Peitho Remote","short_name":"Remote","start_url":"/remote","display":"standalone","background_color":"#101216","theme_color":"#101216","icons":[{"src":"remote-icon.png","sizes":"180x180","type":"image/png"}]}"##;
const REMOTE_ICON_PNG: &[u8] = include_bytes!("../assets/remote-icon.png");

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
            SyncMessage::Index(message) => state.index = Some(message.index),
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
            swapped: state.swapped,
            timer: state.timer,
            generation: state.generation,
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
        _ => "application/octet-stream",
    }
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
    server: Arc<Server>,
    listeners: Arc<Mutex<Vec<Arc<Server>>>>,
    sync: SyncHub,
}

#[derive(Debug)]
pub struct RehearsalSink {
    dir: PathBuf,
    expected: Vec<(String, u64)>,
    session: Mutex<Option<RehearsalSession>>,
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
    Io(io::Error),
    Serialize(String),
}

impl fmt::Display for RehearsalWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SectionMismatch => write!(f, "rehearsal sections do not match this deck"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Serialize(err) => write!(f, "{err}"),
        }
    }
}

impl RehearsalSink {
    pub fn new(dir: PathBuf, expected: Vec<(String, u64)>) -> Self {
        Self {
            dir,
            expected,
            session: Mutex::new(None),
        }
    }

    fn write_snapshot(&self, snapshot: &RehearsalSnapshot) -> Result<(), RehearsalWriteError> {
        if !snapshot_matches_expected(snapshot.sections(), &self.expected) {
            return Err(RehearsalWriteError::SectionMismatch);
        }

        // Hold the session mutex through serialization and disk writes; it
        // serializes concurrent POSTs that share this session path.
        let mut session = self.session.lock().expect("rehearsal sink mutex");
        if let Some(session) = session.as_ref() {
            let record = RehearsalRecord::from_snapshot(session.recorded_at_ms, snapshot);
            let json = rehearsal_record_json(&record)
                .map_err(|err| RehearsalWriteError::Serialize(err.to_string()))?;

            // Keep the session mutex held through the atomic rewrite; it also
            // serializes concurrent POSTs that share this session path and temp file.
            return write_atomic(&session.path, json.as_bytes()).map_err(RehearsalWriteError::Io);
        }

        let recorded_at_ms = epoch_ms_now();
        let record = RehearsalRecord::from_snapshot(recorded_at_ms, snapshot);
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
        *session = Some(RehearsalSession {
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
        Ok(Self {
            root: Arc::new(RwLock::new(root)),
            default_document: default_document.to_owned(),
            serve_remote_assets,
            rehearsal_sink: None,
            server: server.clone(),
            listeners: Arc::new(Mutex::new(vec![server])),
            sync: SyncHub::default(),
        })
    }

    pub fn with_rehearsal_sink(mut self, sink: RehearsalSink) -> Self {
        self.rehearsal_sink = Some(Arc::new(sink));
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
            (&Method::Post, "/rehearsal") => {
                self.respond_rehearsal_post(request);
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
                send_response(request, Response::from_data(bytes).with_header(header));
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

fn validate_extra_listener_host(host: IpAddr) -> miette::Result<()> {
    if host.is_unspecified() {
        return Err(miette::miette!(
            "extra listener must be specific\nhelp: bind the wildcard as the primary listener"
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncIndexMessage {
    index: usize,
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

impl SyncMessage {
    fn is_valid(&self) -> bool {
        match self {
            Self::Close(message) => message.close,
            Self::Index(_) | Self::Swap(_) | Self::Timer(_) => true,
        }
    }
}

enum SyncGet {
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

fn sync_response_body(snapshot: SyncSnapshot, message: Option<&str>) -> String {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SyncResponseBody {
        seq: u64,
        message: Option<Value>,
        index: Option<usize>,
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
    let Ok(snapshot) = serde_json::from_str::<RehearsalSnapshot>(body) else {
        return text_rehearsal_outcome(400, "invalid rehearsal body\n");
    };
    if let Err(message) = snapshot.validate() {
        return text_rehearsal_outcome(400, format!("invalid rehearsal snapshot: {message}\n"));
    }
    let Some(sink) = sink else {
        return json_rehearsal_outcome(rehearsal_post_response_body(false));
    };
    match sink.write_snapshot(&snapshot) {
        Ok(()) => {}
        Err(RehearsalWriteError::SectionMismatch) => {
            return text_rehearsal_outcome(422, "rehearsal sections do not match this deck\n");
        }
        Err(err) => {
            eprintln!("warning: failed to write rehearsal snapshot: {err}");
            return text_rehearsal_outcome(500, "failed to write rehearsal snapshot\n");
        }
    }
    json_rehearsal_outcome(rehearsal_post_response_body(true))
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

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // Crash-orphaned *.json.tmp files do not match the record scheme and are not swept because sweeping could race an in-flight rename.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
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
    send_bytes_response(request, "application/json; charset=utf-8", body.as_bytes());
}

fn send_remote_webmanifest_response(request: tiny_http::Request) {
    send_bytes_response(
        request,
        "application/manifest+json",
        REMOTE_WEBMANIFEST.as_bytes(),
    );
}

fn send_remote_icon_response(request: tiny_http::Request) {
    send_bytes_response(request, "image/png", REMOTE_ICON_PNG);
}

fn send_bytes_response(request: tiny_http::Request, content_type: &str, body: &[u8]) {
    let Ok(header) = Header::from_bytes("Content-Type", content_type) else {
        eprintln!("warning: failed to build Content-Type header");
        return;
    };
    send_response(
        request,
        Response::from_data(body.to_vec()).with_header(header),
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

        let message = SyncMessage::Index(SyncIndexMessage { index: 2 });
        let seq = hub.broadcast_sync_message(&message);

        assert_eq!(seq, 1);
        assert_eq!(
            hub.wait_after(0, Duration::from_secs(1)).unwrap(),
            SyncPoll {
                snapshot: SyncSnapshot {
                    seq: 1,
                    index: Some(2),
                    swapped: false,
                    timer: None,
                    generation: 0,
                    session
                },
                message: Some(r#"{"index":2}"#.to_owned())
            }
        );
        assert!(hub.wait_after(1, Duration::from_millis(1)).is_none());
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

        hub.broadcast_sync_message(&SyncMessage::Index(SyncIndexMessage { index: 1 }));
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
    fn sync_response_body_always_includes_generation() {
        let body = sync_response_body(
            SyncSnapshot {
                seq: 4,
                index: Some(2),
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
        assert!(body.contains(r#""swapped":true"#));
        assert_eq!(json["timer"]["running"], true);
        assert_eq!(json["timer"]["elapsedMs"], 12_000);
        assert_eq!(json["timer"]["atMs"], 98_000);
        assert!(json["nowMs"].as_u64().unwrap() < 24 * 60 * 60 * 1000);
    }

    #[test]
    fn sync_response_body_includes_session() {
        let body = sync_response_body(
            SyncSnapshot {
                seq: 4,
                index: None,
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
    fn reload_is_not_accepted_as_a_posted_sync_message() {
        assert!(serde_json::from_str::<SyncMessage>(r#"{"reload":true}"#).is_err());
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
    fn non_rehearsal_server_discards_rehearsal_reports() {
        let response = rehearsal_post_outcome(
            None,
            r#"{"version":1,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}]}"#,
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
        let sink = RehearsalSink::new(rehearsals.clone(), vec![("Setup".to_owned(), 60_000)]);

        let first = rehearsal_post_outcome(
            Some(&sink),
            r#"{"version":1,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}]}"#,
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
            r#"{"version":1,"elapsedMs":2000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":2000}]}"#,
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
        let sink = RehearsalSink::new(rehearsals.clone(), vec![("Setup".to_owned(), 60_000)]);

        let response = rehearsal_post_outcome(Some(&sink), "not json");

        assert_eq!(response.status, 400);
        assert!(!response.json);
        assert!(fs::read_dir(rehearsals).unwrap().next().is_none());
    }

    #[test]
    fn rehearsal_server_rejects_future_version_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = RehearsalSink::new(rehearsals, vec![("Setup".to_owned(), 60_000)]);

        let response = rehearsal_post_outcome(
            Some(&sink),
            r#"{"version":2,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}]}"#,
        );

        assert_eq!(response.status, 400);
        assert!(response
            .body
            .contains("invalid rehearsal snapshot: unsupported rehearsal version 2"));
    }

    #[test]
    fn rehearsal_server_rejects_section_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let rehearsals = dir.path().join("rehearsals");
        fs::create_dir_all(&rehearsals).unwrap();
        let sink = RehearsalSink::new(rehearsals.clone(), vec![("Setup".to_owned(), 60_000)]);

        let response = rehearsal_post_outcome(
            Some(&sink),
            r#"{"version":1,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":61000,"actualMs":1000}]}"#,
        );

        assert_eq!(response.status, 422);
        assert!(!response.json);
        assert!(fs::read_dir(rehearsals).unwrap().next().is_none());
    }

    #[test]
    fn rehearsal_route_without_sink_returns_recorded_false_over_http() {
        let dir = tempfile::tempdir().unwrap();
        let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

        let response = http_request(
            &server,
            "POST",
            "/rehearsal",
            r#"{"version":1,"elapsedMs":1000,"sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":1000}]}"#,
        );

        assert_eq!(response.status, 200);
        assert_eq!(response.body, r#"{"recorded":false}"#);
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

        let post = http_request(&server, "POST", "/sync", r#"{"index":2}"#);
        let get = http_request(&server, "GET", "/sync", "");

        assert_eq!(post.status, 200);
        assert_eq!(serde_json::from_str::<Value>(&post.body).unwrap()["seq"], 1);
        assert_eq!(get.status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&get.body).unwrap()["index"],
            2
        );
    }

    #[derive(Debug)]
    struct TestHttpResponse {
        status: u16,
        body: String,
    }

    fn http_request(
        server: &PresentServer,
        method: &str,
        path: &str,
        body: &str,
    ) -> TestHttpResponse {
        let addr = server.addr();
        let server_for_request = server.clone();
        let handle = thread::spawn(move || server_for_request.handle_one());
        let mut stream = TcpStream::connect(addr).unwrap();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
