use std::{
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::Duration,
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn present_no_serve_writes_clean_present_cache() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\n",
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
    assert!(cache.join("slides/000-arch-1.html").exists());
    assert!(fs::read_to_string(cache.join("notes.json"))
        .unwrap()
        .contains(r#""notes": {}"#));
    assert!(fs::read_to_string(cache.join("present.html"))
        .unwrap()
        .contains("installSyncBridge(window)"));
}

#[test]
fn present_no_serve_writes_presenter_html() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\nexport function mountPresenterView() {}\n",
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
    assert!(fs::read_to_string(cache.join("present.html"))
        .unwrap()
        .contains("Presenter view"));
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
    let handle = thread::spawn(move || server.handle_one().unwrap());
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
fn present_no_open_server_prints_assigned_url() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\n",
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
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.contains("serving presentation at") {
                tx.send(line).unwrap();
                break;
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
fn repository_example_present_no_serve_smoke() {
    let shell = workspace_root().join("packages/peitho-present/dist/shell.js");
    assert!(shell.exists(), "shell bundle not built; run npm run build");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "present",
            "examples/deck.md",
            "--template",
            "templates/title-body-code.html",
            "--base-css",
            "themes/base.css",
            "--overrides-css",
            "themes/overrides.css",
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
    assert!(fs::read_to_string(cache.join("shell.js"))
        .unwrap()
        .contains("mountPresenterView"));
}

struct Fixture {
    deck: std::path::PathBuf,
    template: std::path::PathBuf,
    base: std::path::PathBuf,
    overrides: std::path::PathBuf,
}

impl Fixture {
    fn write(root: &std::path::Path) -> Self {
        let deck = root.join("deck.md");
        let template = root.join("template.html");
        let base = root.join("base.css");
        let overrides = root.join("overrides.css");
        fs::write(
            &deck,
            "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody",
        )
        .unwrap();
        fs::write(
            &template,
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot></section>"#,
        )
        .unwrap();
        fs::write(&base, ".slot-title { color: red; }").unwrap();
        fs::write(
            &overrides,
            r#"[data-slide-key="arch-1"] .slot-title { color: blue; }"#,
        )
        .unwrap();
        Self {
            deck,
            template,
            base,
            overrides,
        }
    }

    fn present_args(&self, shell: &std::path::Path) -> Vec<OsString> {
        vec![
            OsString::from("present"),
            self.deck.as_os_str().to_owned(),
            OsString::from("--template"),
            self.template.as_os_str().to_owned(),
            OsString::from("--base-css"),
            self.base.as_os_str().to_owned(),
            OsString::from("--overrides-css"),
            self.overrides.as_os_str().to_owned(),
            OsString::from("--shell"),
            shell.as_os_str().to_owned(),
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
