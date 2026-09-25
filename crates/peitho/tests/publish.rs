use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn write_valid_dist(root: &Path) {
    fs::create_dir_all(root.join("slides")).unwrap();
    fs::write(
        root.join("index.html"),
        r#"<!doctype html><main id="peitho-slides"></main>"#,
    )
    .unwrap();
    fs::write(
        root.join("peitho.css"),
        ".slot-title { font-weight: 700; }\n",
    )
    .unwrap();
    fs::write(
        root.join("manifest.json"),
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("slides/000-arch-1.html"),
        r#"<section data-slide-key="arch-1"></section>"#,
    )
    .unwrap();
}

#[test]
fn publish_rejects_missing_distribution() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("distribution is incomplete"))
        .stderr(predicate::str::contains("missing index.html"))
        .stderr(predicate::str::contains("help: run `peitho build` first"));
}

#[test]
fn publish_rejects_distribution_without_slide_fragments() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    fs::create_dir_all(dist.join("slides")).unwrap();
    fs::write(dist.join("index.html"), "").unwrap();
    fs::write(dist.join("manifest.json"), "").unwrap();
    fs::write(dist.join("peitho.css"), "").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("distribution is incomplete"))
        .stderr(predicate::str::contains(
            "slides/ must contain at least one file",
        ))
        .stderr(predicate::str::contains("help: run `peitho build` first"));
}

#[test]
fn publish_rejects_presentation_only_files() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(dist.join("presenter.html"), "<!doctype html>").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "distribution contains presentation-only file: presenter.html",
        ))
        .stderr(predicate::str::contains(
            "help: remove presentation artifacts or run `peitho build` again",
        ));
}

#[test]
fn publish_rejects_remote_presentation_only_file() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(dist.join("remote.js"), "export {}").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "distribution contains presentation-only file: remote.js",
        ));
}

#[test]
fn publish_rejects_slide_sources_file() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(
        dist.join("sources.json"),
        r#"{"version":1,"sources":{},"unavailable":{}}"#,
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "distribution contains presentation-only file: sources.json",
        ));
}

#[test]
fn publish_rejects_font_scope_css() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(dist.join("fontscope.css"), "").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "distribution contains presentation-only file: fontscope.css",
        ));
}

#[test]
fn publish_rejects_preview_edit_source_span_attribute() {
    assert_publish_rejects_preview_edit_annotation("data-peitho-src", "1-4");
}

#[test]
fn publish_rejects_preview_edit_markdown_attribute() {
    assert_publish_rejects_preview_edit_annotation("data-peitho-md", "raw");
}

fn assert_publish_rejects_preview_edit_annotation(attribute: &str, value: &str) {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    let slide = "slides/000-arch-1.html";
    write_valid_dist(&dist);
    fs::write(
        dist.join(slide),
        format!(r#"<section data-slide-key="arch-1"><p {attribute}="{value}">body</p></section>"#),
    )
    .unwrap();

    assert_publish_rejects_annotation_at(&dist, attribute, slide);
}

fn assert_publish_rejects_annotation_at(dist: &Path, attribute: &str, relative: &str) {
    let probe = dist.parent().unwrap().join("publish-command-ran");

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(dist)
        .args(["--", "sh", "-c", "printf invoked > \"$1\"", "peitho-test"])
        .arg(&probe)
        .assert()
        .failure()
        .stderr(predicate::str::contains(attribute))
        .stderr(predicate::str::contains(relative));

    assert!(!probe.exists(), "publish command ran despite contamination");
}

#[cfg(unix)]
#[test]
fn publish_rejects_preview_edit_attribute_in_symlinked_html_file() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(
        dir.path().join("outside.html"),
        r#"<p data-peitho-src="1-4">body</p>"#,
    )
    .unwrap();
    symlink("../outside.html", dist.join("extra.html")).unwrap();

    assert_publish_rejects_annotation_at(&dist, "data-peitho-src", "extra.html");
}

#[cfg(unix)]
#[test]
fn publish_rejects_broken_html_symlink_and_names_the_relative_entry() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    let probe = dir.path().join("publish-command-ran");
    write_valid_dist(&dist);
    symlink("../missing.html", dist.join("broken.html")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "sh", "-c", "printf invoked > \"$1\"", "peitho-test"])
        .arg(&probe)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "failed to inspect distribution entry broken.html",
        ))
        .stderr(predicate::str::contains(
            "help: remove the unreadable entry or run `peitho build` again",
        ));

    assert!(
        !probe.exists(),
        "publish command ran despite unreadable entry"
    );
}

#[cfg(unix)]
#[test]
fn publish_rejects_preview_edit_attribute_in_symlinked_directory() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    let outside = dir.path().join("outdir");
    write_valid_dist(&dist);
    fs::create_dir(&outside).unwrap();
    fs::write(
        outside.join("annotated.html"),
        r#"<p data-peitho-md="raw">body</p>"#,
    )
    .unwrap();
    symlink(".", outside.join("a-loop")).unwrap();
    symlink("../outdir", dist.join("more")).unwrap();

    assert_publish_rejects_annotation_at(&dist, "data-peitho-md", "more/annotated.html");
}

#[test]
fn publish_rejects_preview_edit_attribute_in_ascii_case_insensitive_html_extension() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(dist.join("x.HTML"), r#"<p data-peitho-src="1-4">body</p>"#).unwrap();

    assert_publish_rejects_annotation_at(&dist, "data-peitho-src", "x.HTML");
}

#[test]
fn publish_accepts_slide_text_mentioning_preview_edit_attribute_names() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(
        dist.join("slides/000-arch-1.html"),
        concat!(
            r#"<section data-slide-key="arch-1">"#,
            "<p>Attributes data-peitho-src and data-peitho-md are preview-only.</p>",
            "<pre><code>&lt;p data-peitho-src=&quot;1-4&quot; ",
            "data-peitho-md=&quot;raw&quot;&gt;&lt;/p&gt;</code></pre>",
            "</section>"
        ),
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .success();
}

#[test]
fn publish_rejects_non_utf8_html_and_names_the_relative_file() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    let slide = "slides/000-arch-1.html";
    write_valid_dist(&dist);
    fs::write(dist.join(slide), b"<section>\xff</section>").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "failed to read distribution HTML as UTF-8",
        ))
        .stderr(predicate::str::contains(slide));
}

#[test]
fn publish_rejects_unparseable_html_with_rebuild_help_and_relative_file() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    let slide = "slides/000-arch-1.html";
    write_valid_dist(&dist);
    fs::write(dist.join(slide), "<select><xmp><script>").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("could not be parsed as HTML"))
        .stderr(predicate::str::contains(slide))
        .stderr(predicate::str::contains("help: run `peitho build` again"));
}

#[test]
fn publish_accepts_math_build_distribution_with_katex_fonts() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let dist = dir.path().join("dist");
    fs::write(&deck, "# Math\n\n```math\n\\frac{1}{2}\n```\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--out",
            dist.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(dist.join("katex-fonts/KaTeX_Main-Regular.woff2").is_file());
    assert!(!dist.join("notes.json").exists());
    assert!(!dist.join("present.html").exists());
    assert!(!dist.join("presenter.html").exists());
    assert!(!dist.join("preview.js").exists());
    assert!(!dist.join("shell.js").exists());

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .success();
}

#[test]
fn publish_rejects_missing_manifest_slide_reference() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::remove_file(dist.join("slides/000-arch-1.html")).unwrap();
    fs::write(dist.join("slides/stale.html"), "<section></section>").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "manifest references missing slide fragment: slides/000-arch-1.html",
        ))
        .stderr(predicate::str::contains("help: run `peitho build` first"));
}

#[test]
fn publish_rejects_manifest_slide_reference_outside_dist() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(
        dist.join("manifest.json"),
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"../secret.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        ),
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "manifest contains invalid slide src: ../secret.html",
        ))
        .stderr(predicate::str::contains(
            "help: slide src must be a relative path inside dist/",
        ));
}

#[test]
fn publish_rejects_missing_manifest_asset_reference() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(
        dist.join("manifest.json"),
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ],\n",
            "  \"images\": [{\"src\":\"assets/nonexistent.png\"}]\n",
            "}\n"
        ),
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "manifest references missing asset: assets/nonexistent.png",
        ))
        .stderr(predicate::str::contains("help: run `peitho build` first"));
}

#[test]
fn publish_rejects_manifest_asset_reference_outside_dist() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(
        dist.join("manifest.json"),
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ],\n",
            "  \"images\": [{\"src\":\"../etc/passwd\"}]\n",
            "}\n"
        ),
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "manifest contains invalid asset src: ../etc/passwd",
        ))
        .stderr(predicate::str::contains(
            "help: asset src must be a relative path inside dist/",
        ));
}

#[cfg(unix)]
#[test]
fn publish_rejects_manifest_asset_reference_symlink_outside_dist() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    let outside = dir.path().join("outside/arch.png");
    write_valid_dist(&dist);
    fs::create_dir_all(dist.join("assets")).unwrap();
    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    fs::write(&outside, b"png").unwrap();
    symlink(&outside, dist.join("assets/leak.png")).unwrap();
    fs::write(
        dist.join("manifest.json"),
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 1,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ],\n",
            "  \"images\": [{\"src\":\"assets/leak.png\"}]\n",
            "}\n"
        ),
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "manifest contains invalid asset src: assets/leak.png",
        ))
        .stderr(predicate::str::contains(
            "help: asset src must be a relative path inside dist/",
        ));
}

#[test]
fn publish_rejects_manifest_slide_count_mismatch() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(
        dist.join("manifest.json"),
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Deck\",\n",
            "  \"slideCount\": 2,\n",
            "  \"aspectRatio\": \"16:9\",\n",
            "  \"canvasWidth\": 1280,\n",
            "  \"canvasHeight\": 720,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        ),
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "manifest slideCount does not match slides length",
        ))
        .stderr(predicate::str::contains("help: run `peitho build` first"));
}

#[test]
fn publish_requires_external_command() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .assert()
        .failure()
        .stderr(predicate::str::contains("publish command is missing"))
        .stderr(predicate::str::contains(
            "deployment is delegated to IaC or CI",
        ))
        .stderr(predicate::str::contains("peitho publish -- aws"));
}

#[test]
fn publish_runs_command_with_peitho_dist_env() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    let probe = dir.path().join("probe.txt");
    write_valid_dist(&dist);

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args([
            "--",
            "sh",
            "-c",
            "printf '%s' \"$PEITHO_DIST\" > \"$1\"",
            "peitho-test",
        ])
        .arg(&probe)
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&probe).unwrap(),
        fs::canonicalize(&dist).unwrap().display().to_string()
    );
}

#[test]
fn publish_propagates_command_exit_code() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "sh", "-c", "exit 23"])
        .assert()
        .code(23);
}

#[test]
fn repository_example_can_be_published_to_external_command() {
    let dir = tempdir().unwrap();
    let deck = write_repository_example_deck_with_assets(dir.path());
    let out = dir.path().join("dist");
    let probe = dir.path().join("published.txt");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args(["build", deck.to_str().unwrap(), "--out"])
        .arg(&out)
        .assert()
        .success();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&out)
        .args([
            "--",
            "sh",
            "-c",
            "test -f \"$PEITHO_DIST/manifest.json\" && printf published > \"$1\"",
            "peitho-test",
        ])
        .arg(&probe)
        .assert()
        .success();

    assert_eq!(fs::read_to_string(probe).unwrap(), "published");
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
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
