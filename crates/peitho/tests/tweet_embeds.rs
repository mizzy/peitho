use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
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

mod generic_oembed_cache_key_test_vector {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../peitho-core/tests/support/generic_oembed_cache_key.rs"
    ));
}

const STATUS_URL: &str = "https://x.com/gosukenator/status/2083825695709597710";
const CARD_STATUS_URL: &str = "https://x.com/gosukenator/status/2074821309259973046";
const PNG_FIXTURE: &[u8] = b"\x89PNG\r\n\x1a\nfixture tweet";
const OEMBED_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../peitho-core/tests/fixtures/x-oembed-response.json"
));
const YOUTUBE_PAGE_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
const MASTODON_PAGE_URL: &str = "https://mastodon.social/@Mastodon/117004738413167978";
const YOUTUBE_OEMBED_FIXTURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../peitho-core/tests/fixtures/youtube-oembed-response.json"
));
const MASTODON_OEMBED_FIXTURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../peitho-core/tests/fixtures/mastodon-oembed-response.json"
));
const YOUTUBE_THUMBNAIL_FIXTURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../peitho-core/tests/fixtures/youtube-thumbnail.jpg"
));
const YOUTUBE_THUMBNAIL_URL: &str = "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg";

fn write_tweet_deck(path: &Path) {
    let layouts = path.parent().unwrap().join("layouts");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        layouts.join("title-image.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot><slot name="image" accepts="image" arity="1"></slot><slot name="footnotes" accepts="blocks" arity="0..1"></slot></section>"#,
    )
    .unwrap();
    fs::write(path, format!("# Tweet\n\n```embed\n{STATUS_URL}\n```\n")).unwrap();
}

fn write_embed_deck(path: &Path, blocks: &[(&str, Option<&str>)]) {
    let layouts = path.parent().unwrap().join("layouts");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        layouts.join("title-image.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot><slot name="image" accepts="image" arity="0..*"></slot><slot name="footnotes" accepts="blocks" arity="0..1"></slot></section>"#,
    )
    .unwrap();
    let mut markdown = "# Tweet\n\n".to_owned();
    for (url, options) in blocks {
        markdown.push_str("```embed");
        if let Some(options) = options {
            markdown.push(' ');
            markdown.push_str(options);
        }
        markdown.push('\n');
        markdown.push_str(url);
        markdown.push('\n');
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

fn generic_cache_key(domain: &[u8], normalized_page_url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(normalized_page_url.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn generic_json_cache_key(normalized_page_url: &str) -> String {
    generic_cache_key(b"\0peitho-generic-oembed\0", normalized_page_url)
}

fn generic_thumbnail_cache_key(normalized_page_url: &str) -> String {
    generic_cache_key(b"\0peitho-generic-oembed-thumbnail\0", normalized_page_url)
}

fn seed_generic_json_cache(deck_dir: &Path, page_url: &str, bytes: &[u8]) -> PathBuf {
    let cache_dir = deck_dir.join(".peitho/embeds-cache");
    fs::create_dir_all(&cache_dir).unwrap();
    let path = cache_dir.join(format!("{}.json", generic_json_cache_key(page_url)));
    fs::write(&path, bytes).unwrap();
    path
}

fn seed_generic_thumbnail_cache(
    deck_dir: &Path,
    page_url: &str,
    extension: &str,
    bytes: &[u8],
) -> PathBuf {
    let cache_dir = deck_dir.join(".peitho/embeds-cache");
    fs::create_dir_all(&cache_dir).unwrap();
    let path = cache_dir.join(format!(
        "{}.{}",
        generic_thumbnail_cache_key(page_url),
        extension
    ));
    fs::write(&path, bytes).unwrap();
    path
}

fn seed_youtube_generic_caches(deck_dir: &Path) -> (PathBuf, PathBuf) {
    assert_eq!(
        generic_json_cache_key(YOUTUBE_PAGE_URL),
        generic_oembed_cache_key_test_vector::PINNED_GENERIC_OEMBED_JSON_CACHE_KEY
    );
    assert_eq!(
        generic_thumbnail_cache_key(YOUTUBE_PAGE_URL),
        generic_oembed_cache_key_test_vector::PINNED_GENERIC_OEMBED_THUMBNAIL_CACHE_KEY
    );
    (
        seed_generic_json_cache(deck_dir, YOUTUBE_PAGE_URL, YOUTUBE_OEMBED_FIXTURE),
        seed_generic_thumbnail_cache(deck_dir, YOUTUBE_PAGE_URL, "jpg", YOUTUBE_THUMBNAIL_FIXTURE),
    )
}

fn empty_path_dir(root: &Path) -> PathBuf {
    let bin = root.join("empty-bin");
    fs::create_dir_all(&bin).unwrap();
    bin
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

#[cfg(unix)]
fn write_thumbnail_curl_spy(root: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let bin = root.join("bin");
    let curl = bin.join("curl");
    let calls = root.join("curl-calls.txt");
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        &curl,
        "#!/bin/sh\n/usr/bin/printf '%s\\n' \"$*\" >> \"$CALLS_FILE\"\n/bin/cat \"$THUMBNAIL_FIXTURE\"\n",
    )
    .unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
    (bin, calls)
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

fn without_miette_line_wrapping(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(|line| line.trim_start_matches([' ', '│']))
        .collect()
}

#[test]
fn build_cached_youtube_generic_embed_offline_without_chrome_or_curl() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let empty_path = empty_path_dir(dir.path());
    seed_youtube_generic_caches(dir.path());
    write_embed_deck(&deck, &[(YOUTUBE_PAGE_URL, None)]);

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PATH", &empty_path)
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("built 1 slide"));

    let html = emitted_slide_html(&out);
    assert!(html.contains("peitho-generic-embed-card"), "{html}");
    assert!(
        html.contains(
            r#"<span class="peitho-generic-embed-card__caption"><span class="peitho-generic-embed-card__title">Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster)</span><span class="peitho-generic-embed-card__meta"><span class="peitho-generic-embed-card__author">Rick Astley</span><span class="peitho-generic-embed-card__separator" aria-hidden="true"> · </span><span class="peitho-generic-embed-card__provider">YouTube</span></span></span>"#
        ),
        "{html}"
    );
    assert!(html.contains(YOUTUBE_PAGE_URL), "{html}");
    assert!(html.contains(r#"src="assets/"#), "{html}");
    assert!(!html.contains(YOUTUBE_THUMBNAIL_URL), "{html}");
    assert!(!html.contains("<iframe"), "{html}");
    assert!(!html.contains("<script"), "{html}");
    assert_eq!(files_with_extension(&out.join("assets"), "jpg").len(), 1);
    assert!(!out.join(".peitho").exists());
}

#[test]
fn build_cached_mastodon_text_card_offline_without_image_asset() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let empty_path = empty_path_dir(dir.path());
    seed_generic_json_cache(dir.path(), MASTODON_PAGE_URL, MASTODON_OEMBED_FIXTURE);
    write_embed_deck(&deck, &[(MASTODON_PAGE_URL, None)]);

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PATH", &empty_path)
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success();

    let html = emitted_html(&out);
    assert!(html.contains("peitho-generic-embed-card"), "{html}");
    assert!(
        html.contains(
            r#"<span class="peitho-generic-embed-card__caption"><span class="peitho-generic-embed-card__meta"><span class="peitho-generic-embed-card__author">Mastodon</span><span class="peitho-generic-embed-card__separator" aria-hidden="true"> · </span><span class="peitho-generic-embed-card__provider">mastodon.social</span></span></span>"#
        ),
        "{html}"
    );
    assert!(html.contains(MASTODON_PAGE_URL), "{html}");
    assert!(
        !html.contains("peitho-generic-embed-card__thumbnail"),
        "{html}"
    );
    for extension in ["jpg", "jpeg", "png", "webp", "gif"] {
        assert!(files_with_extension(&out.join("assets"), extension).is_empty());
    }
}

#[cfg(unix)]
#[test]
fn build_generic_json_hit_thumbnail_miss_fetches_only_image() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let (bin, calls) = write_thumbnail_curl_spy(dir.path());
    seed_generic_json_cache(dir.path(), YOUTUBE_PAGE_URL, YOUTUBE_OEMBED_FIXTURE);
    write_embed_deck(&deck, &[(YOUTUBE_PAGE_URL, None)]);

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PATH", &bin)
        .env("CALLS_FILE", &calls)
        .env(
            "THUMBNAIL_FIXTURE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../peitho-core/tests/fixtures/youtube-thumbnail.jpg"
            ),
        )
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success();

    let calls = fs::read_to_string(calls).unwrap();
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.contains(YOUTUBE_THUMBNAIL_URL), "{calls}");
    assert!(!calls.contains(YOUTUBE_PAGE_URL), "{calls}");
    assert!(!calls.contains("oembed"), "{calls}");
    assert!(dir
        .path()
        .join(".peitho/embeds-cache")
        .join(format!(
            "{}.jpg",
            generic_oembed_cache_key_test_vector::PINNED_GENERIC_OEMBED_THUMBNAIL_CACHE_KEY
        ))
        .is_file());
}

#[cfg(unix)]
#[test]
fn build_generic_card_publishes_hashed_thumbnail_not_cache_path() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let probe = dir.path().join("published.txt");
    seed_youtube_generic_caches(dir.path());
    write_embed_deck(&deck, &[(YOUTUBE_PAGE_URL, None)]);

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success();

    let assets = files_with_extension(&out.join("assets"), "jpg");
    assert_eq!(assets.len(), 1);
    let published_rel = format!(
        "assets/{}",
        assets[0].file_name().unwrap().to_string_lossy()
    );
    assert!(
        published_rel.contains(
            generic_oembed_cache_key_test_vector::PINNED_GENERIC_OEMBED_THUMBNAIL_CACHE_KEY
        ),
        "{published_rel}"
    );
    let html = emitted_html(&out);
    assert!(html.contains(&published_rel), "{html}");
    assert!(!html.contains(".peitho/embeds-cache"), "{html}");
    let manifest = fs::read_to_string(out.join("manifest.json")).unwrap();
    assert!(manifest.contains(&published_rel), "{manifest}");

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&out)
        .args([
            "--",
            "/bin/sh",
            "-c",
            "test -f \"$PEITHO_DIST/$1\" && printf published > \"$2\"",
            "peitho-test",
            &published_rel,
        ])
        .arg(&probe)
        .assert()
        .success();
    assert_eq!(fs::read_to_string(probe).unwrap(), "published");
}

#[test]
fn build_generic_provider_html_never_enters_dist() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    seed_generic_json_cache(dir.path(), MASTODON_PAGE_URL, MASTODON_OEMBED_FIXTURE);
    write_embed_deck(&deck, &[(MASTODON_PAGE_URL, None)]);

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success();

    let generated = format!(
        "{}\n{}\n{}",
        emitted_slide_html(&out),
        fs::read_to_string(out.join("peitho.css")).unwrap(),
        fs::read_to_string(out.join("manifest.json")).unwrap()
    );
    for forbidden in [
        "mastodon-embed",
        "embed.js",
        "<blockquote",
        "<script",
        "data-allowed-prefixes",
    ] {
        assert!(
            !generated.contains(forbidden),
            "found {forbidden}: {generated}"
        );
    }
    assert!(files_with_extension(&out.join("assets"), "json").is_empty());
    assert!(!out.join(".peitho").exists());
}

#[cfg(unix)]
#[test]
fn build_generic_failures_report_translated_line_and_exact_refresh_files() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let shared = dir.path().join("shared.md");
    let out = dir.path().join("dist");
    let bin = dir.path().join("bin");
    let curl = bin.join("curl");
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        &curl,
        "#!/bin/sh\nprintf 'fixture thumbnail failure\\n' >&2\nexit 22\n",
    )
    .unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
    write_embed_deck(&deck, &[]);
    fs::write(&deck, "<!-- {\"include\":\"shared.md\"} -->\n").unwrap();
    fs::write(
        &shared,
        format!("# Included\n\n```embed\n{YOUTUBE_PAGE_URL}\n```\n"),
    )
    .unwrap();
    let json_path = seed_generic_json_cache(dir.path(), YOUTUBE_PAGE_URL, YOUTUBE_OEMBED_FIXTURE);
    let thumbnail_key = generic_thumbnail_cache_key(YOUTUBE_PAGE_URL);

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .env("PATH", &bin)
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("shared.md:3"))
        .stderr(predicate::str::contains(YOUTUBE_PAGE_URL))
        .stderr(predicate::str::contains("fixture thumbnail failure"))
        .stderr(predicate::str::contains("work offline"));
    let stderr = without_miette_line_wrapping(&assert.get_output().stderr);
    assert!(
        stderr.contains(&json_path.display().to_string()),
        "missing exact refresh path {} in {stderr}",
        json_path.display()
    );
    for extension in ["jpg", "png", "webp", "gif"] {
        let candidate = dir
            .path()
            .join(".peitho/embeds-cache")
            .join(format!("{thumbnail_key}.{extension}"));
        assert!(
            stderr.contains(&candidate.display().to_string()),
            "missing exact refresh path {} in {}",
            candidate.display(),
            stderr
        );
    }
}

#[cfg(unix)]
#[test]
fn build_mixed_x_and_generic_embeds_touch_only_required_backends() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let cache_dir = dir.path().join(".peitho/embeds-cache");
    let (bin, calls) = write_thumbnail_curl_spy(dir.path());
    seed_oembed_cache(dir.path());
    fs::write(
        cache_dir.join(format!(
            "{}.png",
            embed_cache_key_test_vector::PINNED_BUILTIN_EMBED_CACHE_KEY
        )),
        PNG_FIXTURE,
    )
    .unwrap();
    seed_generic_json_cache(dir.path(), YOUTUBE_PAGE_URL, YOUTUBE_OEMBED_FIXTURE);
    write_embed_deck(
        &deck,
        &[
            (CARD_STATUS_URL, Some("mode=card")),
            (STATUS_URL, Some("mode=screenshot")),
            (YOUTUBE_PAGE_URL, None),
        ],
    );

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PATH", &bin)
        .env("CALLS_FILE", &calls)
        .env(
            "THUMBNAIL_FIXTURE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../peitho-core/tests/fixtures/youtube-thumbnail.jpg"
            ),
        )
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success();

    let calls = fs::read_to_string(calls).unwrap();
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.contains(YOUTUBE_THUMBNAIL_URL), "{calls}");
    assert!(!calls.contains("publish.x.com"), "{calls}");
    assert!(!calls.contains(YOUTUBE_PAGE_URL), "{calls}");
    let html = emitted_html(&out);
    assert_eq!(html.matches(r#"class="peitho-embed-card""#).count(), 2);
    assert_eq!(files_with_extension(&out.join("assets"), "png").len(), 1);
    assert_eq!(files_with_extension(&out.join("assets"), "jpg").len(), 1);
}

#[test]
fn tweet_embed_example_builds_three_slides_offline_without_chrome_or_curl() {
    let dir = tempdir().unwrap();
    let root = workspace_root();
    let deck = root.join("examples/tweet-embed/deck.md");
    let out = dir.path().join("dist");
    let empty_path = empty_path_dir(dir.path());

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(&root)
        .env("PATH", &empty_path)
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("built 3 slide(s)"));

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["slideCount"], 3);
    assert_eq!(manifest["slides"][0]["key"], "snapshotted-at-build-time");
    assert!(emitted_slide_html(&out).contains("peitho-generic-embed-card"));
}

#[test]
fn tweet_embed_example_generic_slide_uses_committed_json_and_jpeg_caches() {
    let dir = tempdir().unwrap();
    let root = workspace_root();
    let example = root.join("examples/tweet-embed");
    let cache_dir = example.join(".peitho/embeds-cache");
    let json_cache = cache_dir.join(format!(
        "{}.json",
        generic_oembed_cache_key_test_vector::PINNED_GENERIC_OEMBED_JSON_CACHE_KEY
    ));
    let jpeg_cache = cache_dir.join(format!(
        "{}.jpg",
        generic_oembed_cache_key_test_vector::PINNED_GENERIC_OEMBED_THUMBNAIL_CACHE_KEY
    ));

    assert_eq!(fs::read(&json_cache).unwrap(), YOUTUBE_OEMBED_FIXTURE);
    assert_eq!(fs::read(&jpeg_cache).unwrap(), YOUTUBE_THUMBNAIL_FIXTURE);

    let out = dir.path().join("dist");
    let empty_path = empty_path_dir(dir.path());
    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(&root)
        .env("PATH", &empty_path)
        .env("PEITHO_CHROME_PATH", dir.path().join("missing-chrome"))
        .args(["build", example.join("deck.md").to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success();

    let jpeg_assets = files_with_extension(&out.join("assets"), "jpg");
    assert_eq!(jpeg_assets.len(), 1);
    assert_eq!(
        fs::read(&jpeg_assets[0]).unwrap(),
        YOUTUBE_THUMBNAIL_FIXTURE
    );
    let html = emitted_slide_html(&out);
    assert!(html.contains(YOUTUBE_PAGE_URL), "{html}");
    assert!(html.contains("Never Gonna Give You Up"), "{html}");
    assert!(html.contains(r#"src="assets/"#), "{html}");
    assert!(!html.contains(YOUTUBE_THUMBNAIL_URL), "{html}");
    assert!(!out.join(".peitho").exists());
}

#[test]
fn build_card_embed_from_cached_oembed_without_chrome() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    seed_oembed_cache(dir.path());
    write_embed_deck(&deck, &[(CARD_STATUS_URL, Some("mode=card"))]);

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
            (CARD_STATUS_URL, Some("mode=card")),
            (CARD_STATUS_URL, Some("mode=card")),
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
    write_embed_deck(&deck, &[(CARD_STATUS_URL, Some("mode=card"))]);
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
            (CARD_STATUS_URL, Some("mode=card")),
            (STATUS_URL, Some("mode=screenshot")),
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
