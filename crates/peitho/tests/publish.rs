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
    let out = dir.path().join("dist");
    let probe = dir.path().join("published.txt");

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
        ])
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
