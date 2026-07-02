# Peitho Milestone 6 Publish and Watch Plan

## Purpose

Milestone 6 completes the CLI split from spec section 12:

- `peitho build` creates the static distribution in `dist/`.
- `peitho present` creates the volatile presentation cache in `.peitho/present-cache/`.
- `peitho publish` validates an already-built `dist/` and delegates deployment to an external command.

This milestone also adds `peitho build --watch`, which rebuilds when the Markdown, template, base CSS, or overrides CSS file changes. It does not add deploy configuration, README content, shell features, or publish-specific generated assets.

Registry check performed for watch dependencies:

- `notify` newest crates.io version on 2026-07-02 is `9.0.0-rc.4`, which is a release candidate. Use latest stable `notify = "8.2.0"`.
- `notify-debouncer-mini = "0.7.0"` is current stable and depends on the notify 8 line.

The real filesystem watcher is intentionally thin. It watches the parent directories of the four input files, not the files themselves, so atomic-save editors that replace a file by rename do not detach the watcher after the first save. Debounced event paths are filtered through `watch_paths()` and `same_watch_path()`. Tests cover parent-directory de-duplication, the deterministic rebuild handler, and publish validation. A full notify spawn test is omitted because kernel watcher delivery and editor write behavior are platform- and timing-dependent; the production loop only converts debounced paths into the tested handler call.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `Cargo.toml` | workspace dependency versions for `notify` and `notify-debouncer-mini` | crates.io |
| `crates/peitho/Cargo.toml` | CLI crate dependencies for watch | workspace dependencies |
| `crates/peitho-core/src/domain.rs` | strict `SlideKey` deserialization for manifest validation | serde |
| `crates/peitho-core/src/manifest.rs` | deserialize manifest and expose read-only accessors | `SlideKey` |
| `crates/peitho/src/main.rs` | `publish` command, `build --watch`, dist validation, command execution, watcher loop | `peitho-core`, `notify`, `notify-debouncer-mini` |
| `crates/peitho/tests/publish.rs` | publish integration tests | CLI binary, temp fixtures |
| `crates/peitho/tests/build.rs` | one CLI regression for `build --watch` flag parsing is not needed here; watch handler tests live in `main.rs` | existing build fixtures |

No `.gitignore` or CI changes are required. Existing `.gitignore` already excludes `dist/`, `target/`, `.peitho/`, `node_modules/`, and `packages/peitho-present/dist/`.

Dependency direction:

1. `peitho-core` owns manifest schema and strict key parsing.
2. `peitho` validates `dist/` using the core manifest type.
3. `publish` runs an external command only after validation.
4. `watch` reuses the same build pipeline as one-shot `build`.

## Implementation Tasks

### Task 1 - Make Manifest Deserializable for Publish Validation

Goal: publish validation reads `manifest.json` through `peitho-core` types instead of a CLI-local schema.

Files:

- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/manifest.rs`

Test:

```rust
// crates/peitho-core/src/manifest.rs
#[test]
fn deserializes_manifest_schema_for_publish_validation() {
    let json = concat!(
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
    );

    let manifest: Manifest = serde_json::from_str(json).unwrap();

    assert_eq!(manifest.slide_count(), 1);
    assert_eq!(manifest.slides()[0].src(), "slides/000-arch-1.html");
    assert_eq!(manifest.slides()[0].key().as_str(), "arch-1");
}

#[test]
fn rejects_invalid_slide_key_when_deserializing_manifest() {
    let json = concat!(
        "{\n",
        "  \"version\": 1,\n",
        "  \"peithoVersion\": \"0.1.0\",\n",
        "  \"title\": \"Deck\",\n",
        "  \"slideCount\": 1,\n",
        "  \"slides\": [\n",
        "    {\n",
        "      \"index\": 0,\n",
        "      \"key\": \"bad key\",\n",
        "      \"src\": \"slides/000-bad.html\",\n",
        "      \"hasNotes\": false\n",
        "    }\n",
        "  ]\n",
        "}\n"
    );

    let err = serde_json::from_str::<Manifest>(json).unwrap_err();

    assert!(err.to_string().contains("slide key must use lowercase ascii"));
}
```

Expected Red:

```text
the trait bound `manifest::Manifest: serde::Deserialize<'de>` is not satisfied
```

Implementation:

```rust
// crates/peitho-core/src/domain.rs
use serde::{Deserialize, Deserializer, Serialize};

impl<'de> Deserialize<'de> for SlideKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}
```

```rust
// crates/peitho-core/src/manifest.rs
use serde::{Deserialize, Serialize};

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    version: u8,
    #[serde(rename = "peithoVersion")]
    peitho_version: String,
    title: String,
    #[serde(rename = "slideCount")]
    slide_count: usize,
    slides: Vec<ManifestSlide>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSlide {
    index: usize,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "string"))]
    key: SlideKey,
    src: String,
    #[serde(rename = "hasNotes")]
    has_notes: bool,
}

impl Manifest {
    pub fn slide_count(&self) -> usize {
        self.slide_count
    }

    pub fn slides(&self) -> &[ManifestSlide] {
        &self.slides
    }
}

impl ManifestSlide {
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn key(&self) -> &SlideKey {
        &self.key
    }

    pub fn src(&self) -> &str {
        &self.src
    }

    pub fn has_notes(&self) -> bool {
        self.has_notes
    }
}
```

Verification:

```sh
cargo test -p peitho-core manifest::tests::deserializes_manifest_schema_for_publish_validation
cargo test -p peitho-core manifest::tests::rejects_invalid_slide_key_when_deserializing_manifest
```

### Task 2 - Add Watch Dependencies

Goal: make the CLI crate compile with the stable watcher and debouncer crates.

Files:

- `Cargo.toml`
- `crates/peitho/Cargo.toml`

Test:

```rust
// crates/peitho/src/main.rs
#[test]
fn watch_dependency_types_are_available() {
    fn accepts_recursive_mode(_mode: notify::RecursiveMode) {}

    accepts_recursive_mode(notify::RecursiveMode::NonRecursive);
    let result: notify_debouncer_mini::DebounceEventResult = Ok(Vec::new());

    assert!(result.unwrap().is_empty());
}
```

Expected Red before dependency changes:

```text
unresolved import `notify`
unresolved import `notify_debouncer_mini`
```

Implementation:

```toml
# Cargo.toml
[workspace.dependencies]
assert_cmd = "2"
clap = { version = "4", features = ["derive"] }
html-escape = "0.2"
lol_html = "2"
miette = { version = "7", features = ["fancy"] }
notify = "8.2.0"
notify-debouncer-mini = "0.7.0"
predicates = "3"
pulldown-cmark = "0.10"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tempfile = "3"
tiny_http = "0.12.0"
ts-rs = { version = "12.0.1", default-features = true }
```

```toml
# crates/peitho/Cargo.toml
[dependencies]
clap.workspace = true
miette.workspace = true
notify.workspace = true
notify-debouncer-mini.workspace = true
peitho-core = { path = "../peitho-core" }
serde_json.workspace = true
tiny_http.workspace = true
```

Verification:

```sh
cargo test -p peitho watch_dependency_types_are_available
```

### Task 3 - Add Publish Command and Completeness Validation

Goal: `peitho publish --dist <dir> -- <command>` fails before command execution when the static distribution is incomplete.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/publish.rs`

Test:

```rust
// crates/peitho/tests/publish.rs
use std::{fs, path::Path};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn write_valid_dist(root: &Path) {
    fs::create_dir_all(root.join("slides")).unwrap();
    fs::write(root.join("index.html"), "<!doctype html><main id=\"peitho-slides\"></main>").unwrap();
    fs::write(root.join("peitho.css"), ".slot-title { font-weight: 700; }\n").unwrap();
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
    fs::write(root.join("slides/000-arch-1.html"), "<section data-slide-key=\"arch-1\"></section>").unwrap();
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
        .stderr(predicate::str::contains("slides/ must contain at least one file"))
        .stderr(predicate::str::contains("help: run `peitho build` first"));
}
```

Expected Red:

```text
error: unrecognized subcommand 'publish'
```

Implementation:

```rust
// crates/peitho/src/main.rs
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Subcommand)]
enum Command {
    Build {
        input: PathBuf,
        #[arg(long, default_value = "templates/title-body-code.html")]
        template: PathBuf,
        #[arg(long, default_value = "themes/base.css")]
        base_css: PathBuf,
        #[arg(long, default_value = "themes/overrides.css")]
        overrides_css: PathBuf,
        #[arg(long, default_value = "dist")]
        out: PathBuf,
    },
    Present {
        input: PathBuf,
        #[arg(long, default_value = "templates/title-body-code.html")]
        template: PathBuf,
        #[arg(long, default_value = "themes/base.css")]
        base_css: PathBuf,
        #[arg(long, default_value = "themes/overrides.css")]
        overrides_css: PathBuf,
        #[arg(long, default_value = "packages/peitho-present/dist/shell.js")]
        shell: PathBuf,
        #[arg(long, default_value_t = 0)]
        port: u16,
        #[arg(long)]
        no_open: bool,
        #[arg(long)]
        no_serve: bool,
    },
    Publish {
        #[arg(long, default_value = "dist")]
        dist: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            input,
            template,
            base_css,
            overrides_css,
            out,
        } => build(&input, &template, &base_css, &overrides_css, &out),
        Command::Present {
            input,
            template,
            base_css,
            overrides_css,
            shell,
            port,
            no_open,
            no_serve,
        } => present(PresentOptions {
            input,
            template,
            base_css,
            overrides_css,
            shell,
            port,
            no_open,
            no_serve,
        }),
        Command::Publish { dist, command: _ } => {
            validate_publish_dist(&dist)?;
            Ok(())
        }
    }
}

struct PublishDistribution {
    dist: PathBuf,
}

fn validate_publish_dist(dist: &Path) -> miette::Result<PublishDistribution> {
    require_dist_file(dist, "index.html")?;
    require_dist_file(dist, "manifest.json")?;
    require_dist_file(dist, "peitho.css")?;
    require_slides_dir_with_files(dist)?;

    read_publish_manifest(dist)?;
    let canonical = fs::canonicalize(dist).map_err(|err| {
        miette::miette!(
            "distribution is incomplete: failed to resolve {}\nhelp: run `peitho build` first\ncaused by: {err}",
            dist.display()
        )
    })?;

    Ok(PublishDistribution {
        dist: canonical,
    })
}

fn read_publish_manifest(dist: &Path) -> miette::Result<peitho_core::Manifest> {
    let path = dist.join("manifest.json");
    let json = fs::read_to_string(&path).map_err(|err| {
        miette::miette!(
            "failed to read manifest.json\nhelp: run `peitho build` first\ncaused by: {err}"
        )
    })?;

    serde_json::from_str(&json).map_err(|err| {
        miette::miette!(
            "failed to parse manifest.json\nhelp: run `peitho build` first\ncaused by: {err}"
        )
    })
}

fn require_dist_file(dist: &Path, file: &str) -> miette::Result<()> {
    let path = dist.join(file);
    if path.is_file() {
        return Ok(());
    }

    Err(miette::miette!(
        "distribution is incomplete: missing {file}\nhelp: run `peitho build` first"
    ))
}

fn require_slides_dir_with_files(dist: &Path) -> miette::Result<()> {
    let slides = dist.join("slides");
    if !slides.is_dir() {
        return Err(miette::miette!(
            "distribution is incomplete: missing slides/\nhelp: run `peitho build` first"
        ));
    }

    let mut has_file = false;
    for entry in fs::read_dir(&slides).into_diagnostic()? {
        if entry.into_diagnostic()?.file_type().into_diagnostic()?.is_file() {
            has_file = true;
            break;
        }
    }
    if has_file {
        Ok(())
    } else {
        Err(miette::miette!(
            "distribution is incomplete: slides/ must contain at least one file\nhelp: run `peitho build` first"
        ))
    }
}
```

Verification:

```sh
cargo test -p peitho --test publish publish_rejects_missing_distribution
cargo test -p peitho --test publish publish_rejects_distribution_without_slide_fragments
```

### Task 4 - Reject Presentation-Only Files in Publish Dist

Goal: publish fails when volatile present-cache artifacts are mixed into `dist/`.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/publish.rs`

Test:

```rust
// crates/peitho/tests/publish.rs
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
```

Expected Red:

```text
test fails because presenter.html is ignored
```

Implementation:

```rust
// crates/peitho/src/main.rs
const PRESENTATION_ONLY_DIST_FILES: &[&str] =
    &["present.html", "presenter.html", "notes.json", "shell.js"];

fn validate_publish_dist(dist: &Path) -> miette::Result<PublishDistribution> {
    require_dist_file(dist, "index.html")?;
    require_dist_file(dist, "manifest.json")?;
    require_dist_file(dist, "peitho.css")?;
    require_slides_dir_with_files(dist)?;
    reject_presentation_only_files(dist)?;

    read_publish_manifest(dist)?;
    let canonical = fs::canonicalize(dist).map_err(|err| {
        miette::miette!(
            "distribution is incomplete: failed to resolve {}\nhelp: run `peitho build` first\ncaused by: {err}",
            dist.display()
        )
    })?;

    Ok(PublishDistribution {
        dist: canonical,
    })
}

fn reject_presentation_only_files(dist: &Path) -> miette::Result<()> {
    for file in PRESENTATION_ONLY_DIST_FILES {
        if dist.join(file).exists() {
            return Err(miette::miette!(
                "distribution contains presentation-only file: {file}\nhelp: remove presentation artifacts or run `peitho build` again"
            ));
        }
    }
    Ok(())
}
```

Verification:

```sh
cargo test -p peitho --test publish publish_rejects_presentation_only_files
```

### Task 5 - Validate Manifest Slide References

Goal: publish fails when `manifest.json` points to a missing, absolute, parent-directory, or empty slide reference.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/publish.rs`

Test:

```rust
// crates/peitho/tests/publish.rs
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
```

Expected Red:

```text
test fails because manifest slide refs are not checked
```

Implementation:

```rust
// crates/peitho/src/main.rs
use std::path::Component;

fn read_publish_manifest(dist: &Path) -> miette::Result<peitho_core::Manifest> {
    let path = dist.join("manifest.json");
    let json = fs::read_to_string(&path).map_err(|err| {
        miette::miette!(
            "failed to read manifest.json\nhelp: run `peitho build` first\ncaused by: {err}"
        )
    })?;

    let manifest: peitho_core::Manifest = serde_json::from_str(&json).map_err(|err| {
        miette::miette!(
            "failed to parse manifest.json\nhelp: run `peitho build` first\ncaused by: {err}"
        )
    })?;

    validate_manifest_slide_refs(dist, &manifest)?;
    Ok(manifest)
}

fn validate_manifest_slide_refs(
    dist: &Path,
    manifest: &peitho_core::Manifest,
) -> miette::Result<()> {
    if manifest.slide_count() != manifest.slides().len() {
        return Err(miette::miette!(
            "manifest slideCount does not match slides length\nhelp: run `peitho build` first"
        ));
    }

    if manifest.slides().is_empty() || manifest.slide_count() == 0 {
        return Err(miette::miette!(
            "manifest has no slides\nhelp: run `peitho build` first"
        ));
    }

    for slide in manifest.slides() {
        let src = slide.src();
        let path = Path::new(src);
        let invalid_component = path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        });
        if src.is_empty() || path.is_absolute() || invalid_component {
            return Err(miette::miette!(
                "manifest contains invalid slide src: {src}\nhelp: slide src must be a relative path inside dist/"
            ));
        }

        if !dist.join(path).is_file() {
            return Err(miette::miette!(
                "manifest references missing slide fragment: {src}\nhelp: run `peitho build` first"
            ));
        }
    }

    Ok(())
}
```

Verification:

```sh
cargo test -p peitho --test publish publish_rejects_missing_manifest_slide_reference
cargo test -p peitho --test publish publish_rejects_manifest_slide_reference_outside_dist
cargo test -p peitho --test publish publish_rejects_manifest_slide_count_mismatch
```

### Task 6 - Require a Publish Command

Goal: a valid dist without a command fails with a help message that keeps deployment out of Peitho.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/publish.rs`

Test:

```rust
// crates/peitho/tests/publish.rs
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
            "help: deployment is delegated to IaC or CI; example: peitho publish -- aws s3 sync dist/ s3://bucket",
        ));
}
```

Expected Red:

```text
test fails because the command vector is accepted silently
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            input,
            template,
            base_css,
            overrides_css,
            out,
        } => build(&input, &template, &base_css, &overrides_css, &out),
        Command::Present {
            input,
            template,
            base_css,
            overrides_css,
            shell,
            port,
            no_open,
            no_serve,
        } => present(PresentOptions {
            input,
            template,
            base_css,
            overrides_css,
            shell,
            port,
            no_open,
            no_serve,
        }),
        Command::Publish { dist, command } => {
            let code = publish(&dist, &command)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
    }
}

fn publish(dist: &Path, command: &[OsString]) -> miette::Result<i32> {
    let distribution = validate_publish_dist(dist)?;
    if command.is_empty() {
        return Err(miette::miette!(
            "publish command is missing\nhelp: deployment is delegated to IaC or CI; example: peitho publish -- aws s3 sync dist/ s3://bucket"
        ));
    }

    run_publish_command(&distribution.dist, command)
}
```

Verification:

```sh
cargo test -p peitho --test publish publish_requires_external_command
```

### Task 7 - Execute Publish Command with PEITHO_DIST

Goal: validated publish runs the external command in the current working directory and sets `PEITHO_DIST` to the canonical dist path.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/publish.rs`

Test:

```rust
// crates/peitho/tests/publish.rs
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
        .args(["--", "sh", "-c", "printf '%s' \"$PEITHO_DIST\" > \"$1\"", "peitho-test"])
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
```

Expected Red:

```text
probe.txt does not exist
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn run_publish_command(dist: &Path, command: &[OsString]) -> miette::Result<i32> {
    let executable = &command[0];
    let status = std::process::Command::new(executable)
        .args(&command[1..])
        .env("PEITHO_DIST", dist)
        .status()
        .map_err(|err| {
            miette::miette!(
                "failed to run publish command: {}\nhelp: check that the command exists and is executable\ncaused by: {err}",
                executable.to_string_lossy()
            )
        })?;

    Ok(status.code().unwrap_or(1))
}
```

Verification:

```sh
cargo test -p peitho --test publish publish_runs_command_with_peitho_dist_env
cargo test -p peitho --test publish publish_propagates_command_exit_code
```

### Task 8 - Refactor Build Arguments into BuildOptions

Goal: one-shot build and watch use the same input structure without changing existing build behavior.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/src/main.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_options_lists_watched_input_paths() {
        let options = BuildOptions {
            input: PathBuf::from("deck.md"),
            template: PathBuf::from("template.html"),
            base_css: PathBuf::from("base.css"),
            overrides_css: PathBuf::from("overrides.css"),
            out: PathBuf::from("dist"),
        };

        assert_eq!(
            options.watch_paths(),
            [
                PathBuf::from("deck.md"),
                PathBuf::from("template.html"),
                PathBuf::from("base.css"),
                PathBuf::from("overrides.css"),
            ]
        );
    }

    #[test]
    fn build_options_deduplicates_watch_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let options = BuildOptions {
            input: dir.path().join("deck.md"),
            template: dir.path().join("title-body-code.html"),
            base_css: dir.path().join("base.css"),
            overrides_css: dir.path().join("overrides.css"),
            out: dir.path().join("dist"),
        };

        assert_eq!(options.watch_dirs(), vec![dir.path().to_path_buf()]);
    }
}
```

Expected Red:

```text
cannot find struct `BuildOptions` in this scope
```

Implementation:

```rust
// crates/peitho/src/main.rs
#[derive(Debug, Clone)]
struct BuildOptions {
    input: PathBuf,
    template: PathBuf,
    base_css: PathBuf,
    overrides_css: PathBuf,
    out: PathBuf,
}

impl BuildOptions {
    fn watch_paths(&self) -> [PathBuf; 4] {
        [
            self.input.clone(),
            self.template.clone(),
            self.base_css.clone(),
            self.overrides_css.clone(),
        ]
    }

    fn watch_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        for path in self.watch_paths() {
            let dir = parent_dir_for_watch(&path);
            if !dirs
                .iter()
                .any(|existing| same_watch_path(existing, &dir))
            {
                dirs.push(dir);
            }
        }
        dirs
    }
}

fn parent_dir_for_watch(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn same_watch_path(left: &Path, right: &Path) -> bool {
    left == right
        || match (fs::canonicalize(left), fs::canonicalize(right)) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
}

fn build(options: &BuildOptions) -> miette::Result<()> {
    let artifacts = build_artifacts(
        &options.input,
        &options.template,
        &options.base_css,
        &options.overrides_css,
    )?;
    emit_distribution(&options.out, &artifacts)?;
    println!(
        "built {} slide(s) into {}",
        artifacts.slide_count,
        options.out.display()
    );
    Ok(())
}
```

The existing `build_artifacts()` function keeps its current signature so `present()` can continue to call it with explicit paths.

Verification:

```sh
cargo test -p peitho build_options_lists_watched_input_paths
cargo test -p peitho build_options_deduplicates_watch_parent_dirs
cargo test -p peitho --test build build_still_writes_distribution_after_pipeline_refactor
```

### Task 9 - Add One-Shot Watch Rebuild Success Path

Goal: watch mode has a deterministic rebuild function that writes a one-line success message and updates the output directory.

Files:

- `crates/peitho/src/main.rs`

Test:

```rust
// crates/peitho/src/main.rs
#[test]
fn watch_rebuild_once_writes_distribution_and_success_line() {
    let fixture = WatchFixture::new("# Intro\n\nBody\n");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();

    assert!(stderr.is_empty());
    assert!(String::from_utf8(stdout).unwrap().contains("built 1 slide(s)"));
    assert!(fixture.options.out.join("manifest.json").exists());
    assert!(fixture.options.out.join("slides/000-intro.html").exists());
}

struct WatchFixture {
    _dir: tempfile::TempDir,
    options: BuildOptions,
}

impl WatchFixture {
    fn new(markdown: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let template = dir.path().join("title-body-code.html");
        let base = dir.path().join("base.css");
        let overrides = dir.path().join("overrides.css");
        let out = dir.path().join("dist");

        fs::write(&deck, markdown).unwrap();
        fs::write(
            &template,
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#,
        )
        .unwrap();
        fs::write(&base, ".slot-title { font-weight: 700; }\n").unwrap();
        fs::write(&overrides, "").unwrap();

        Self {
            _dir: dir,
            options: BuildOptions {
                input: deck,
                template,
                base_css: base,
                overrides_css: overrides,
                out,
            },
        }
    }
}
```

Expected Red:

```text
cannot find function `rebuild_once_for_watch` in this scope
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn rebuild_once_for_watch(
    options: &BuildOptions,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    match build_artifacts(
        &options.input,
        &options.template,
        &options.base_css,
        &options.overrides_css,
    ) {
        Ok(artifacts) => {
            match emit_distribution(&options.out, &artifacts) {
                Ok(()) => {
                    writeln!(
                        stdout,
                        "built {} slide(s) into {}",
                        artifacts.slide_count,
                        options.out.display()
                    )
                    .into_diagnostic()?;
                }
                Err(err) => {
                    writeln!(stderr, "build failed: {err}").into_diagnostic()?;
                }
            }
        }
        Err(err) => {
            writeln!(stderr, "build failed: {err}").into_diagnostic()?;
        }
    }

    Ok(())
}
```

Verification:

```sh
cargo test -p peitho watch_rebuild_once_writes_distribution_and_success_line
```

### Task 10 - Keep Watch Alive After Build Failure

Goal: a failed rebuild reports the existing build error and returns `Ok(())` so the watcher can recover on the next save.

Files:

- `crates/peitho/src/main.rs`

Test:

```rust
// crates/peitho/src/main.rs
#[test]
fn watch_rebuild_once_reports_failure_without_returning_error() {
    let fixture = WatchFixture::new("# Intro\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();

    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("build failed:"));
    assert!(stderr.contains("slot 'code' got 2 item(s)"));
    assert!(stderr.contains("help: use a layout with more code capacity or remove one code block"));
}

#[test]
fn watch_rebuild_once_reports_emit_failure_without_returning_error() {
    let fixture = WatchFixture::new("# Intro\n");
    fs::write(&fixture.options.out, "not a directory").unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();

    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("build failed:"));
}
```

Expected Red:

```text
watch_rebuild_once returns Err instead of Ok
```

Implementation:

The Task 9 implementation catches both `build_artifacts()` and `emit_distribution()` errors and writes `build failed: {err}` to `stderr`. Keep both `match` blocks intact; do not replace either failure path with `?`.

Verification:

```sh
cargo test -p peitho watch_rebuild_once_reports_failure_without_returning_error
cargo test -p peitho watch_rebuild_once_reports_emit_failure_without_returning_error
```

### Task 11 - Rebuild on Debounced Watched Paths

Goal: the event handler rebuilds when one of the four watched files changes and ignores unrelated paths.

Files:

- `crates/peitho/src/main.rs`

Test:

```rust
// crates/peitho/src/main.rs
#[test]
fn watch_path_handler_rebuilds_after_markdown_change() {
    let fixture = WatchFixture::new("# Intro\n");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    rebuild_once_for_watch(&fixture.options, &mut stdout, &mut stderr).unwrap();
    fs::write(&fixture.options.input, "# Intro\n\n---\n# Details\n").unwrap();

    handle_watch_paths(
        &fixture.options,
        &[fixture.options.input.clone()],
        &mut stdout,
        &mut stderr,
    )
    .unwrap();

    let manifest = fs::read_to_string(fixture.options.out.join("manifest.json")).unwrap();
    assert!(manifest.contains(r#""slideCount": 2"#));
    assert!(String::from_utf8(stdout).unwrap().contains("built 2 slide(s)"));
    assert!(stderr.is_empty());
}

#[test]
fn watch_path_handler_ignores_unwatched_file() {
    let fixture = WatchFixture::new("# Intro\n");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let unrelated = fixture.options.out.join("ignored.txt");

    handle_watch_paths(&fixture.options, &[unrelated], &mut stdout, &mut stderr).unwrap();

    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert!(!fixture.options.out.join("manifest.json").exists());
}
```

Expected Red:

```text
cannot find function `handle_watch_paths` in this scope
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn handle_watch_paths(
    options: &BuildOptions,
    changed_paths: &[PathBuf],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    let watched = options.watch_paths();
    let relevant = changed_paths
        .iter()
        .any(|changed| watched.iter().any(|path| same_watch_path(path, changed)));

    if relevant {
        rebuild_once_for_watch(options, stdout, stderr)?;
    }

    Ok(())
}
```

Verification:

```sh
cargo test -p peitho watch_path_handler_rebuilds_after_markdown_change
cargo test -p peitho watch_path_handler_ignores_unwatched_file
```

### Task 12 - Implement the Notify Watch Loop

Goal: `build --watch` performs an initial build, watches the de-duplicated parent directories of the four input files with a 200ms debounce, filters event paths back to the four files, and keeps running after rebuild errors.

Files:

- `crates/peitho/src/main.rs`

Test:

The deterministic tests from Tasks 9-11 are the acceptance tests for rebuild behavior. This task adds a compile test that proves the real notify loop exists without entering the infinite loop.

```rust
// crates/peitho/src/main.rs
#[test]
fn watch_build_function_is_available_for_cli_dispatch() {
    let _watch: fn(BuildOptions) -> miette::Result<()> = watch_build;
}
```

Expected Red:

```text
cannot find function `watch_build` in this scope
```

Implementation:

```rust
// crates/peitho/src/main.rs
use std::{sync::mpsc, time::Duration};

use notify::RecursiveMode;
use notify::Watcher;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};

fn watch_build(options: BuildOptions) -> miette::Result<()> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(200), tx).map_err(|err| {
        miette::miette!("failed to start file watcher\nhelp: check file watcher permissions\ncaused by: {err}")
    })?;

    for dir in options.watch_dirs() {
        debouncer
            .watcher()
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|err| {
                miette::miette!(
                    "failed to watch {}\nhelp: verify the parent directory exists before starting --watch\ncaused by: {err}",
                    dir.display()
                )
            })?;
    }

    println!("watching parent directories for markdown, template, base css, and overrides css");
    rebuild_once_for_watch(&options, &mut std::io::stdout(), &mut std::io::stderr())?;

    for result in rx {
        match result {
            Ok(events) => {
                let paths = events.into_iter().map(|event| event.path).collect::<Vec<_>>();
                handle_watch_paths(
                    &options,
                    &paths,
                    &mut std::io::stdout(),
                    &mut std::io::stderr(),
                )?;
            }
            Err(err) => {
                eprintln!("watch error: {err}");
            }
        }
    }

    Ok(())
}
```

Ctrl-C uses the process default signal behavior and exits the watch process. No custom signal handler is added.

Verification:

```sh
cargo test -p peitho watch_build_function_is_available_for_cli_dispatch
cargo check -p peitho
```

### Task 13 - Add the Build Watch Flag

Goal: `peitho build <md> --watch` parses to the build command with `watch = true` and dispatches to the real watcher.

Files:

- `crates/peitho/src/main.rs`

Test:

```rust
// crates/peitho/src/main.rs
#[test]
fn build_command_accepts_watch_flag() {
    let cli = Cli::parse_from(["peitho", "build", "deck.md", "--watch"]);

    match cli.command {
        Command::Build { input, watch, .. } => {
            assert_eq!(input, PathBuf::from("deck.md"));
            assert!(watch);
        }
        Command::Present { .. } | Command::Publish { .. } => {
            panic!("expected build command");
        }
    }
}
```

Expected Red:

```text
error: unexpected argument '--watch' found
```

Implementation:

```rust
// crates/peitho/src/main.rs
#[derive(Debug, Subcommand)]
enum Command {
    Build {
        input: PathBuf,
        #[arg(long, default_value = "templates/title-body-code.html")]
        template: PathBuf,
        #[arg(long, default_value = "themes/base.css")]
        base_css: PathBuf,
        #[arg(long, default_value = "themes/overrides.css")]
        overrides_css: PathBuf,
        #[arg(long, default_value = "dist")]
        out: PathBuf,
        #[arg(long)]
        watch: bool,
    },
    Present {
        input: PathBuf,
        #[arg(long, default_value = "templates/title-body-code.html")]
        template: PathBuf,
        #[arg(long, default_value = "themes/base.css")]
        base_css: PathBuf,
        #[arg(long, default_value = "themes/overrides.css")]
        overrides_css: PathBuf,
        #[arg(long, default_value = "packages/peitho-present/dist/shell.js")]
        shell: PathBuf,
        #[arg(long, default_value_t = 0)]
        port: u16,
        #[arg(long)]
        no_open: bool,
        #[arg(long)]
        no_serve: bool,
    },
    Publish {
        #[arg(long, default_value = "dist")]
        dist: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
}

Command::Build {
    input,
    template,
    base_css,
    overrides_css,
    out,
    watch,
} => {
    let options = BuildOptions {
        input,
        template,
        base_css,
        overrides_css,
        out,
    };
    if watch {
        watch_build(options)
    } else {
        build(&options)
    }
}
```

Verification:

```sh
cargo test -p peitho build_command_accepts_watch_flag
```

### Task 14 - Publish the Repository Example Through a Shell Command

Goal: the real repository example can be built, validated, and handed to an external publish command.

Files:

- `crates/peitho/tests/publish.rs`

Test:

```rust
// crates/peitho/tests/publish.rs
#[test]
fn repository_example_can_be_published_to_external_command() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("dist");
    let probe = dir.path().join("published.txt");

    Command::cargo_bin("peitho")
        .unwrap()
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
        ])
        .arg(&out)
        .assert()
        .success();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&out)
        .args(["--", "sh", "-c", "test -f \"$PEITHO_DIST/manifest.json\" && printf published > \"$1\"", "peitho-test"])
        .arg(&probe)
        .assert()
        .success();

    assert_eq!(fs::read_to_string(probe).unwrap(), "published");
}
```

Expected Red:

```text
publish subcommand or PEITHO_DIST behavior is incomplete
```

Implementation:

This task adds the test above. The production path is the same `publish()` path from Tasks 3-7:

```rust
fn publish(dist: &Path, command: &[OsString]) -> miette::Result<i32> {
    let distribution = validate_publish_dist(dist)?;
    if command.is_empty() {
        return Err(miette::miette!(
            "publish command is missing\nhelp: deployment is delegated to IaC or CI; example: peitho publish -- aws s3 sync dist/ s3://bucket"
        ));
    }

    run_publish_command(&distribution.dist, command)
}
```

The test passes only when that single path accepts the output from `peitho build`, rejects presentation-cache files, and sets `PEITHO_DIST`.

Verification:

```sh
cargo test -p peitho --test publish repository_example_can_be_published_to_external_command
```

### Task 15 - Manual Watch Smoke Command

Goal: document the local smoke command used to confirm the real watcher starts without adding a flaky CI test.

Files:

- `crates/peitho/src/main.rs`

Test:

The automated tests remain the unit tests from Tasks 10-13. The manual smoke command is run locally and stopped with Ctrl-C after the initial build line appears:

```sh
cargo run -p peitho -- build examples/deck.md \
  --template templates/title-body-code.html \
  --base-css themes/base.css \
  --overrides-css themes/overrides.css \
  --out /private/tmp/peitho-watch-dist \
  --watch
```

Expected output marker:

```text
watching parent directories for markdown, template, base css, and overrides css
built 3 slide(s) into /private/tmp/peitho-watch-dist
```

Implementation:

The production code under verification is the `watch_build()` loop from Task 12:

```rust
fn watch_build(options: BuildOptions) -> miette::Result<()> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(200), tx).map_err(|err| {
        miette::miette!("failed to start file watcher\nhelp: check file watcher permissions\ncaused by: {err}")
    })?;

    for dir in options.watch_dirs() {
        debouncer
            .watcher()
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|err| {
                miette::miette!(
                    "failed to watch {}\nhelp: verify the parent directory exists before starting --watch\ncaused by: {err}",
                    dir.display()
                )
            })?;
    }

    println!("watching parent directories for markdown, template, base css, and overrides css");
    rebuild_once_for_watch(&options, &mut std::io::stdout(), &mut std::io::stderr())?;

    for result in rx {
        match result {
            Ok(events) => {
                let paths = events.into_iter().map(|event| event.path).collect::<Vec<_>>();
                handle_watch_paths(
                    &options,
                    &paths,
                    &mut std::io::stdout(),
                    &mut std::io::stderr(),
                )?;
            }
            Err(err) => {
                eprintln!("watch error: {err}");
            }
        }
    }

    Ok(())
}
```

The command is stopped with Ctrl-C after the expected output marker appears.

Verification:

```sh
cargo test -p peitho build_options_deduplicates_watch_parent_dirs
cargo test -p peitho watch_rebuild_once_writes_distribution_and_success_line
cargo test -p peitho watch_rebuild_once_reports_failure_without_returning_error
cargo test -p peitho watch_path_handler_rebuilds_after_markdown_change
cargo test -p peitho watch_path_handler_ignores_unwatched_file
cargo test -p peitho watch_build_function_is_available_for_cli_dispatch
```

### Task 16 - Final Gates

Goal: verify the whole workspace after publish and watch are complete.

Files:

- `Cargo.toml`
- `crates/peitho/Cargo.toml`
- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/manifest.rs`
- `crates/peitho/src/main.rs`
- `crates/peitho/tests/publish.rs`

Test:

```sh
cargo test --workspace
git diff --exit-code bindings/
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Implementation:

Milestone 6 cleanup actions are limited to these concrete changes:

- use `cargo fmt --all` for formatting;
- remove unused imports or dead helper functions reported by clippy;
- keep bindings drift clean after manifest derive/accessor changes.

Verification:

```sh
cargo test --workspace
git diff --exit-code bindings/
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```
