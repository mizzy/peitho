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
        deck_with_assets(
            "./title-body-code.html",
            "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```",
        ),
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
        deck_with_assets(
            "./title-body-code.html",
            "# Architecture\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```",
        ),
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
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 7"))
        .stderr(predicate::str::contains("slot 'code' got 2 item(s)"))
        .stderr(predicate::str::contains(
            "help: use a layout with more code capacity or remove one code block",
        ));
}

#[cfg(unix)]
#[test]
fn build_with_code_images_writes_svg_asset_and_references_it() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layout = dir.path().join("title-image.html");
    let command = dir.path().join("svg-command");
    let out = dir.path().join("dist");
    fs::write(
        &command,
        "#!/bin/sh\ncat >/dev/null\nprintf '<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>'\n",
    )
    .unwrap();
    fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        &deck,
        format!(
            "---\nlayouts: ./title-image.html\ncss: ./css\ncode_images:\n  mermaid: {}\n---\n# Diagram\n\n```mermaid\ngraph TD\n```",
            command.display()
        ),
    )
    .unwrap();
    fs::write(
        &layout,
        r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1><figure><slot name="image" accepts="image" arity="1"></slot></figure></section>"#,
    )
    .unwrap();
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

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

    let assets = asset_files(&out);
    assert_eq!(assets.len(), 1);
    assert_eq!(
        assets[0].extension().and_then(|ext| ext.to_str()),
        Some("svg")
    );
    assert_eq!(
        fs::read(&assets[0]).unwrap(),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"
    );
    let slide = fs::read_to_string(out.join("slides/000-diagram.html")).unwrap();
    let asset_name = assets[0].file_name().unwrap().to_string_lossy();
    assert!(slide.contains(&format!(r#"src="assets/{asset_name}""#)));
}

#[test]
fn build_with_missing_code_images_command_reports_code_block_line() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layout = dir.path().join("title-image.html");
    let missing = dir.path().join("missing-svg-command");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        format!(
            "---\nlayouts: ./title-image.html\ncss: ./css\ncode_images:\n  mermaid: {}\n---\n# Diagram\n\n```mermaid\ngraph TD\n```",
            missing.display()
        ),
    )
    .unwrap();
    fs::write(
        &layout,
        r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1><figure><slot name="image" accepts="image" arity="1"></slot></figure></section>"#,
    )
    .unwrap();
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 9"))
        .stderr(predicate::str::contains("code_images 'mermaid' failed"))
        .stderr(predicate::str::contains("missing-svg-"))
        .stderr(predicate::str::contains(
            "command': No such file or directory",
        ))
        .stderr(predicate::str::contains(
            "help: install the command or fix the code_images frontmatter",
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
        deck_with_assets(
            "./custom-layout.html",
            "# Architecture\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```",
        ),
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
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

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
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
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
    write_layout(dir.path());
    write_base_css(dir.path());
    write_overrides_css(
        dir.path(),
        r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
    );

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
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
    assert!(manifest.contains(r#""aspectRatio": "16:9""#));
    assert!(manifest.contains(r#""canvasWidth": 1280"#));
    assert!(manifest.contains(r#""canvasHeight": 720"#));
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
    assert!(index.contains("--peitho-canvas-width: 1280px;"));
    assert!(index.contains("--peitho-canvas-height: 720px;"));
    assert!(index.contains("width: 1280px; height: 720px;"));
    assert!(index.contains("const CANVAS_WIDTH = 1280"));
    assert!(index.contains("const CANVAS_HEIGHT = 720"));
    assert!(!index.contains("shell.js"));
    assert!(!index.contains("Peitho Architecture"));
    assert!(!index.contains("data-slide-key=\"arch-1\""));
}

#[test]
fn build_copies_markdown_image_to_dist_assets() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        deck_with_assets(
            "./title-image.html",
            "# Visual\n\n![Architecture](img/arch.png)",
        ),
    )
    .unwrap();
    write_test_png(&dir.path().join("img/arch.png"), TEST_PNG);
    write_image_layout(dir.path(), "1");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let assets = asset_files(&out);
    assert_eq!(assets.len(), 1);
    let asset_name = assets[0].file_name().unwrap().to_string_lossy();
    assert!(asset_name.ends_with("-arch.png"));
    let slide = fs::read_to_string(out.join("slides/000-visual.html")).unwrap();
    assert!(slide.contains(&format!(r#"<img src="assets/{asset_name}""#)));
}

#[test]
fn build_copies_nested_unicode_markdown_image_path() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        deck_with_assets(
            "./title-image.html",
            "# Visual\n\n![Diagram](./img/deep/画像.png)",
        ),
    )
    .unwrap();
    write_test_png(&dir.path().join("img/deep/画像.png"), TEST_PNG);
    write_image_layout(dir.path(), "1");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let assets = asset_files(&out);
    assert_eq!(assets.len(), 1);
    let asset_name = assets[0].file_name().unwrap().to_string_lossy();
    assert!(asset_name.ends_with("-画像.png"));
    let slide = fs::read_to_string(out.join("slides/000-visual.html")).unwrap();
    assert!(slide.contains(&format!(r#"<img src="assets/{asset_name}" alt="Diagram">"#)));
}

#[test]
fn build_fails_for_missing_markdown_image_with_line_and_help() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        deck_with_assets("./title-image.html", "# Visual\n\n![Missing](missing.png)"),
    )
    .unwrap();
    write_image_layout(dir.path(), "1");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slide 1 ('visual'), line 7"))
        .stderr(predicate::str::contains("image file not found:"))
        .stderr(predicate::str::contains("missing.png"))
        .stderr(predicate::str::contains(
            "help: place the image at the deck-relative path or fix the path",
        ));
}

#[cfg(unix)]
#[test]
fn build_fails_for_unreadable_markdown_image_with_line_and_help() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let image = dir.path().join("img/locked.png");
    fs::write(
        &deck,
        deck_with_assets(
            "./title-image.html",
            "# Visual\n\n![Locked](img/locked.png)",
        ),
    )
    .unwrap();
    write_test_png(&image, TEST_PNG);
    fs::set_permissions(&image, fs::Permissions::from_mode(0o000)).unwrap();
    write_image_layout(dir.path(), "1");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slide 1 ('visual'), line 7"))
        .stderr(predicate::str::contains("image file unreadable:"))
        .stderr(predicate::str::contains("img/locked.png"))
        .stderr(predicate::str::contains(
            "help: make the image file readable",
        ));
}

#[cfg(unix)]
#[test]
fn build_fails_for_symlinked_markdown_image_outside_deck_dir() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside_dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let outside = outside_dir.path().join("arch.png");
    let link = dir.path().join("img/link.png");
    fs::write(
        &deck,
        deck_with_assets("./title-image.html", "# Visual\n\n![Leaked](img/link.png)"),
    )
    .unwrap();
    write_test_png(&outside, TEST_PNG);
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    symlink(&outside, &link).unwrap();
    write_image_layout(dir.path(), "1");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slide 1 ('visual'), line 7"))
        .stderr(predicate::str::contains(
            "image path escapes deck directory",
        ))
        .stderr(predicate::str::contains("img/"))
        .stderr(predicate::str::contains("link.png"))
        .stderr(predicate::str::contains(
            "help: keep image files inside the deck directory",
        ));
}

#[test]
fn build_deduplicates_images_by_content_hash() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        deck_with_assets(
            "./title-image.html",
            "# Gallery\n\n![First](img/arch.png)\n\n![Second](img/copy.png)",
        ),
    )
    .unwrap();
    write_test_png(&dir.path().join("img/arch.png"), TEST_PNG);
    write_test_png(&dir.path().join("img/copy.png"), TEST_PNG);
    write_image_layout(dir.path(), "1..*");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let assets = asset_files(&out);
    assert_eq!(assets.len(), 1);
    let asset_name = assets[0].file_name().unwrap().to_string_lossy();
    assert!(asset_name.ends_with("-arch.png"));
    let slide = fs::read_to_string(out.join("slides/000-gallery.html")).unwrap();
    assert_eq!(slide.matches(&format!("assets/{asset_name}")).count(), 2);
}

#[test]
fn build_deduplicates_same_image_across_slides_by_content_hash() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        deck_with_assets(
            "./title-image.html",
            "# First\n\n![Diagram](img/arch.png)\n\n---\n# Second\n\n![Diagram](img/arch.png)",
        ),
    )
    .unwrap();
    write_test_png(&dir.path().join("img/arch.png"), TEST_PNG);
    write_image_layout(dir.path(), "1");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let assets = asset_files(&out);
    assert_eq!(assets.len(), 1);
    let asset_name = assets[0].file_name().unwrap().to_string_lossy();
    for slide in ["slides/000-first.html", "slides/001-second.html"] {
        let html = fs::read_to_string(out.join(slide)).unwrap();
        assert!(html.contains(&format!("assets/{asset_name}")));
    }
}

#[test]
fn build_replaces_stale_image_assets_when_image_content_changes() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let image = dir.path().join("img/arch.png");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        deck_with_assets("./title-image.html", "# Visual\n\n![Diagram](img/arch.png)"),
    )
    .unwrap();
    write_test_png(&image, TEST_PNG);
    write_image_layout(dir.path(), "1");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let old_asset = asset_files(&out)[0].file_name().unwrap().to_owned();

    let mut changed_png = TEST_PNG.to_vec();
    let last = changed_png.last_mut().unwrap();
    *last ^= 0x01;
    write_test_png(&image, &changed_png);
    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let assets = asset_files(&out);
    assert_eq!(assets.len(), 1);
    let new_asset = assets[0].file_name().unwrap();
    assert_ne!(new_asset, old_asset.as_os_str());
    assert!(!out.join("assets").join(&old_asset).exists());
    let slide = fs::read_to_string(out.join("slides/000-visual.html")).unwrap();
    assert!(slide.contains(&format!("assets/{}", new_asset.to_string_lossy())));
}

#[test]
fn build_keeps_same_basename_images_distinct_when_content_differs() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        deck_with_assets(
            "./title-image.html",
            "# Gallery\n\n![First](img/a/arch.png)\n\n![Second](img/b/arch.png)",
        ),
    )
    .unwrap();
    let mut other_png = TEST_PNG.to_vec();
    let last = other_png.last_mut().unwrap();
    *last ^= 0x01;
    write_test_png(&dir.path().join("img/a/arch.png"), TEST_PNG);
    write_test_png(&dir.path().join("img/b/arch.png"), &other_png);
    write_image_layout(dir.path(), "1..*");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let assets = asset_files(&out);
    assert_eq!(assets.len(), 2);
    let asset_names = assets
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_ne!(asset_names[0], asset_names[1]);
    assert!(asset_names.iter().all(|name| name.ends_with("-arch.png")));
    let slide = fs::read_to_string(out.join("slides/000-gallery.html")).unwrap();
    for asset_name in asset_names {
        assert!(slide.contains(&format!("assets/{asset_name}")));
    }
}

#[test]
fn build_rejects_two_image_paragraphs_when_image_slot_allows_one() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        deck_with_assets(
            "./title-image.html",
            "# Gallery\n\n![First](a.png)\n\n![Second](b.png)",
        ),
    )
    .unwrap();
    write_image_layout(dir.path(), "1");
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slide 1 ('gallery'), line 7"))
        .stderr(predicate::str::contains("slot 'hero' got 2 item(s)"))
        .stderr(predicate::str::contains("allows 1"))
        .stderr(predicate::str::contains(
            "help: use a layout with more hero capacity or remove one hero block",
        ));
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
    fs::write(
        &deck,
        deck_with_assets("./title-body-code.html", "# Intro\n\n---\n# Intro"),
    )
    .unwrap();
    write_layout(dir.path());
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");
    let out = dir.path().join("dist");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slide 2 ('intro'), line 8"))
        .stderr(predicate::str::contains("duplicate slide key 'intro'"))
        .stderr(predicate::str::contains(
            "help: add an explicit unique key comment before this slide",
        ));
}

#[test]
fn repository_example_builds_three_slide_distribution() {
    let dir = tempdir().unwrap();
    let deck = write_repository_example_deck_with_assets(dir.path());
    let out = dir.path().join("dist");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 3 slide(s)"));

    assert!(out.join("slides/000-arch-1.html").exists());
    assert!(out.join("slides/001-convention-mapping.html").exists());
    assert!(out.join("slides/002-dist-1.html").exists());
    assert!(fs::read_to_string(out.join("manifest.json"))
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
fn two_column_example_preserves_blocks_slot_paragraph_after_heading() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/two-column/deck.md",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 3 slide(s)"));

    let html = fs::read_to_string(
        out.path()
            .join("slides/001-compare-convention-vs-explicit.html"),
    )
    .unwrap();
    assert!(html.contains("<h2>Convention mapping</h2>"));
    assert!(html.contains(
        "Headings go to <code>title</code>, code goes to <code>code</code>, \
         everything else goes to <code>body</code>."
    ));
    assert!(html.contains("<h2>Explicit selection</h2>"));
    assert!(html.contains(
        "Use it when a layout has multiple <code>blocks</code> slots and \
         convention cannot resolve the ambiguity."
    ));
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
fn peitho_tour_example_exercises_dispatch_and_agenda_sections() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/peitho-tour/deck.md",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 20 slide(s)"));

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["plannedDurationMs"].as_u64(), Some(1_200_000));
    let sections = manifest["sections"].as_array().unwrap();
    let expected = [
        ("Intro", 0, 3, 180_000),
        ("Install", 4, 4, 60_000),
        ("Write", 5, 10, 360_000),
        ("Design", 11, 14, 300_000),
        ("Run", 15, 18, 240_000),
        ("Close", 19, 19, 60_000),
    ];
    assert_eq!(sections.len(), expected.len());
    for (section, (name, start, end, planned)) in sections.iter().zip(expected) {
        assert_eq!(section["name"].as_str(), Some(name));
        assert_eq!(section["startIndex"].as_u64(), Some(start));
        assert_eq!(section["endIndex"].as_u64(), Some(end));
        assert_eq!(section["plannedDurationMs"].as_u64(), Some(planned));
    }

    // The preview slide holds only text + a single image; type-driven dispatch
    // must land it on the `shot` layout (with the image slot) instead of
    // `topic`, without any explicit `layout` pin.
    let preview = fs::read_to_string(out.path().join("slides/015-preview.html")).unwrap();
    assert!(preview.contains("guide-shot"));
}

#[test]
fn peitho_tour_or_new_image_example_builds() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/image-showcase/deck.md",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 1 slide(s)"));

    let assets = asset_files(out.path());
    assert_eq!(assets.len(), 1);
    let asset_name = assets[0].file_name().unwrap().to_string_lossy();
    assert!(asset_name.ends_with("-arch.png"));
    let slide = fs::read_to_string(out.path().join("slides/000-image-showcase.html")).unwrap();
    assert!(slide.contains(&format!(r#"<img src="assets/{asset_name}""#)));

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["images"].as_array().unwrap().len(), 1);
    let expected_src = format!("assets/{asset_name}");
    assert_eq!(
        manifest["images"][0]["src"].as_str(),
        Some(expected_src.as_str())
    );
}

#[test]
fn aspect_ratio_4_3_example_builds_with_960_canvas() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "build",
            "examples/aspect-ratio-4-3/deck.md",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 2 slide(s)"));

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["aspectRatio"].as_str(), Some("4:3"));
    assert_eq!(manifest["canvasWidth"].as_u64(), Some(960));
    assert_eq!(manifest["canvasHeight"].as_u64(), Some(720));

    let index = fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(index.contains("--peitho-canvas-width: 960px;"));
    assert!(index.contains("--peitho-canvas-height: 720px;"));
    assert!(index.contains("width: 960px; height: 720px;"));
    assert!(index.contains("const CANVAS_WIDTH = 960"));
    assert!(index.contains("const CANVAS_HEIGHT = 720"));
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
    write_layout(dir.path());
    write_base_css(dir.path());
    write_overrides_css(dir.path(), "");
    let out = dir.path().join("dist");

    write_multi_slide_fixture(&deck);
    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(slide_fragment_count(&out), 3);

    fs::write(&deck, deck_with_assets("./title-body-code.html", "# Solo")).unwrap();
    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(slide_fragment_count(&out), 1);
    assert!(out.join("slides/000-solo.html").exists());
}

#[test]
fn base_theme_reads_canvas_dimensions_from_css_variables_with_16_9_fallback() {
    let css = fs::read_to_string(workspace_root().join("themes/base.css")).unwrap();

    assert!(css.contains("width: var(--peitho-canvas-width, 1280px);"));
    assert!(css.contains("height: var(--peitho-canvas-height, 720px);"));
    assert!(css.contains("font-size: 56px;"));
    assert!(!css.contains("min-height: 100vh"));
    assert!(!css.contains("font-size: 1.4rem"));
}

#[test]
fn build_reads_layouts_from_frontmatter() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        "---\nlayouts: ./custom-layouts\n---\n# Frontmatter Layout\n\nBody",
    )
    .unwrap();
    write_layout_dir(dir.path(), "custom-layouts", "frontmatter-layout");

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

    let html = fs::read_to_string(out.join("slides/000-frontmatter-layout.html")).unwrap();
    assert!(html.contains(r#"class="frontmatter-layout peitho-slide""#));
}

#[test]
fn build_reads_layouts_from_absolute_path_in_frontmatter() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    let layouts = write_layout_dir(dir.path(), "absolute-layouts", "absolute-layout");
    fs::write(
        &deck,
        format!("---\nlayouts: {}\n---\n# Intro\n", layouts.display()),
    )
    .unwrap();

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

    let html = fs::read_to_string(out.join("slides/000-intro.html")).unwrap();
    assert!(html.contains(r#"class="absolute-layout peitho-slide""#));
}

#[test]
fn build_reads_css_from_frontmatter() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        "---\ncss: ./custom-css\n---\n# Frontmatter CSS\n\nBody",
    )
    .unwrap();
    let css_dir = dir.path().join("custom-css");
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(
        css_dir.join("theme.css"),
        ".slot-title { color: rebeccapurple; }",
    )
    .unwrap();

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

    assert!(fs::read_to_string(out.join("peitho.css"))
        .unwrap()
        .contains(".slot-title { color: rebeccapurple; }"));
}

#[test]
fn build_reads_syntaxes_from_frontmatter() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        "---\nsyntaxes: ./custom-syntaxes\n---\n# Custom Syntax\n\n```crn\nresource \"site\" {}\n```",
    )
    .unwrap();
    let syntaxes_dir = dir.path().join("custom-syntaxes");
    fs::create_dir_all(&syntaxes_dir).unwrap();
    fs::write(
        syntaxes_dir.join("carina.sublime-syntax"),
        CARINA_SUBLIME_SYNTAX,
    )
    .unwrap();

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

    let html = fs::read_to_string(out.join("slides/000-custom-syntax.html")).unwrap();
    assert!(html.contains("hl-keyword"));
}

#[test]
fn build_reads_syntaxes_file_from_frontmatter() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        "---\nsyntaxes: ./carina.sublime-syntax\n---\n# Custom Syntax\n\n```crn\nresource \"site\" {}\n```",
    )
    .unwrap();
    fs::write(
        dir.path().join("carina.sublime-syntax"),
        CARINA_SUBLIME_SYNTAX,
    )
    .unwrap();

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

    let html = fs::read_to_string(out.join("slides/000-custom-syntax.html")).unwrap();
    assert!(html.contains("hl-keyword"));
}

#[test]
fn build_frontmatter_layouts_overrides_deck_adjacent() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        "---\nlayouts: ./other-layouts\n---\n# Override Layout\n\nBody",
    )
    .unwrap();
    write_layout_dir(dir.path(), "layouts", "deck-adjacent-layout");
    write_layout_dir(dir.path(), "other-layouts", "frontmatter-layout");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let html = fs::read_to_string(out.join("slides/000-override-layout.html")).unwrap();
    assert!(html.contains(r#"class="frontmatter-layout peitho-slide""#));
    assert!(!html.contains("deck-adjacent-layout"));
}

#[test]
fn build_frontmatter_non_existent_path_errors_with_line_and_help() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    fs::write(
        &deck,
        "---\nlayouts: ./nope-does-not-exist\n---\n# Missing Layouts",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("layouts path does not exist"))
        .stderr(predicate::str::contains("line 2"))
        .stderr(predicate::str::contains(
            "check the layouts: value in the frontmatter",
        ));
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
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

fn write_multi_slide_fixture(path: &Path) {
    fs::write(
        path,
        deck_with_assets(
            "./title-body-code.html",
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
        ),
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

fn write_image_layout(dir: &Path, image_arity: &str) -> PathBuf {
    let path = dir.join("title-image.html");
    fs::write(
        &path,
        format!(
            r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1><figure><slot name="hero" accepts="image" arity="{image_arity}"></slot></figure></section>"#
        ),
    )
    .unwrap();
    path
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

fn write_test_png(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn asset_files(out: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(out.join("assets"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    files.sort();
    files
}

const TEST_PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H', b'D', b'R',
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, b'I', b'D', b'A', b'T', 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xae,
    0x42, 0x60, 0x82,
];

const CARINA_SUBLIME_SYNTAX: &str = r#"%YAML 1.2
---
name: Carina
file_extensions: [crn]
scope: source.carina
contexts:
  main:
    - match: '\b(resource|provider|module)\b'
      scope: keyword.control.carina
"#;

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
    write_layout(dir.path());
    write_base_css(dir.path());
    write_overrides_css(dir.path(), override_css);
    let out = dir.path().join("dist");

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    (dir, out)
}
