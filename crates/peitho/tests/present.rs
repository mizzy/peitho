use std::{
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, Ipv6Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
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
        .stdout(predicate::str::contains("--no-presenter"))
        .stdout(predicate::str::contains("--host [<IP>]"))
        .stdout(predicate::str::contains(
            "bare --host picks the best address automatically",
        ))
        .stdout(predicate::str::contains("VPN, e.g. Tailscale, preferred"));
}

#[test]
fn present_bare_host_requires_server_in_cli_path() {
    let bare = Command::cargo_bin("peitho")
        .unwrap()
        .args(["present", "--no-serve", "--host"])
        .output()
        .unwrap();

    assert!(!bare.status.success());
    assert!(String::from_utf8_lossy(&bare.stderr).contains("--host requires the present server"));
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
    assert!(cache.join("remote.html").exists());
    assert!(cache.join("shell.js").exists());
    assert!(cache.join("remote.js").exists());
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
    let present_html = fs::read_to_string(cache.join("present.html")).unwrap();
    assert!(present_html.contains("peitho.installSyncBridge("));
    assert!(present_html.contains("adoptTimerState: (state) => shell.adoptTimerState(state)"));
    assert!(fs::read_to_string(cache.join("present.html"))
        .unwrap()
        .contains("installCloseOnEscape(window)"));
}

#[test]
fn present_no_serve_expands_two_file_deck_into_manifest_and_notes() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layout = dir.path().join("layout.html");
    let css_dir = dir.path().join("css");
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(
        &deck,
        deck_with_assets(
            "./layout.html",
            "# Cover\n\n---\n<!-- {\"include\":\"shared.md\"} -->\n---\n# End\n",
        ),
    )
    .unwrap();
    fs::write(
        dir.path().join("shared.md"),
        "<!-- {\"key\":\"shared\"} -->\n# Shared\n\n<!-- Speaker note from include. -->\n",
    )
    .unwrap();
    fs::write(
        &layout,
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot></section>"#,
    )
    .unwrap();
    fs::write(css_dir.join("base.css"), "").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(["present", deck.to_str().unwrap(), "--no-serve", "--no-open"])
        .assert()
        .success();

    let cache = dir.path().join(".peitho/present-cache");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(cache.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["slideCount"].as_u64(), Some(3));
    let slides = manifest["slides"].as_array().unwrap();
    assert_eq!(slides[0]["key"].as_str(), Some("cover"));
    assert_eq!(slides[1]["key"].as_str(), Some("shared"));
    assert_eq!(slides[2]["key"].as_str(), Some("end"));
    assert_eq!(slides[1]["hasNotes"].as_bool(), Some(true));

    let notes = fs::read_to_string(cache.join("notes.json")).unwrap();
    assert!(notes.contains("Speaker note from include."));
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
    assert!(presenter.contains(r#"[data-peitho-urgency="urgent"]"#));
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
fn present_reads_layouts_from_frontmatter() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(
        &deck,
        "---\nlayouts: ./custom-layouts\n---\n# Frontmatter Layout\n\nBody",
    )
    .unwrap();
    write_layout_dir(dir.path(), "custom-layouts", "frontmatter-layout");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(["present", deck.to_str().unwrap(), "--no-open", "--no-serve"])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated present cache"));

    let html = fs::read_to_string(
        dir.path()
            .join(".peitho/present-cache/slides/000-frontmatter-layout.html"),
    )
    .unwrap();
    assert!(html.contains(r#"class="frontmatter-layout peitho-slide""#));
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

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
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
fn present_server_serves_remote_webmanifest_over_http() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind_with_remote_assets(
        cache,
        0,
        "present.html",
        None,
        true,
    )
    .unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = get_http(addr, "/remote.webmanifest");
    let body: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();

    assert!(response.contains("200 OK"));
    assert!(response.contains("application/manifest+json"));
    assert_eq!(body["display"], "standalone");
    assert_eq!(body["start_url"], "/remote");
    assert_eq!(body["icons"][0]["src"], "remote-icon.png");
    handle.join().unwrap();
}

#[test]
fn present_server_serves_remote_icon_over_http() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind_with_remote_assets(
        cache,
        0,
        "present.html",
        None,
        true,
    )
    .unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = get_http_bytes(addr, "/remote-icon.png");
    let head = response_head(&response);
    let body = response_body_bytes(&response);

    assert!(head.contains("200 OK"));
    assert!(head.contains("Content-Type: image/png"));
    assert!(body.starts_with(b"\x89PNG\r\n\x1a\n"));
    handle.join().unwrap();
}

#[test]
fn present_server_without_remote_assets_404s_remote_routes() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || {
        server.handle_one();
        server.handle_one();
    });

    let manifest = get_http(addr, "/remote.webmanifest");
    let icon = get_http(addr, "/remote-icon.png");

    assert!(manifest.contains("404 Not Found"));
    assert!(icon.contains("404 Not Found"));
    handle.join().unwrap();
}

#[test]
fn present_server_extra_listener_serves_manifest_over_http() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();
    fs::write(cache.join("manifest.json"), r#"{"version":1}"#).unwrap();

    let host = IpAddr::V6(Ipv6Addr::LOCALHOST);
    let server = peitho::server::PresentServer::bind_with_remote_assets(
        cache,
        0,
        "present.html",
        Some(host),
        false,
    )
    .unwrap();
    let primary_addr = server.addr();
    let extra_addr = SocketAddr::new(host, primary_addr.port());
    let handle = thread::spawn(move || server.serve_forever());

    let primary = get_http(primary_addr, "/manifest.json");
    let extra = get_http(extra_addr, "/manifest.json");
    let close_response = post_sync(primary_addr, r#"{"close":true}"#);
    assert_sync_post_ack(&close_response, 1);
    join_present_server(handle, Duration::from_secs(3));

    assert!(primary.contains("200 OK"));
    assert!(primary.contains("application/json"));
    assert!(primary.contains(r#"{"version":1}"#));
    assert!(extra.contains("200 OK"));
    assert!(extra.contains("application/json"));
    assert!(extra.contains(r#"{"version":1}"#));
}

#[test]
fn present_server_serves_new_root_after_swap_root() {
    let dir = tempdir().unwrap();
    let old_root = dir.path().join("old");
    let new_root = dir.path().join("new");
    fs::create_dir_all(&old_root).unwrap();
    fs::create_dir_all(&new_root).unwrap();
    fs::write(old_root.join("index.html"), "<!doctype html>old root").unwrap();
    fs::write(new_root.join("index.html"), "<!doctype html>new root").unwrap();

    let server = peitho::server::PresentServer::bind(old_root, 0, "index.html").unwrap();
    let addr = server.addr();
    let serving = server.clone();
    let handle = thread::spawn(move || {
        serving.handle_one();
        serving.handle_one();
    });

    let first = get_http(addr, "/");
    server.swap_root(new_root);
    let second = get_http(addr, "/");
    handle.join().unwrap();

    assert!(first.contains("200 OK"));
    assert!(first.contains("old root"));
    assert!(second.contains("200 OK"));
    assert!(second.contains("new root"));
}

#[test]
fn present_server_relays_sync_post_to_long_poll_subscriber() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.serve_forever());

    let mut poll = TcpStream::connect(addr).unwrap();
    poll.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    poll.write_all(b"GET /sync?seq=0 HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();

    let post_response = post_sync(addr, r#"{"index":1}"#);
    assert_sync_post_ack(&post_response, 1);

    let event = read_until_contains(&mut poll, r#""swapped":false"#);
    assert!(event.contains("200 OK"));
    assert!(event.contains("application/json"));
    assert!(event.contains(r#""seq":1"#));
    assert!(event.contains(r#""message":{"index":1}"#));
    assert!(event.contains(r#""index":1"#));
    assert!(event.contains(r#""swapped":false"#));
    assert!(event.contains(r#""generation":0"#));

    drop(poll);
    drop(handle);
}

#[test]
fn present_server_poll_response_includes_current_replay_state() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.serve_forever());

    let swap_response = post_sync(addr, r#"{"swapped":true}"#);
    assert_sync_post_ack(&swap_response, 1);

    let mut poll = TcpStream::connect(addr).unwrap();
    poll.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    poll.write_all(b"GET /sync?seq=1 HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();

    let index_response = post_sync(addr, r#"{"index":3}"#);
    assert_sync_post_ack(&index_response, 2);

    let event = read_until_contains(&mut poll, r#""swapped":true"#);
    assert!(event.contains("200 OK"));
    assert!(event.contains("application/json"));
    assert!(event.contains(r#""seq":2"#));
    assert!(event.contains(r#""message":{"index":3}"#));
    assert!(event.contains(r#""index":3"#));
    assert!(event.contains(r#""swapped":true"#));
    assert!(event.contains(r#""generation":0"#));

    drop(poll);
    drop(handle);
}

#[test]
fn present_server_relays_swap_sync_post_to_long_poll_subscriber() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.serve_forever());

    let mut poll = TcpStream::connect(addr).unwrap();
    poll.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    poll.write_all(b"GET /sync?seq=0 HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();

    let post_response = post_sync(addr, r#"{"swapped":true}"#);
    assert_sync_post_ack(&post_response, 1);

    let event = read_until_contains(&mut poll, r#""index":null"#);
    assert!(event.contains("200 OK"));
    assert!(event.contains("application/json"));
    assert!(event.contains(r#""seq":1"#));
    assert!(event.contains(r#""message":{"swapped":true}"#));
    assert!(event.contains(r#""index":null"#));
    assert!(event.contains(r#""swapped":true"#));
    assert!(event.contains(r#""generation":0"#));

    drop(poll);
    drop(handle);
}

#[test]
fn present_server_relays_close_sync_post_to_long_poll_subscriber() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.serve_forever());

    let mut poll = TcpStream::connect(addr).unwrap();
    poll.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    poll.write_all(b"GET /sync?seq=0 HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();

    let post_response = post_sync(addr, r#"{"close":true}"#);
    assert_sync_post_ack(&post_response, 1);

    let event = read_until_contains(&mut poll, r#""message":{"close":true}"#);
    assert!(event.contains("200 OK"));
    assert!(event.contains(r#""seq":1"#));
    assert!(event.contains(r#""message":{"close":true}"#));
    assert!(event.contains(r#""generation":0"#));

    drop(poll);
    drop(handle);
}

#[test]
fn present_server_broadcast_reload_reaches_long_poll_subscriber() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let seq = server.broadcast_reload();
    let handle = thread::spawn(move || server.handle_one());

    let response = get_http(addr, "/sync?seq=0");
    let body: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();

    assert!(response.contains("200 OK"));
    assert_eq!(seq, 1);
    assert_eq!(body["seq"], 1);
    assert!(body["message"].is_null());
    assert!(body["index"].is_null());
    assert_eq!(body["swapped"], false);
    assert_eq!(body["generation"], 1);
    handle.join().unwrap();
}

#[test]
fn present_server_reload_does_not_change_handshake_replay_state() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    assert_eq!(server.broadcast_reload(), 1);
    let handle = thread::spawn(move || server.handle_one());

    let response = get_http(addr, "/sync");
    let body: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();

    assert!(response.contains("200 OK"));
    assert_eq!(body["seq"], 1);
    assert!(body["message"].is_null());
    assert!(body["index"].is_null());
    assert_eq!(body["swapped"], false);
    assert_eq!(body["generation"], 1);
    handle.join().unwrap();
}

#[test]
fn present_server_shuts_down_after_close_sync_post() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.serve_forever());

    let post_response = post_sync(addr, r#"{"close":true}"#);
    assert_sync_post_ack(&post_response, 1);

    join_present_server(handle, Duration::from_secs(3));
}

#[test]
fn present_server_with_extra_listener_shuts_down_after_primary_close_sync_post() {
    assert_dual_listener_shutdown(DualListenerCloseTarget::Primary);
}

#[test]
fn present_server_with_extra_listener_shuts_down_after_extra_close_sync_post() {
    assert_dual_listener_shutdown(DualListenerCloseTarget::Extra);
}

#[test]
fn present_server_sync_handshake_returns_current_seq_without_replaying_latest_message() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || {
        server.handle_one();
        server.handle_one();
    });

    let post_response = post_sync(addr, r#"{"close":true}"#);
    assert_sync_post_ack(&post_response, 1);

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .write_all(b"GET /sync HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    assert!(response.contains("200 OK"));
    assert!(response.contains(r#""seq":1"#));
    assert!(response.contains(r#""message":null"#));
    assert!(response.contains(r#""index":null"#));
    assert!(response.contains(r#""swapped":false"#));
    assert!(response.contains(r#""generation":0"#));
    assert!(!response.contains(r#""close":true"#));
    handle.join().unwrap();
}

#[test]
fn present_server_sync_initial_handshake_returns_replay_state_defaults() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = get_http(addr, "/sync");
    let body: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();

    assert_eq!(body["seq"], 0);
    assert!(body["message"].is_null());
    assert!(body["index"].is_null());
    assert_eq!(body["swapped"], false);
    assert_eq!(body["generation"], 0);
    handle.join().unwrap();
}

#[test]
fn present_server_sync_handshake_replays_index_and_swapped_after_broadcasts() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || {
        server.handle_one();
        server.handle_one();
        server.handle_one();
    });

    let index_response = post_sync(addr, r#"{"index":2}"#);
    assert_sync_post_ack(&index_response, 1);
    let swap_response = post_sync(addr, r#"{"swapped":true}"#);
    assert_sync_post_ack(&swap_response, 2);
    let response = get_http(addr, "/sync");
    let body: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();

    assert_eq!(body["seq"], 2);
    assert!(body["message"].is_null());
    assert_eq!(body["index"], 2);
    assert_eq!(body["swapped"], true);
    assert_eq!(body["generation"], 0);
    handle.join().unwrap();
}

#[test]
fn present_server_sync_close_does_not_clobber_replay_state() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || {
        server.handle_one();
        server.handle_one();
        server.handle_one();
        server.handle_one();
    });

    let index_response = post_sync(addr, r#"{"index":2}"#);
    assert_sync_post_ack(&index_response, 1);
    let swap_response = post_sync(addr, r#"{"swapped":true}"#);
    assert_sync_post_ack(&swap_response, 2);
    let close_response = post_sync(addr, r#"{"close":true}"#);
    assert_sync_post_ack(&close_response, 3);
    let response = get_http(addr, "/sync?seq=");
    let body: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();

    assert_eq!(body["seq"], 3);
    assert!(body["message"].is_null());
    assert_eq!(body["index"], 2);
    assert_eq!(body["swapped"], true);
    assert_eq!(body["generation"], 0);
    handle.join().unwrap();
}

#[test]
fn present_server_rejects_invalid_sync_post_body() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = post_sync(addr, r#"{"key":"x"}"#);

    assert!(response.contains("400 Bad Request"));
    handle.join().unwrap();
}

#[test]
fn present_server_rejects_reload_sync_post_body() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = post_sync(addr, r#"{"reload":true}"#);

    assert!(response.contains("400 Bad Request"));
    handle.join().unwrap();
}

#[test]
fn present_server_rejects_non_boolean_swap_sync_post_body() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = post_sync(addr, r#"{"swapped":"x"}"#);

    assert!(response.contains("400 Bad Request"));
    handle.join().unwrap();
}

#[test]
fn present_server_rejects_swap_sync_post_body_with_extra_fields() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let response = post_sync(addr, r#"{"swapped":true,"extra":1}"#);

    assert!(response.contains("400 Bad Request"));
    handle.join().unwrap();
}

#[test]
fn present_server_rejects_mixed_sync_post_body() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0, "present.html").unwrap();
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
    let (serving_tx, serving_rx) = mpsc::channel();
    let (lines_tx, lines_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut lines = Vec::new();
        let mut serving_tx = Some(serving_tx);
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.contains("serving presentation at") {
                if let Some(tx) = serving_tx.take() {
                    tx.send(line.clone()).unwrap();
                }
            }
            lines.push(line);
        }
        lines_tx.send(lines).unwrap();
    });
    let line = serving_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("present server did not print serving URL within 5 seconds");
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();
    let lines = lines_rx.recv().unwrap();

    assert!(line.contains("http://127.0.0.1:"));
    assert!(line.contains("/present.html"));
    let serving_index = lines
        .iter()
        .position(|captured| captured == &line)
        .expect("captured serving line");
    assert!(
        !lines[serving_index + 1..]
            .iter()
            .any(|line| contains_qr_half_block(line)),
        "plain present without --host printed QR-looking output: {lines:?}"
    );
}

#[test]
fn present_rehearsal_prints_recording_directory_after_serving_url() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    fs::write(
        &fixture.deck,
        deck_with_assets(
            "./layout.html",
            "<!-- {\"key\":\"arch-1\",\"section\":\"Setup\",\"time\":\"1m\"} -->\n# Architecture\n\nBody",
        ),
    )
    .unwrap();
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\nexport function serverSyncChannelFactory() {}\n",
    )
    .unwrap();

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("peitho"))
        .current_dir(dir.path())
        .args(fixture.present_args(&shell))
        .args(["--no-open", "--port", "0", "--rehearsal"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let (recording_tx, recording_rx) = mpsc::channel();
    let (lines_tx, lines_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut lines = Vec::new();
        let mut recording_tx = Some(recording_tx);
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line == "recording rehearsal to .peitho/rehearsals/" {
                if let Some(tx) = recording_tx.take() {
                    tx.send(()).unwrap();
                }
            }
            lines.push(line);
        }
        lines_tx.send(lines).unwrap();
    });
    recording_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("present server did not print rehearsal recording line within 5 seconds");
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();
    let lines = lines_rx.recv().unwrap();

    let serving_index = lines
        .iter()
        .position(|line| line.contains("serving presentation at"))
        .expect("captured serving line");
    assert_eq!(
        lines.get(serving_index + 1).map(String::as_str),
        Some("recording rehearsal to .peitho/rehearsals/"),
        "rehearsal line should immediately follow serving URL: {lines:?}"
    );
}

#[test]
fn present_host_prints_remote_qr_after_remote_control_lines() {
    let Some(lines) = capture_present_remote_output(&["--host", "0.0.0.0"]) else {
        return;
    };

    let remote_line_indexes = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.starts_with("remote control").then_some(index))
        .collect::<Vec<_>>();
    assert!(
        !remote_line_indexes.is_empty(),
        "expected remote control lines in {lines:?}"
    );
    let first_url = remote_control_url_from_line(&lines[remote_line_indexes[0]]);
    let blank_index = remote_line_indexes.last().unwrap() + 1;
    assert_eq!(lines[blank_index], "");
    assert_eq!(
        lines[blank_index + 1],
        lines[remote_line_indexes[0]].replacen("remote control", "scan to open", 1)
    );

    let expected_qr = peitho::qr::qr_unicode_lines(first_url).unwrap();
    assert_eq!(lines[blank_index + 2], expected_qr[0]);
    assert_eq!(lines[blank_index + 3], expected_qr[1]);
}

#[test]
fn present_bare_host_prints_single_remote_line_caption_and_qr() {
    let Some(lines) = capture_present_remote_output(&["--host"]) else {
        return;
    };

    let remote_line_indexes = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.starts_with("remote control").then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(
        remote_line_indexes.len(),
        1,
        "bare --host should print exactly one remote control line: {lines:?}"
    );
    let first_url = remote_control_url_from_line(&lines[remote_line_indexes[0]]);
    let blank_index = remote_line_indexes[0] + 1;
    assert_eq!(lines[blank_index], "");
    assert_eq!(
        lines[blank_index + 1],
        lines[remote_line_indexes[0]].replacen("remote control", "scan to open", 1)
    );

    let expected_qr = peitho::qr::qr_unicode_lines(first_url).unwrap();
    assert_eq!(lines[blank_index + 2], expected_qr[0]);
    assert_eq!(lines[blank_index + 3], expected_qr[1]);
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
    assert_sync_post_ack(&post_response, 1);

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

enum DualListenerCloseTarget {
    Primary,
    Extra,
}

fn assert_dual_listener_shutdown(target: DualListenerCloseTarget) {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let host = IpAddr::V6(Ipv6Addr::LOCALHOST);
    let server = peitho::server::PresentServer::bind_with_remote_assets(
        cache,
        0,
        "present.html",
        Some(host),
        false,
    )
    .unwrap();
    let primary_addr = server.addr();
    let extra_addr = SocketAddr::new(host, primary_addr.port());
    let close_addr = match target {
        DualListenerCloseTarget::Primary => primary_addr,
        DualListenerCloseTarget::Extra => extra_addr,
    };
    let handle = thread::spawn(move || server.serve_forever());

    let post_response = post_sync(close_addr, r#"{"close":true}"#);
    assert_sync_post_ack(&post_response, 1);

    join_present_server(handle, Duration::from_secs(3));
}

#[test]
fn repository_example_present_no_serve_smoke() {
    let shell = workspace_root().join("packages/peitho-present/dist/shell.js");
    assert!(shell.exists(), "shell bundle not built; run npm run build");
    let dir = tempdir().unwrap();
    let deck = write_repository_example_deck_with_assets(dir.path());

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args(["present", deck.to_str().unwrap(), "--no-serve", "--no-open"])
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
    assert!(shell_js.contains("installCanvasScaler"));
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
}

impl Fixture {
    fn write(root: &std::path::Path) -> Self {
        let deck = root.join("deck.md");
        let layout = root.join("layout.html");
        let css_dir = root.join("css");
        fs::create_dir_all(&css_dir).unwrap();
        fs::write(
            &deck,
            deck_with_assets(
                "./layout.html",
                "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody",
            ),
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
        Self { deck }
    }

    fn present_args(&self, shell: &std::path::Path) -> Vec<OsString> {
        let mut args = self.present_args_builtin_shell();
        args.push(OsString::from("--shell"));
        args.push(shell.as_os_str().to_owned());
        args
    }

    fn present_args_builtin_shell(&self) -> Vec<OsString> {
        vec![OsString::from("present"), self.deck.as_os_str().to_owned()]
    }
}

fn deck_with_assets(layouts: &str, body: &str) -> String {
    format!("---\nlayouts: {layouts}\ncss: ./css\n---\n{body}")
}

fn write_repository_example_deck_with_assets(dir: &Path) -> PathBuf {
    let root = workspace_root();
    let deck = dir.join("deck.md");
    let body = fs::read_to_string(root.join("examples/minimal/deck.md")).unwrap();
    fs::write(
        &deck,
        format!(
            "---\nlayouts: {}\ncss: {}\n---\n{body}",
            root.join("layouts/title-body-code.html").display(),
            root.join("themes/base.css").display()
        ),
    )
    .unwrap();
    deck
}

fn write_layout_dir(root: &Path, name: &str, class: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("statement.html"),
        format!(
            r#"<section class="{class}"><h1><slot name="title" accepts="inline" arity="1"></slot></h1><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#
        ),
    )
    .unwrap();
    dir
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

fn post_sync(addr: SocketAddr, body: &str) -> String {
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

fn assert_sync_post_ack(response: &str, seq: u64) {
    assert!(response.contains("200 OK"));
    assert!(response.contains("application/json"));
    let body: serde_json::Value = serde_json::from_str(response_body(response)).unwrap();
    assert_eq!(body["seq"], seq);
}

fn get_http(addr: SocketAddr, path: &str) -> String {
    String::from_utf8(get_http_bytes(addr, path)).unwrap()
}

fn get_http_bytes(addr: SocketAddr, path: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    response
}

fn response_body(response: &str) -> &str {
    let (_, body) = split_http_response(response.as_bytes());
    std::str::from_utf8(body).unwrap()
}

fn response_head(response: &[u8]) -> String {
    let (head, _) = split_http_response(response);
    String::from_utf8_lossy(head).into_owned()
}

fn response_body_bytes(response: &[u8]) -> &[u8] {
    split_http_response(response).1
}

fn split_http_response(response: &[u8]) -> (&[u8], &[u8]) {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (&response[..index], &response[index + 4..]))
        .unwrap_or((response, response))
}

fn capture_present_remote_output(host_args: &[&str]) -> Option<Vec<String>> {
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
        .args(host_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut lines = Vec::new();
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            let done =
                line.contains('█') || line.contains("no non-loopback network addresses found");
            lines.push(line);
            if done {
                tx.send(lines).unwrap();
                break;
            }
        }
    });

    let lines = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(lines) => lines,
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = child.kill();
            let status = child.wait().unwrap();
            reader.join().unwrap();
            let mut stderr_text = String::new();
            stderr.read_to_string(&mut stderr_text).unwrap();
            if !status.success()
                && (stderr_text.contains("failed to bind present server")
                    || stderr_text.contains("no non-loopback network address found for --host"))
            {
                eprintln!("skipping QR assertion: {stderr_text}");
                return None;
            }
            panic!("present server exited before printing remote control output: {stderr_text}");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            child.kill().unwrap();
            child.wait().unwrap();
            reader.join().unwrap();
            panic!("present server did not print remote control output within 5 seconds");
        }
    };
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();

    if lines
        .iter()
        .any(|line| line.contains("no non-loopback network addresses found"))
    {
        eprintln!("skipping QR assertion: no non-loopback network addresses found");
        return None;
    }

    Some(lines)
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

fn serving_addr(line: &str) -> SocketAddr {
    let prefix = "serving presentation at http://";
    let rest = line
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("unexpected serving URL line: {line}"));
    let host_port = rest
        .strip_suffix("/present.html")
        .unwrap_or_else(|| panic!("unexpected serving URL line: {line}"));
    host_port.parse().unwrap()
}

fn remote_control_url_from_line(line: &str) -> &str {
    let start = line
        .find("http://")
        .unwrap_or_else(|| panic!("remote control line did not contain a URL: {line}"));
    &line[start..]
}

fn contains_qr_half_block(line: &str) -> bool {
    line.contains('█') || line.contains('▀') || line.contains('▄')
}
