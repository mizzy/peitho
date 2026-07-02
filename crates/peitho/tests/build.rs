use std::{fs, path::PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn build_writes_index_html_and_css() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let template = dir.path().join("title-body-code.html");
    let base = dir.path().join("base.css");
    let overrides = dir.path().join("overrides.css");
    let out = dir.path().join("dist");

    fs::write(
        &deck,
        "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```",
    )
    .unwrap();
    fs::write(
        &template,
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
            "--template",
            template.to_str().unwrap(),
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

    assert!(fs::read_to_string(out.join("index.html"))
        .unwrap()
        .contains(r#"data-slide-key="arch-1""#));
    assert!(fs::read_to_string(out.join("peitho.css"))
        .unwrap()
        .contains(r#"[data-slide-key="arch-1"] .slot-code"#));
}

#[test]
fn build_fails_with_line_and_help_for_contract_violation() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let template = dir.path().join("title-body-code.html");
    let base = dir.path().join("base.css");
    let overrides = dir.path().join("overrides.css");
    let out = dir.path().join("dist");

    fs::write(
        &deck,
        "# Architecture\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```",
    )
    .unwrap();
    fs::write(
        &template,
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
            "--template",
            template.to_str().unwrap(),
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
fn contract_error_uses_template_file_stem_as_layout_name() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let template = dir.path().join("custom-layout.html");
    let base = dir.path().join("base.css");
    let overrides = dir.path().join("overrides.css");
    let out = dir.path().join("dist");

    fs::write(
        &deck,
        "# Architecture\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```",
    )
    .unwrap();
    fs::write(
        &template,
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
            "--template",
            template.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("layout 'custom-layout' allows"));
}

#[test]
fn repository_example_builds() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/deck.md",
            "--template",
            "templates/title-body-code.html",
            "--base-css",
            "themes/base.css",
            "--overrides-css",
            "themes/overrides.css",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let html = fs::read_to_string(out.path().join("index.html")).unwrap();
    let css = fs::read_to_string(out.path().join("peitho.css")).unwrap();
    assert!(html.contains(r#"data-slide-key="arch-1""#));
    assert!(html.contains(r#"class="slot-code""#));
    assert!(css.contains(r#"[data-slide-key="arch-1"] .slot-code"#));
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
