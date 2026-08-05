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

mod embed_card_cache_key_test_vector {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../peitho-core/tests/support/embed_card_cache_key.rs"
    ));
}

const STATUS_URL: &str = "https://x.com/gosukenator/status/2083825695709597710";
const CARD_STATUS_URL: &str = "https://x.com/gosukenator/status/2074821309259973046";
const PNG_FIXTURE: &[u8] = b"\x89PNG\r\n\x1a\nfixture tweet";
const OEMBED_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../peitho-core/tests/fixtures/x-oembed-response.json"
));

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

fn write_embed_deck(path: &Path, blocks: &[(&str, &str)]) {
    let layouts = path.parent().unwrap().join("layouts");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        layouts.join("title-image.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot><slot name="image" accepts="image" arity="0..*"></slot></section>"#,
    )
    .unwrap();
    let mut markdown = "# Tweet\n\n".to_owned();
    for (url, option) in blocks {
        markdown.push_str("```embed\n");
        markdown.push_str(url);
        markdown.push('\n');
        if !option.is_empty() {
            markdown.push_str(option);
            markdown.push('\n');
        }
        markdown.push_str("```\n\n");
    }
    fs::write(path, markdown).unwrap();
}

fn seed_oembed_cache(deck_dir: &Path) -> PathBuf {
    let cache_dir = deck_dir.join(".peitho/embeds-cache");
    fs::create_dir_all(&cache_dir).unwrap();
    let path = cache_dir.join(format!(
        "{}.json",
        embed_card_cache_key_test_vector::PINNED_BUILTIN_EMBED_CARD_CACHE_KEY
    ));
    fs::write(&path, OEMBED_FIXTURE).unwrap();
    path
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
    html.push_str(&emitted_slide_html(out));
    html
}

fn emitted_slide_html(out: &Path) -> String {
    let mut html = String::new();
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

fn files_with_extension(dir: &Path, extension: &str) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut files = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|actual| actual == extension))
        .collect::<Vec<_>>();
    files.sort();
    files
}

#[test]
fn build_card_embed_from_cached_oembed_without_chrome() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    seed_oembed_cache(dir.path());
    write_embed_deck(&deck, &[(CARD_STATUS_URL, "mode: card")]);

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

    let html = emitted_slide_html(&out);
    assert!(html.contains("peitho-embed-card__tweet-text"), "{html}");
    assert!(html.contains("自前プレゼンツール"), "{html}");
    assert!(html.contains(r#"lang="ja" dir="ltr""#), "{html}");
    assert!(!html.contains("<blockquote"), "{html}");
    assert!(!html.contains("twitter-tweet"), "{html}");
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("provider_name"), "{html}");
    let css = fs::read_to_string(out.join("peitho.css")).unwrap();
    assert_eq!(css.matches(".peitho-embed-card {").count(), 1);
    let manifest = fs::read_to_string(out.join("manifest.json")).unwrap();
    assert!(manifest.contains("自前プレゼンツール"));
    assert!(files_with_extension(&out.join("assets"), "json").is_empty());
    assert!(!out.join(".peitho").exists());
}

#[test]
fn build_all_card_embeds_never_create_png_assets() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    seed_oembed_cache(dir.path());
    write_embed_deck(
        &deck,
        &[
            (CARD_STATUS_URL, "mode: card"),
            (CARD_STATUS_URL, "mode: card"),
        ],
    );

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .arg("build")
        .arg(&deck)
        .arg("--out")
        .arg(&out)
        .assert()
        .success();

    assert!(png_files(&dir.path().join(".peitho/embeds-cache")).is_empty());
    assert!(files_with_extension(&out.join("assets"), "png").is_empty());
    assert!(files_with_extension(&out.join("assets"), "json").is_empty());
    let html = emitted_html(&out);
    assert_eq!(html.matches(r#"class="peitho-embed-card""#).count(), 2);
    let css = fs::read_to_string(out.join("peitho.css")).unwrap();
    assert_eq!(css.matches(".peitho-embed-card {").count(), 1);
}

#[cfg(unix)]
#[test]
fn build_card_cache_miss_reports_curl_failure_with_refresh_help() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let bin = dir.path().join("bin");
    let curl = bin.join("curl");
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        &curl,
        "#!/bin/sh\nprintf 'fixture HTTP failure\\n' >&2\nexit 22\n",
    )
    .unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
    write_embed_deck(&deck, &[(CARD_STATUS_URL, "mode: card")]);
    Command::cargo_bin("peitho")
        .unwrap()
        .env("PATH", &bin)
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
            "gosukenator/status/2074821309259973046",
        ))
        .stderr(predicate::str::contains(
            embed_card_cache_key_test_vector::PINNED_BUILTIN_EMBED_CARD_CACHE_KEY,
        ))
        .stderr(predicate::str::contains("fixture HTTP failure"))
        .stderr(predicate::str::contains("works offline"))
        .stderr(predicate::str::contains("delete the cache file to refresh"));
}

#[test]
fn build_mixed_embed_modes_use_only_their_required_backends() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let cache_dir = dir.path().join(".peitho/embeds-cache");
    seed_oembed_cache(dir.path());
    fs::write(
        cache_dir.join(format!(
            "{}.png",
            embed_cache_key_test_vector::PINNED_BUILTIN_EMBED_CACHE_KEY
        )),
        PNG_FIXTURE,
    )
    .unwrap();
    write_embed_deck(
        &deck,
        &[
            (CARD_STATUS_URL, "mode: card"),
            (STATUS_URL, "mode: screenshot"),
        ],
    );

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .arg("build")
        .arg(&deck)
        .arg("--out")
        .arg(&out)
        .assert()
        .success();

    let html = emitted_html(&out);
    assert_eq!(html.matches(r#"class="peitho-embed-card""#).count(), 1);
    assert_eq!(html.matches(r#"<img src="assets/"#).count(), 1);
    assert_eq!(files_with_extension(&out.join("assets"), "png").len(), 1);
    assert!(files_with_extension(&out.join("assets"), "json").is_empty());
    assert_eq!(files_with_extension(&cache_dir, "json").len(), 1);
    assert_eq!(files_with_extension(&cache_dir, "png").len(), 1);
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
