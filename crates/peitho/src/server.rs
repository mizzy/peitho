use std::{
    fs,
    io::{Read, Write},
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncPoll {
    snapshot: SyncSnapshot,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncSnapshot {
    seq: u64,
    index: Option<usize>,
    swapped: bool,
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
            },
            message: state.latest.clone().expect("latest sync message"),
        })
    }

    fn snapshot(&self) -> SyncSnapshot {
        let (lock, _) = &*self.state;
        let state = lock.lock().expect("sync hub mutex");
        SyncSnapshot {
            seq: state.seq,
            index: state.index,
            swapped: state.swapped,
        }
    }
}

pub(crate) fn resolve_request_path(root: &Path, url: &str) -> Option<PathBuf> {
    let path = url.split('?').next().unwrap_or(url);
    if path.contains("://") {
        return None;
    }
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(root.join("present.html"));
    }
    // Extensionless aliases keeping Chrome app names dot-free so app window
    // placement is saved and restored (see browser::presenter_url).
    match trimmed {
        "presenter" | "presenter-swapped" => return Some(root.join("presenter.html")),
        "present-swapped" => return Some(root.join("present.html")),
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
        _ => "application/octet-stream",
    }
}

pub struct PresentServer {
    root: PathBuf,
    server: Arc<Server>,
    sync: SyncHub,
}

impl PresentServer {
    pub fn bind(root: PathBuf, port: u16) -> miette::Result<Self> {
        let server = Server::http(("127.0.0.1", port))
            .map_err(|err| miette::miette!("failed to bind present server: {err}"))?;
        Ok(Self {
            root,
            server: Arc::new(server),
            sync: SyncHub::default(),
        })
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

    pub fn serve_forever(self) -> miette::Result<()> {
        for request in self.server.incoming_requests() {
            self.respond(request, Some(ShutdownHandle::new(self.server.clone())));
        }
        let _ = writeln!(std::io::stdout(), "presentation ended");
        Ok(())
    }

    pub fn handle_one(&self) {
        if let Some(request) = self.server.incoming_requests().next() {
            self.respond(request, None);
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
                        sync_response_body(event.snapshot, Some(&event.message)),
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
        let Some(path) = resolve_request_path(&self.root, request.url()) else {
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

struct ShutdownHandle {
    server: Arc<Server>,
}

impl ShutdownHandle {
    fn new(server: Arc<Server>) -> Self {
        Self { server }
    }

    fn start(self) {
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            self.server.unblock();
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
        !matches!(self, Self::Close(message) if !message.close)
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
            if value.is_empty() || value == "now" {
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
        r#"{{"seq":{},"message":{},"index":{},"swapped":{}}}"#,
        snapshot.seq,
        message.unwrap_or("null"),
        index,
        snapshot.swapped
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
    fn resolves_root_to_present_html() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/").unwrap(),
            Path::new("/cache").join("present.html")
        );
    }

    #[test]
    fn resolves_extensionless_presenter_route() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/presenter").unwrap(),
            Path::new("/cache").join("presenter.html")
        );
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/presenter?seq=1").unwrap(),
            Path::new("/cache").join("presenter.html")
        );
    }

    #[test]
    fn resolves_swapped_routes_to_role_pages() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/present-swapped").unwrap(),
            Path::new("/cache").join("present.html")
        );
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/present-swapped?seq=1").unwrap(),
            Path::new("/cache").join("present.html")
        );
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/presenter-swapped").unwrap(),
            Path::new("/cache").join("presenter.html")
        );
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/presenter-swapped?seq=1").unwrap(),
            Path::new("/cache").join("presenter.html")
        );
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(resolve_request_path(Path::new("/cache"), "/../manifest.json").is_none());
        assert!(resolve_request_path(Path::new("/cache"), "/slides/../../secret").is_none());
        assert!(resolve_request_path(Path::new("/cache"), "http://x/manifest.json").is_none());
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
                    swapped: false
                },
                message: r#"{"index":2}"#.to_owned()
            }
        );
        assert!(hub.wait_after(1, Duration::from_millis(1)).is_none());
    }

    #[test]
    fn parses_sync_get_query() {
        assert!(matches!(sync_get("/sync"), Some(SyncGet::Handshake)));
        assert!(matches!(sync_get("/sync?seq="), Some(SyncGet::Handshake)));
        assert!(matches!(
            sync_get("/sync?seq=now"),
            Some(SyncGet::Handshake)
        ));
        assert!(matches!(sync_get("/sync?seq=42"), Some(SyncGet::Poll(42))));
        assert!(matches!(
            sync_get("/sync?other=x&seq=7"),
            Some(SyncGet::Poll(7))
        ));
        assert!(sync_get("/sync?seq=nope").is_none());
    }
}
