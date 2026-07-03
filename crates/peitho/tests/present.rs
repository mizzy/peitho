use std::{
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn present_help_lists_no_presenter_flag() {
    Command::cargo_bin("peitho")
        .unwrap()
        .args(["present", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--no-presenter"));
}

#[test]
fn present_no_serve_writes_clean_present_cache() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\nexport function serverSyncChannelFactory() {}\n",
    )
    .unwrap();
    let stale = dir.path().join(".peitho/present-cache/stale.txt");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    fs::write(&stale, "old").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(fixture.present_args(&shell))
        .args(["--no-serve", "--no-open"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".peitho/present-cache"));

    let cache = dir.path().join(".peitho/present-cache");
    assert!(!stale.exists());
    assert!(cache.join("present.html").exists());
    assert!(cache.join("shell.js").exists());
    assert!(cache.join("peitho.css").exists());
    assert!(cache.join("manifest.json").exists());
    assert!(cache.join("notes.json").exists());
    assert!(cache.join("present.json").exists());
    assert!(cache.join("slides/000-arch-1.html").exists());
    assert!(fs::read_to_string(cache.join("notes.json"))
        .unwrap()
        .contains(r#""notes": {}"#));
    assert!(fs::read_to_string(cache.join("present.json"))
        .unwrap()
        .contains(r#""presenterOpen": false"#));
    assert!(fs::read_to_string(cache.join("present.html"))
        .unwrap()
        .contains("installSyncBridge(window, peitho.serverSyncChannelFactory())"));
    assert!(fs::read_to_string(cache.join("present.html"))
        .unwrap()
        .contains("installCloseOnEscape(window)"));
}

#[test]
fn present_no_serve_writes_presenter_html() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\nexport function serverSyncChannelFactory() {}\nexport function mountPresenterView() {}\n",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(fixture.present_args(&shell))
        .args(["--no-serve", "--no-open"])
        .assert()
        .success();

    let cache = dir.path().join(".peitho/present-cache");
    let presenter = fs::read_to_string(cache.join("presenter.html")).unwrap();
    assert!(presenter.contains("mountPresenterView"));
    assert!(presenter.contains(".peitho-presenter-pane"));
    assert!(fs::read_to_string(cache.join("present.html"))
        .unwrap()
        .contains("installPresentationControls"));
    assert!(!fs::read_to_string(cache.join("present.html"))
        .unwrap()
        .contains("peitho-presenter-link"));
}

#[test]
fn present_without_shell_flag_writes_builtin_shell() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(fixture.present_args_builtin_shell())
        .args(["--no-serve", "--no-open"])
        .assert()
        .success();

    let written = fs::read_to_string(dir.path().join(".peitho/present-cache/shell.js")).unwrap();
    let committed =
        fs::read_to_string(workspace_root().join("packages/peitho-present/dist/shell.js")).unwrap();
    assert_eq!(written, committed);
    assert!(written.contains("mountPresentShell"));
}

#[test]
fn present_fails_with_help_when_shell_bundle_is_missing() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let missing_shell = dir.path().join("missing-shell.js");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(fixture.present_args(&missing_shell))
        .args(["--no-serve", "--no-open"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("shell bundle not found"))
        .stderr(predicate::str::contains(
            "cd packages/peitho-present && npm run build",
        ))
        .stderr(predicate::str::contains("--shell"))
        .stderr(predicate::str::contains("<path>"));
}

#[test]
fn present_server_serves_manifest_over_http() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();
    fs::write(cache.join("manifest.json"), r#"{"version":1}"#).unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .write_all(b"GET /manifest.json HTTP/1.0\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    handle.join().unwrap();

    assert!(response.contains("200 OK"));
    assert!(response.contains("application/json"));
    assert!(response.contains(r#"{"version":1}"#));
}

#[test]
fn present_server_relays_sync_post_to_long_poll_subscriber() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.serve_forever());

    let mut poll = TcpStream::connect(addr).unwrap();
    poll.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    poll.write_all(b"GET /sync?seq=0 HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();

    let post_response = post_sync(addr, r#"{"index":1}"#);
    assert!(post_response.contains("204 No Content"));

    let event = read_until_contains(&mut poll, r#""message":{"index":1}"#);
    assert!(event.contains("200 OK"));
    assert!(event.contains("application/json"));
    assert!(event.contains(r#""seq":1"#));
    assert!(event.contains(r#""message":{"index":1}"#));

    drop(poll);
    drop(handle);
}

#[test]
fn present_server_relays_close_sync_post_to_long_poll_subscriber() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.serve_forever());

    let mut poll = TcpStream::connect(addr).unwrap();
    poll.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    poll.write_all(b"GET /sync?seq=0 HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();

    let post_response = post_sync(addr, r#"{"close":true}"#);
    assert!(post_response.contains("204 No Content"));

    let event = read_until_contains(&mut poll, r#""message":{"close":true}"#);
    assert!(event.contains("200 OK"));
    assert!(event.contains(r#""seq":1"#));
    assert!(event.contains(r#""message":{"close":true}"#));

    drop(poll);
    drop(handle);
}

#[test]
fn present_server_shuts_down_after_close_sync_post() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.serve_forever());

    let post_response = post_sync(addr, r#"{"close":true}"#);
    assert!(post_response.contains("204 No Content"));

    join_present_server(handle, Duration::from_secs(3));
}

#[test]
fn present_server_sync_handshake_returns_current_seq_without_replaying_latest_message() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || {
        server.handle_one();
        server.handle_one();
    });

    let post_response = post_sync(addr, r#"{"close":true}"#);
    assert!(post_response.contains("204 No Content"));

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .write_all(b"GET /sync HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    assert!(response.contains("200 OK"));
    assert!(response.contains(r#""seq":1"#));
    assert!(response.contains(r#""message":null"#));
    assert!(!response.contains(r#""close":true"#));
    handle.join().unwrap();
}

#[test]
fn present_server_rejects_invalid_sync_post_body() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = post_sync(addr, r#"{"key":"x"}"#);

    assert!(response.contains("400 Bad Request"));
    handle.join().unwrap();
}

#[test]
fn present_server_rejects_mixed_sync_post_body() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = post_sync(addr, r#"{"index":1,"close":true}"#);

    assert!(response.contains("400 Bad Request"));
    handle.join().unwrap();
}

#[test]
fn present_no_open_server_prints_assigned_url() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\nexport function serverSyncChannelFactory() {}\n",
    )
    .unwrap();

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("peitho"))
        .current_dir(dir.path())
        .args(fixture.present_args(&shell))
        .args(["--no-open", "--port", "0"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut tx = Some(tx);
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.contains("serving presentation at") {
                if let Some(tx) = tx.take() {
                    tx.send(line).unwrap();
                }
            }
        }
    });
    let line = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("present server did not print serving URL within 5 seconds");
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();

    assert!(line.contains("http://127.0.0.1:"));
    assert!(line.contains("/present.html"));
}

#[test]
fn present_process_exits_after_close_sync_post() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\nexport function serverSyncChannelFactory() {}\n",
    )
    .unwrap();

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("peitho"))
        .current_dir(dir.path())
        .args(fixture.present_args(&shell))
        .args(["--no-open", "--port", "0"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut tx = Some(tx);
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.contains("serving presentation at") {
                if let Some(tx) = tx.take() {
                    tx.send(line).unwrap();
                }
            }
        }
    });
    let line = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("present server did not print serving URL within 5 seconds");
    let addr = serving_addr(&line);

    let post_response = post_sync(addr, r#"{"close":true}"#);
    assert!(post_response.contains("204 No Content"));

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "present process did not exit after close sync post"
        );
        thread::sleep(Duration::from_millis(25));
    }
    reader.join().unwrap();
}

#[test]
fn repository_example_present_no_serve_smoke() {
    let shell = workspace_root().join("packages/peitho-present/dist/shell.js");
    assert!(shell.exists(), "shell bundle not built; run npm run build");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "present",
            "examples/deck.md",
            "--layouts",
            "layouts/title-body-code.html",
            "--css",
            "themes/base.css",
            "--no-serve",
            "--no-open",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated present cache"));

    let cache = workspace_root().join(".peitho/present-cache");
    assert!(cache.join("present.html").exists());
    assert!(cache.join("presenter.html").exists());
    assert!(cache.join("shell.js").exists());
    assert!(fs::read_to_string(cache.join("manifest.json"))
        .unwrap()
        .contains(r#""slideCount": 3"#));
    assert!(fs::read_to_string(cache.join("presenter.html"))
        .unwrap()
        .contains("mountPresenterView"));
    let present_html = fs::read_to_string(cache.join("present.html")).unwrap();
    let presenter_html = fs::read_to_string(cache.join("presenter.html")).unwrap();
    let shell_js = fs::read_to_string(cache.join("shell.js")).unwrap();
    assert!(present_html.contains("installPresentationControls"));
    assert!(present_html.contains("installCanvasClickNavigation"));
    assert!(present_html.contains("installFullscreenShortcut"));
    assert!(present_html.contains("installCloseOnEscape(window)"));
    assert!(present_html.contains("serverSyncChannelFactory"));
    assert!(present_html.contains("data-peitho-action=\"close\""));
    assert!(!present_html.contains("peitho-presenter-link"));
    assert!(presenter_html.contains(".peitho-presenter-pane"));
    assert!(presenter_html.contains("installCloseOnEscape(window)"));
    assert!(presenter_html.contains("serverSyncChannelFactory"));
    assert!(shell_js.contains("CANVAS_WIDTH"));
    assert!(shell_js.contains("installPresentationControls"));
    assert!(shell_js.contains("installCloseOnEscape"));
    assert!(shell_js.contains("openPresenterPopup"));
    assert!(shell_js.contains("serverSyncChannelFactory"));
    assert!(shell_js.contains(r#"data-peitho-action="close""#));
    assert!(!shell_js.contains("getScreenDetails"));
    assert!(!shell_js.contains("requestFullscreen({screen"));
    assert!(!shell_js.contains("data-peitho-place-overlay"));
    assert!(shell_js.contains("mountPresenterView"));
}

struct Fixture {
    deck: std::path::PathBuf,
    layout: std::path::PathBuf,
    css_dir: std::path::PathBuf,
}

impl Fixture {
    fn write(root: &std::path::Path) -> Self {
        let deck = root.join("deck.md");
        let layout = root.join("layout.html");
        let css_dir = root.join("css");
        fs::create_dir_all(&css_dir).unwrap();
        fs::write(
            &deck,
            "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody",
        )
        .unwrap();
        fs::write(
            &layout,
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot></section>"#,
        )
        .unwrap();
        fs::write(css_dir.join("base.css"), ".slot-title { color: red; }").unwrap();
        fs::write(
            css_dir.join("overrides.css"),
            r#"[data-slide-key="arch-1"] .slot-title { color: blue; }"#,
        )
        .unwrap();
        Self {
            deck,
            layout,
            css_dir,
        }
    }

    fn present_args(&self, shell: &std::path::Path) -> Vec<OsString> {
        let mut args = self.present_args_builtin_shell();
        args.push(OsString::from("--shell"));
        args.push(shell.as_os_str().to_owned());
        args
    }

    fn present_args_builtin_shell(&self) -> Vec<OsString> {
        vec![
            OsString::from("present"),
            self.deck.as_os_str().to_owned(),
            OsString::from("--layouts"),
            self.layout.as_os_str().to_owned(),
            OsString::from("--css"),
            self.css_dir.as_os_str().to_owned(),
        ]
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn read_until_contains(stream: &mut TcpStream, needle: &str) -> String {
    let mut out = String::new();
    let mut buf = [0_u8; 128];
    while !out.contains(needle) {
        let len = stream.read(&mut buf).unwrap();
        assert!(len > 0, "stream closed before reading {needle:?}: {out:?}");
        out.push_str(&String::from_utf8_lossy(&buf[..len]));
    }
    out
}

fn post_sync(addr: std::net::SocketAddr, body: &str) -> String {
    let mut post = TcpStream::connect(addr).unwrap();
    write!(
        post,
        "POST /sync HTTP/1.0\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
    let mut response = String::new();
    post.read_to_string(&mut response).unwrap();
    response
}

fn join_present_server(handle: thread::JoinHandle<miette::Result<()>>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !handle.is_finished() {
        assert!(
            Instant::now() < deadline,
            "present server did not stop within {timeout:?}"
        );
        thread::sleep(Duration::from_millis(25));
    }
    handle.join().unwrap().unwrap();
}

fn serving_addr(line: &str) -> std::net::SocketAddr {
    let prefix = "serving presentation at http://";
    let rest = line
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("unexpected serving URL line: {line}"));
    let host_port = rest
        .strip_suffix("/present.html")
        .unwrap_or_else(|| panic!("unexpected serving URL line: {line}"));
    host_port.parse().unwrap()
}
