use std::{env, fs, path::PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
#[ignore]
// Chromeを実行するE2Eテスト。cargo test -- --ignored で明示的に実行。
fn export_pdf_writes_nonempty_pdf() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping export_pdf_writes_nonempty_pdf: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("out.pdf");
    fs::write(
        &deck,
        "---\naspect_ratio: 16:9\nresolution: 1920x1080\n---\n# Exported PDF\n\n<!-- speaker secret -->\n",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .args(["export", "pdf"])
        .arg(&deck)
        .args(["-o"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("exported 1 slide(s)"));

    let bytes = fs::read(&out).unwrap();
    assert!(bytes.len() > 4);
    assert_eq!(&bytes[..5], b"%PDF-");
}

fn test_chrome_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("PEITHO_CHROME_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }

    let mac_chrome = PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    if mac_chrome.is_file() {
        return Some(mac_chrome);
    }

    for program in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ] {
        if let Some(path) = find_in_path(program) {
            return Some(path);
        }
    }

    None
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(program);
        candidate.is_file().then_some(candidate)
    })
}
