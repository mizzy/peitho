use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

mod util;
use util::test_chrome_path;

mod embed_cache_key_test_vector {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../peitho-core/tests/support/embed_cache_key.rs"
    ));
}

const STATUS_URL: &str = "https://x.com/gosukenator/status/2083825695709597710";
const PNG_FIXTURE: &[u8] = b"\x89PNG\r\n\x1a\nfixture tweet";

fn write_tweet_deck(path: &Path) {
    let layouts = path.parent().unwrap().join("layouts");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        layouts.join("title-image.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot><slot name="image" accepts="image" arity="1"></slot></section>"#,
    )
    .unwrap();
    fs::write(path, format!("# Tweet\n\n```embed\n{STATUS_URL}\n```\n")).unwrap();
}

fn png_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn emitted_html(out: &Path) -> String {
    let mut html = fs::read_to_string(out.join("index.html")).unwrap();
    for entry in fs::read_dir(out.join("slides")).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "html")
        {
            html.push_str(&fs::read_to_string(path).unwrap());
        }
    }
    html
}

#[test]
fn build_uses_cached_tweet_embed_without_chrome() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let cache_dir = dir.path().join(".peitho/embeds-cache");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(
        cache_dir.join(format!(
            "{}.png",
            embed_cache_key_test_vector::PINNED_BUILTIN_EMBED_CACHE_KEY
        )),
        PNG_FIXTURE,
    )
    .unwrap();
    write_tweet_deck(&deck);

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .arg("build")
        .arg(&deck)
        .arg("--out")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("built 1 slide"));

    assert_eq!(png_files(&out.join("assets")).len(), 1);
    let html = emitted_html(&out);
    assert!(!html.contains("platform.x.com"));
    assert!(!html.contains("widgets.js"));
    assert!(!html.contains("twitter-tweet"));
    assert!(!out.join(".peitho").exists());
}

#[test]
fn build_reports_cache_miss_when_chrome_is_unavailable() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let cache_path = dir.path().join(".peitho/embeds-cache").join(format!(
        "{}.png",
        embed_cache_key_test_vector::PINNED_BUILTIN_EMBED_CACHE_KEY
    ));
    write_tweet_deck(&deck);

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .arg("build")
        .arg(&deck)
        .arg("--out")
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 3"))
        .stderr(predicate::str::contains("https://x.com/"))
        .stderr(predicate::str::contains(
            "gosukenator/status/2083825695709597710",
        ))
        .stderr(predicate::str::contains(".peitho/embeds-"))
        .stderr(predicate::str::contains(
            cache_path.file_name().unwrap().to_string_lossy(),
        ))
        .stderr(predicate::str::contains("Chrome not found"))
        .stderr(predicate::str::contains("PEITHO_CHROME_PATH"))
        .stderr(predicate::str::contains("delete the cache file to refresh"));

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(
        stderr.matches("install Google Chrome or Chromium").count(),
        1,
        "actual stderr: {stderr}"
    );
}

#[test]
#[ignore]
fn build_renders_official_tweet_embed_png() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping build_renders_official_tweet_embed_png: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let cache_dir = dir.path().join(".peitho/embeds-cache");
    write_tweet_deck(&deck);

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("build")
        .arg(&deck)
        .arg("--out")
        .arg(&out)
        .assert()
        .success();

    let cache_pngs = png_files(&cache_dir);
    assert_eq!(cache_pngs.len(), 1);
    assert_eq!(
        &fs::read(&cache_pngs[0]).unwrap()[..8],
        b"\x89PNG\r\n\x1a\n"
    );
    let dist_pngs = png_files(&out.join("assets"));
    assert_eq!(dist_pngs.len(), 1);
    assert_eq!(&fs::read(&dist_pngs[0]).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    let html = emitted_html(&out);
    assert!(html.contains(r#"<img src="assets/"#));
    assert!(!html.contains("platform.x.com"));
    assert!(!html.contains("widgets.js"));
    assert!(!html.contains("twitter-tweet"));
}
