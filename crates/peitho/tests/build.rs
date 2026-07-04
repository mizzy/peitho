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
    let css_dir = dir.path().join("css");
    fs::create_dir_all(&css_dir).unwrap();
    let base = css_dir.join("base.css");
    let overrides = css_dir.join("overrides.css");
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
            "--layouts",
            layout.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
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
    let css_dir = dir.path().join("css");
    fs::create_dir_all(&css_dir).unwrap();
    let base = css_dir.join("base.css");
    let overrides = css_dir.join("overrides.css");
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
            "--layouts",
            layout.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
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
    let css_dir = dir.path().join("css");
    fs::create_dir_all(&css_dir).unwrap();
    let base = css_dir.join("base.css");
    let overrides = css_dir.join("overrides.css");
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
            "--layouts",
            layout.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
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
fn build_dispatches_slides_across_multiple_layouts() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        "<!-- {\"key\":\"cover\"} -->\n# Cover Only\n\n---\n<!-- {\"key\":\"body\"} -->\n# Statement\n\nBody paragraph\n",
    )
    .unwrap();
    let layouts_dir = dir.path().join("layouts");
    fs::create_dir_all(&layouts_dir).unwrap();
    fs::write(
        layouts_dir.join("cover.html"),
        r#"<section class="cover"><h1><slot name="title" accepts="inline" arity="1"></slot></h1></section>"#,
    )
    .unwrap();
    fs::write(
        layouts_dir.join("statement.html"),
        r#"<section class="statement"><h1><slot name="title" accepts="inline" arity="1"></slot></h1><slot name="body" accepts="blocks" arity="1..*"></slot></section>"#,
    )
    .unwrap();
    let base = write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layouts",
            layouts_dir.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 2 slide(s)"));

    let cover_html = fs::read_to_string(out.join("slides/000-cover.html")).unwrap();
    let body_html = fs::read_to_string(out.join("slides/001-body.html")).unwrap();
    assert!(cover_html.contains(r#"class="cover peitho-slide""#));
    assert!(body_html.contains(r#"class="statement peitho-slide""#));
}

#[test]
fn build_zero_config_uses_layouts_and_css_dirs_next_to_the_deck() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(&deck, "# Zero Config\n\nBody\n").unwrap();
    let layouts_dir = dir.path().join("layouts");
    fs::create_dir_all(&layouts_dir).unwrap();
    fs::write(
        layouts_dir.join("statement.html"),
        r#"<section class="zero-config"><h1><slot name="title" accepts="inline" arity="1"></slot></h1><slot name="body" accepts="blocks" arity="0..*"></slot></section>"#,
    )
    .unwrap();
    let css_dir = dir.path().join("css");
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(css_dir.join("base.css"), ".zero-config { color: teal; }").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 1 slide"));

    let html = fs::read_to_string(out.join("slides/000-zero-config.html")).unwrap();
    assert!(html.contains(r#"class="zero-config peitho-slide""#));
    assert!(fs::read_to_string(out.join("peitho.css"))
        .unwrap()
        .contains(".zero-config { color: teal; }"));
}

#[test]
fn build_rejects_ambiguous_slide_with_disambiguation_help() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "# Title Only\n").unwrap();
    let layouts_dir = dir.path().join("layouts");
    fs::create_dir_all(&layouts_dir).unwrap();
    for name in ["cover-a.html", "cover-b.html"] {
        fs::write(
            layouts_dir.join(name),
            r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1></section>"#,
        )
        .unwrap();
    }
    let base = write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layouts",
            layouts_dir.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
            "--out",
            dir.path().join("dist").to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slide matches multiple layouts"))
        .stderr(predicate::str::contains("cover-a"))
        .stderr(predicate::str::contains("cover-b"))
        .stderr(predicate::str::contains(r#"{"layout":"…"}"#));
}

#[test]
fn build_writes_slide_fragments_in_slides_directory() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    write_multi_slide_fixture(&deck);
    let layout = write_layout(dir.path());
    let base = write_base_css(dir.path());
    write_overrides_css(
        dir.path(),
        r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
    );

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layouts",
            layout.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
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
    write_overrides_css(dir.path(), "");
    let out = dir.path().join("dist");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layouts",
            layout.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
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
            "--layouts",
            "layouts/title-body-code.html",
            "--css",
            "themes/base.css",
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
fn lightning_talk_example_declares_five_minute_planned_duration() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/lightning-talk/deck.md",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 5 slide(s)"));

    assert!(fs::read_to_string(out.path().join("manifest.json"))
        .unwrap()
        .contains(r#""plannedDurationMs": 300000"#));
}

#[test]
fn lightning_talk_example_declares_agenda_sections() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/lightning-talk/deck.md",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 5 slide(s)"));

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["plannedDurationMs"].as_u64(), Some(300_000));
    let sections = manifest["sections"].as_array().unwrap();
    let expected = [
        ("Setup", 0, 0, 60_000),
        ("Problem", 1, 1, 60_000),
        ("Approach", 2, 3, 120_000),
        ("Wrap-up", 4, 4, 60_000),
    ];
    assert_eq!(sections.len(), expected.len());
    for (section, (name, start, end, planned)) in sections.iter().zip(expected) {
        assert_eq!(section["name"].as_str(), Some(name));
        assert_eq!(section["startIndex"].as_u64(), Some(start));
        assert_eq!(section["endIndex"].as_u64(), Some(end));
        assert_eq!(section["plannedDurationMs"].as_u64(), Some(planned));
    }
}

#[test]
fn feature_tour_example_exercises_dispatch_sections_and_list_slot() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/feature-tour/deck.md",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 7 slide(s)"));

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["plannedDurationMs"].as_u64(), Some(480_000));
    let sections = manifest["sections"].as_array().unwrap();
    let expected = [
        ("Basics", 0, 1, 120_000),
        ("Contracts", 2, 3, 180_000),
        ("Presenting", 4, 6, 180_000),
    ];
    assert_eq!(sections.len(), expected.len());
    for (section, (name, start, end, planned)) in sections.iter().zip(expected) {
        assert_eq!(section["name"].as_str(), Some(name));
        assert_eq!(section["startIndex"].as_u64(), Some(start));
        assert_eq!(section["endIndex"].as_u64(), Some(end));
        assert_eq!(section["plannedDurationMs"].as_u64(), Some(planned));
    }

    // The checks slide is list-only, which structurally matches both `topic`
    // and `agenda`; its explicit {"layout":"agenda"} request must win.
    let checks = fs::read_to_string(out.path().join("slides/002-checks.html")).unwrap();
    assert!(checks.contains("tour-agenda"));
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
    write_overrides_css(dir.path(), "");
    let out = dir.path().join("dist");

    write_multi_slide_fixture(&deck);
    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layouts",
            layout.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
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
            "--layouts",
            layout.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
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

/// Accepts either a css file (manual fixtures) or the css dir itself
/// (helper fixtures) and returns the directory to pass to --css.
fn css_dir_for(path: &Path) -> PathBuf {
    if path.extension().is_some() {
        path.parent().unwrap().to_path_buf()
    } else {
        path.to_path_buf()
    }
}

fn write_base_css(dir: &Path) -> PathBuf {
    let css_dir = dir.join("css");
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(
        css_dir.join("base.css"),
        ".slot-title { font-weight: 700; }",
    )
    .unwrap();
    css_dir
}

fn write_overrides_css(dir: &Path, css: &str) -> PathBuf {
    let css_dir = dir.join("css");
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(css_dir.join("overrides.css"), css).unwrap();
    css_dir
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
    write_overrides_css(dir.path(), override_css);
    let out = dir.path().join("dist");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--layouts",
            layout.to_str().unwrap(),
            "--css",
            css_dir_for(&base).to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    (dir, out)
}
