use std::{
    fs,
    io::{Read, Write},
    net::{IpAddr, SocketAddr},
    path::{Component, Path, PathBuf},
    sync::{Arc, Condvar, Mutex, RwLock},
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Clone, Default)]
pub(crate) struct SyncHub {
    state: Arc<(Mutex<SyncState>, Condvar)>,
}

#[derive(Default)]
struct SyncState {
    seq: u64,
    latest: Option<String>,
    index: Option<usize>,
    swapped: bool,
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncPoll {
    snapshot: SyncSnapshot,
    message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncSnapshot {
    seq: u64,
    index: Option<usize>,
    swapped: bool,
    generation: u64,
}

impl SyncHub {
    fn broadcast_sync_message(&self, message: &SyncMessage) -> u64 {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().expect("sync hub mutex");
        match message {
            SyncMessage::Index(message) => state.index = Some(message.index),
            SyncMessage::Swap(message) => state.swapped = message.swapped,
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
                generation: state.generation,
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
            generation: state.generation,
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
    server: Arc<Server>,
    listeners: Arc<Mutex<Vec<Arc<Server>>>>,
    sync: SyncHub,
}

impl PresentServer {
    pub fn bind(root: PathBuf, port: u16, default_document: &'static str) -> miette::Result<Self> {
        Self::bind_with_host(root, port, default_document, None)
    }

    pub fn bind_with_host(
        root: PathBuf,
        port: u16,
        default_document: &'static str,
        host: Option<IpAddr>,
    ) -> miette::Result<Self> {
        match bind_plan(host) {
            BindPlan::LoopbackOnly => Self::bind_addr(
                root,
                SocketAddr::from(([127, 0, 0, 1], port)),
                default_document,
            ),
            BindPlan::WildcardOnly(host) => {
                Self::bind_addr(root, SocketAddr::new(host, port), default_document)
            }
            BindPlan::LoopbackPlusExtra(host) => {
                let server = Self::bind_addr(
                    root,
                    SocketAddr::from(([127, 0, 0, 1], port)),
                    default_document,
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
    ) -> miette::Result<Self> {
        let server = Server::http(addr)
            .map_err(|err| miette::miette!("failed to bind present server at {addr}: {err}"))?;
        let server = Arc::new(server);
        Ok(Self {
            root: Arc::new(RwLock::new(root)),
            default_document: default_document.to_owned(),
            server: server.clone(),
            listeners: Arc::new(Mutex::new(vec![server])),
            sync: SyncHub::default(),
        })
    }

    fn add_listener(&self, host: IpAddr) -> miette::Result<SocketAddr> {
        validate_extra_listener_host(host)?;
        let addr = SocketAddr::new(host, self.addr().port());
        let server = Server::http(addr)
            .map_err(|err| miette::miette!("failed to bind present server at {addr}: {err}"))?;
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
            _ => {}
        }

        if request.method() != &Method::Get {
            send_response(request, Response::empty(StatusCode(405)));
            return;
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
        self.sync.broadcast_sync_message(&message);
        send_response(request, Response::empty(StatusCode(204)));
        if matches!(message, SyncMessage::Close(_)) {
            if let Some(shutdown) = shutdown {
                shutdown.start();
            }
        }
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
struct SyncCloseMessage {
    close: bool,
}

impl SyncMessage {
    fn is_valid(&self) -> bool {
        match self {
            Self::Close(message) => message.close,
            Self::Index(_) | Self::Swap(_) => true,
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
    let index = snapshot
        .index
        .map_or_else(|| "null".to_owned(), |index| index.to_string());
    format!(
        r#"{{"seq":{},"message":{},"index":{},"swapped":{},"generation":{}}}"#,
        snapshot.seq,
        message.unwrap_or("null"),
        index,
        snapshot.swapped,
        snapshot.generation
    )
}

fn send_json_response(request: tiny_http::Request, body: String) {
    let Ok(header) = Header::from_bytes("Content-Type", "application/json; charset=utf-8") else {
        eprintln!("warning: failed to build Content-Type header");
        return;
    };
    send_response(request, Response::from_string(body).with_header(header));
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
    use std::path::Path;
    use std::time::Duration;

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
                    generation: 0
                },
                message: Some(r#"{"index":2}"#.to_owned())
            }
        );
        assert!(hub.wait_after(1, Duration::from_millis(1)).is_none());
    }

    #[test]
    fn sync_hub_broadcast_reload_advances_generation_without_transient_message() {
        let hub = SyncHub::default();

        let seq = hub.broadcast_reload();

        assert_eq!(seq, 1);
        assert_eq!(
            hub.wait_after(0, Duration::from_secs(1)).unwrap(),
            SyncPoll {
                snapshot: SyncSnapshot {
                    seq: 1,
                    index: None,
                    swapped: false,
                    generation: 1
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
                generation: 1
            }
        );
    }

    #[test]
    fn sync_response_body_always_includes_generation() {
        let body = sync_response_body(
            SyncSnapshot {
                seq: 4,
                index: Some(2),
                swapped: true,
                generation: 9,
            },
            None,
        );

        assert!(body.contains(r#""generation":9"#));
        assert!(body.contains(r#""message":null"#));
        assert!(body.contains(r#""index":2"#));
        assert!(body.contains(r#""swapped":true"#));
    }

    #[test]
    fn reload_is_not_accepted_as_a_posted_sync_message() {
        assert!(serde_json::from_str::<SyncMessage>(r#"{"reload":true}"#).is_err());
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
}
