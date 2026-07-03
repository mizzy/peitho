use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::{tempdir, TempDir};

#[test]
fn build_writes_index_html_and_css() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layout = dir.path().join("title-body-code.html");
    let base = dir.path().join("base.css");
    let overrides = dir.path().join("overrides.css");
    let out = dir.path().join("dist");

    fs::write(
        &deck,
        "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```",
    )
    .unwrap();
    fs::write(
        &layout,
        r#"<section class="slide"><h1><slot name="title" accepts="inline" arity="1"></slot></h1><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#,
    )
    .unwrap();
    fs::write(&base, ".slot-title { font-weight: 700; }").unwrap();
    fs::write(
        &overrides,
        r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layout",
            layout.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 1 slide"));

    let index = fs::read_to_string(out.join("index.html")).unwrap();
    let first = fs::read_to_string(out.join("slides/000-arch-1.html")).unwrap();
    assert!(index.contains("fetchOk('manifest.json')"));
    assert!(!index.contains(r#"data-slide-key="arch-1""#));
    assert!(first.contains(r#"data-slide-key="arch-1""#));
    assert!(fs::read_to_string(out.join("peitho.css"))
        .unwrap()
        .contains(r#"[data-slide-key="arch-1"] .slot-code"#));
}

#[test]
fn build_fails_with_line_and_help_for_contract_violation() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layout = dir.path().join("title-body-code.html");
    let base = dir.path().join("base.css");
    let overrides = dir.path().join("overrides.css");
    let out = dir.path().join("dist");

    fs::write(
        &deck,
        "# Architecture\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```",
    )
    .unwrap();
    fs::write(
        &layout,
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#,
    )
    .unwrap();
    fs::write(&base, "").unwrap();
    fs::write(&overrides, "").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layout",
            layout.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 3"))
        .stderr(predicate::str::contains("slot 'code' got 2 item(s)"))
        .stderr(predicate::str::contains(
            "help: use a layout with more code capacity or remove one code block",
        ));
}

#[test]
fn contract_error_uses_layout_file_stem_as_layout_name() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layout = dir.path().join("custom-layout.html");
    let base = dir.path().join("base.css");
    let overrides = dir.path().join("overrides.css");
    let out = dir.path().join("dist");

    fs::write(
        &deck,
        "# Architecture\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```",
    )
    .unwrap();
    fs::write(
        &layout,
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#,
    )
    .unwrap();
    fs::write(&base, "").unwrap();
    fs::write(&overrides, "").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layout",
            layout.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("layout"))
        .stderr(predicate::str::contains("custom-layout"))
        .stderr(predicate::str::contains("allows 0..1"));
}

#[test]
fn build_writes_slide_fragments_in_slides_directory() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    write_multi_slide_fixture(&deck);
    let layout = write_layout(dir.path());
    let base = write_base_css(dir.path());
    let overrides = write_overrides_css(
        dir.path(),
        r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
    );

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layout",
            layout.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out.join("slides/000-arch-1.html").exists());
    assert!(out.join("slides/001-convention-mapping.html").exists());
    assert!(out.join("slides/002-dist-1.html").exists());
    assert!(fs::read_to_string(out.join("slides/000-arch-1.html"))
        .unwrap()
        .contains(r#"data-slide-key="arch-1""#));
}

#[test]
fn build_writes_manifest_json_with_refs_not_html() {
    let (_dir, out) = build_multi_slide_fixture();
    let manifest = fs::read_to_string(out.join("manifest.json")).unwrap();

    assert!(manifest.contains(r#""version": 1"#));
    assert!(manifest.contains(&format!(
        r#""peithoVersion": "{}""#,
        env!("CARGO_PKG_VERSION")
    )));
    assert!(manifest.contains(r#""title": "Peitho Architecture""#));
    assert!(manifest.contains(r#""slideCount": 3"#));
    assert!(manifest.contains(r#""src": "slides/000-arch-1.html""#));
    assert!(manifest.contains(r#""hasNotes": false"#));
    assert!(!manifest.contains("<section"));
}

#[test]
fn build_still_writes_distribution_after_pipeline_refactor() {
    let (_dir, out) = build_multi_slide_fixture();

    assert!(out.join("index.html").exists());
    assert!(out.join("manifest.json").exists());
    assert!(out.join("peitho.css").exists());
    assert!(out.join("slides/000-arch-1.html").exists());
    assert!(!out.join("present.html").exists());
    assert!(!out.join("notes.json").exists());
}

#[test]
fn build_writes_fetching_index_without_embedded_slide_html() {
    let (_dir, out) = build_multi_slide_fixture();
    let index = fs::read_to_string(out.join("index.html")).unwrap();

    assert!(index.contains("fetchOk('manifest.json')"));
    assert!(index.contains("fetchOk(slide.src)"));
    assert!(index.contains(r#"<main id="peitho-slides">"#));
    assert!(index.contains(r#"<div id="peitho-canvas"></div>"#));
    assert!(index.contains("const CANVAS_WIDTH = 1280"));
    assert!(!index.contains("shell.js"));
    assert!(!index.contains("Peitho Architecture"));
    assert!(!index.contains("data-slide-key=\"arch-1\""));
}

#[test]
fn build_accepts_override_targeting_derived_second_slide_key() {
    let (_dir, out) = build_multi_slide_fixture_with_override(
        r#"[data-slide-key="convention-mapping"] .slot-body { color: red; }"#,
    );

    let css = fs::read_to_string(out.join("peitho.css")).unwrap();
    assert!(css.contains(r#"[data-slide-key="convention-mapping"] .slot-body"#));
}

#[test]
fn build_fails_on_duplicate_slide_keys_with_line_help_and_slide_context() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "# Intro\n\n---\n# Intro").unwrap();
    let layout = write_layout(dir.path());
    let base = write_base_css(dir.path());
    let overrides = write_overrides_css(dir.path(), "");
    let out = dir.path().join("dist");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layout",
            layout.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slide 2 ('intro'), line 4"))
        .stderr(predicate::str::contains("duplicate slide key 'intro'"))
        .stderr(predicate::str::contains(
            "help: add an explicit unique key comment before this slide",
        ));
}

#[test]
fn repository_example_builds_three_slide_distribution() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/deck.md",
            "--layout",
            "layouts/title-body-code.html",
            "--base-css",
            "themes/base.css",
            "--overrides-css",
            "themes/overrides.css",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 3 slide(s)"));

    assert!(out.path().join("slides/000-arch-1.html").exists());
    assert!(out
        .path()
        .join("slides/001-convention-mapping.html")
        .exists());
    assert!(out.path().join("slides/002-dist-1.html").exists());
    assert!(fs::read_to_string(out.path().join("manifest.json"))
        .unwrap()
        .contains(r#""slideCount": 3"#));
}

#[test]
fn build_keeps_slide_html_only_in_fragment_files() {
    let (_dir, out) = build_multi_slide_fixture();
    let index = fs::read_to_string(out.join("index.html")).unwrap();
    let first = fs::read_to_string(out.join("slides/000-arch-1.html")).unwrap();

    assert!(!index.contains("data-slide-key"));
    assert!(first.contains(r#"data-slide-key="arch-1""#));
}

#[test]
fn build_clears_stale_slide_fragments_before_writing_new_ones() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layout = write_layout(dir.path());
    let base = write_base_css(dir.path());
    let overrides = write_overrides_css(dir.path(), "");
    let out = dir.path().join("dist");

    write_multi_slide_fixture(&deck);
    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layout",
            layout.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(slide_fragment_count(&out), 3);

    fs::write(&deck, "# Solo").unwrap();
    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layout",
            layout.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(slide_fragment_count(&out), 1);
    assert!(out.join("slides/000-solo.html").exists());
}

#[test]
fn base_theme_targets_fixed_canvas_size() {
    let css = fs::read_to_string(workspace_root().join("themes/base.css")).unwrap();

    assert!(css.contains("width: 1280px;"));
    assert!(css.contains("height: 720px;"));
    assert!(css.contains("font-size: 56px;"));
    assert!(!css.contains("min-height: 100vh"));
    assert!(!css.contains("font-size: 1.4rem"));
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}

fn write_multi_slide_fixture(path: &Path) {
    fs::write(
        path,
        r#"<!-- {"key":"arch-1"} -->
# Peitho Architecture

Body

```rust
fn main() {}
```

---

# Convention Mapping

- title
- body

---
<!-- {"key":"dist-1"} -->
# Distribution

Fragments and manifest.
"#,
    )
    .unwrap();
}

fn write_layout(dir: &Path) -> PathBuf {
    let path = dir.join("title-body-code.html");
    fs::write(
        &path,
        r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#,
    )
    .unwrap();
    path
}

fn write_base_css(dir: &Path) -> PathBuf {
    let path = dir.join("base.css");
    fs::write(&path, ".slot-title { font-weight: 700; }").unwrap();
    path
}

fn write_overrides_css(dir: &Path, css: &str) -> PathBuf {
    let path = dir.join("overrides.css");
    fs::write(&path, css).unwrap();
    path
}

fn slide_fragment_count(out: &Path) -> usize {
    fs::read_dir(out.join("slides")).unwrap().count()
}

fn build_multi_slide_fixture() -> (TempDir, PathBuf) {
    build_multi_slide_fixture_with_override(
        r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
    )
}

fn build_multi_slide_fixture_with_override(override_css: &str) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_multi_slide_fixture(&deck);
    let layout = write_layout(dir.path());
    let base = write_base_css(dir.path());
    let overrides = write_overrides_css(dir.path(), override_css);
    let out = dir.path().join("dist");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layout",
            layout.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    (dir, out)
}
