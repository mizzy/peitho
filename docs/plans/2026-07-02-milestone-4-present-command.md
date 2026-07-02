# Peitho Milestone 4 Present Command Plan

## Purpose

Milestone 4 adds the volatile presentation path: `peitho present <md>` builds the same checked slide model as `build`, emits a fresh `.peitho/present-cache/`, serves it over HTTP, opens a browser when requested, and enables two-window synchronization through a TypeScript `BroadcastChannel` bridge. The persistent distribution output remains unchanged: `dist/index.html` does not receive the presentation shell or notes.

Cache policy: M4 chooses clean regeneration over incremental reuse. Every `present` run removes `.peitho/present-cache/` with `remove_dir_all` and writes a new cache. This settles the M4 behavior without deciding the future watch/incremental policy left open in §18.

Version note: `tiny_http 0.12.0` was confirmed on crates.io before writing this plan. During implementation, run `CARGO_HOME=/private/tmp/peitho-cargo-home cargo info tiny_http@0.12.0` or plain `cargo info tiny_http@0.12.0` when the local Cargo cache is writable. If the version is unavailable, use the latest stable crates.io version and report the adjustment.

Out of scope: presenter-view UI, timers, `peitho:presentationstart/end`, `peitho:timercontrol`, notes Markdown syntax, `publish`, and `--watch`.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `Cargo.toml` | workspace dependency pin for `tiny_http = "0.12.0"` | crates.io |
| `.gitignore` | ignore `.peitho/` volatile present cache | repo root |
| `crates/peitho-core/src/notes.rs` | `notes_json(&Notes)` serialization helper | existing `Notes` |
| `crates/peitho-core/src/render.rs` | `render_present_index()` for `present.html` | TS shell API |
| `crates/peitho-core/src/lib.rs` | export `notes_json` and `render_present_index` | core modules |
| `crates/peitho/src/main.rs` | `present` subcommand, shared build pipeline, cache emission, browser open | `peitho-core`, `server.rs` |
| `crates/peitho/src/lib.rs` | exposes `server` for integration tests | `server.rs` |
| `crates/peitho/src/server.rs` | tiny_http static server, path traversal guard, content types | `tiny_http` |
| `crates/peitho/tests/present.rs` | CLI present cache, shell missing, HTTP/path tests | `assert_cmd`, `tempfile` |
| `packages/peitho-present/src/sync.ts` | BroadcastChannel bridge between `slidechange` and `navigate` | DOM events |
| `packages/peitho-present/src/index.ts` | export `installSyncBridge` | `sync.ts` |
| `packages/peitho-present/test/sync.test.ts` | mocked-channel bridge tests and echo-stop regression | shell + keyboard APIs |
| `.github/workflows/ci.yml` | build `shell.js` before Rust tests and keep the existing Node job | npm + Rust CI |

Dependency direction: Rust emits `present.html` and copies the already-built `shell.js`; Rust does not embed TypeScript output. TypeScript imports generated bindings for manifest and stays unaware of the Rust CLI. The sync bridge is layered above DOM events and never calls shell internals.

## Implementation Tasks

### Task 1 - Add Volatile Cache Ignore and tiny_http Dependency

Goal: make the volatile cache untracked and add the only new Rust dependency.

Files:

- `.gitignore`
- `Cargo.toml`
- `crates/peitho/Cargo.toml`

Test:

```bash
cargo metadata --format-version 1 --no-deps | rg '"tiny_http"'
rg -n '^\\.peitho/$' .gitignore
```

Expected Red before implementation:

```text
no matches found for "tiny_http"
no matches found for ".peitho/"
```

Implementation:

```gitignore
# .gitignore
/target
/dist
.peitho/
node_modules/
packages/peitho-present/node_modules/
packages/peitho-present/dist/
```

```toml
# Cargo.toml
[workspace.dependencies]
tiny_http = "0.12.0"
```

```toml
# crates/peitho/Cargo.toml
[dependencies]
tiny_http.workspace = true
```

Verification:

```bash
cargo metadata --format-version 1 --no-deps | rg '"tiny_http"'
rg -n '^\\.peitho/$' .gitignore
cargo test -p peitho --no-run
```

### Task 2 - Serialize Empty Notes JSON from peitho-core

Goal: present emits `notes.json` from the existing Rust `Notes` schema, not ad hoc CLI JSON.

Files:

- `crates/peitho-core/src/notes.rs`
- `crates/peitho-core/src/lib.rs`

Test:

```rust
// crates/peitho-core/src/notes.rs
#[test]
fn serializes_notes_json_with_trailing_newline() {
    let json = notes_json(&Notes::empty()).unwrap();

    assert_eq!(
        json,
        "{\n  \"version\": 1,\n  \"notes\": {}\n}\n"
    );
}
```

Implementation:

```rust
// crates/peitho-core/src/notes.rs
use crate::error::{BuildError, ErrorKind, Result};

pub fn notes_json(notes: &Notes) -> Result<String> {
    let mut json = serde_json::to_string_pretty(notes).map_err(|err| {
        BuildError::new(
            ErrorKind::Manifest,
            None,
            format!("failed to serialize notes: {err}"),
            "keep notes fields serializable",
        )
    })?;
    json.push('\n');
    Ok(json)
}
```

```rust
// crates/peitho-core/src/lib.rs
pub use notes::{notes_json, Notes};
```

Verification:

```bash
cargo test -p peitho-core notes::tests::serializes_notes_json_with_trailing_newline
```

### Task 3 - Render present.html in peitho-core

Goal: generate the volatile presentation entrypoint that imports `shell.js`, mounts the shell, installs keyboard navigation and sync bridge, and fetches `notes.json`.

Files:

- `crates/peitho-core/src/render.rs`
- `crates/peitho-core/src/lib.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn present_index_mounts_shell_keyboard_sync_and_notes() {
    let html = render_present_index();

    assert!(html.contains(r#"<main id="peitho-present-root"></main>"#));
    assert!(html.contains(
        r#"import { installKeyboardNavigation, installSyncBridge, mountPresentShell } from './shell.js';"#
    ));
    assert!(html.contains("fetchOk('notes.json')"));
    assert!(html.contains("await mountPresentShell({ root })"));
    assert!(html.contains("installKeyboardNavigation(window)"));
    assert!(html.contains("installSyncBridge(window)"));
    assert!(!html.contains("fetchOk(slide.src)"));
}
```

Implementation:

```rust
// crates/peitho-core/src/render.rs
pub fn render_present_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Present</title>
</head>
<body>
  <main id="peitho-present-root"></main>
  <script type="module">
    import { installKeyboardNavigation, installSyncBridge, mountPresentShell } from './shell.js';

    function showError(message) {
      const root = document.getElementById('peitho-present-root');
      root.textContent = message;
    }

    async function fetchOk(url) {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
      return response;
    }

    async function main() {
      const root = document.getElementById('peitho-present-root');
      try {
        window.peithoNotes = await fetchOk('notes.json').then((response) => response.json());
        await mountPresentShell({ root });
        installKeyboardNavigation(window);
        installSyncBridge(window);
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#
        .to_owned()
}
```

```rust
// crates/peitho-core/src/lib.rs
pub use render::{render_deck, render_distribution_index, render_present_index};
```

Verification:

```bash
cargo test -p peitho-core render::tests::present_index_mounts_shell_keyboard_sync_and_notes
```

### Task 4 - Add BroadcastChannel Sync Bridge Tests

Goal: specify sync bridge behavior with a mock channel before implementation.

Files:

- `packages/peitho-present/test/sync.test.ts`

Test:

```ts
// packages/peitho-present/test/sync.test.ts
import { afterEach, expect, it, vi } from "vitest";
import { installSyncBridge, mountPresentShell } from "../src/index";
import type { PresentShell } from "../src/index";
import type { SyncChannel } from "../src/sync";

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function okText(value: string): Response {
  return { ok: true, status: 200, text: async () => value } as Response;
}

const manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 2,
  slides: [
    { index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false },
    { index: 1, key: "arch-1", src: "slides/001-arch-1.html", hasNotes: false }
  ]
};

function standardFetch(): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-arch-1.html") return okText("<section><h1>Arch</h1></section>");
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  }) as unknown as typeof fetch;
}

function mockChannel() {
  const channel: SyncChannel & { sent: unknown[]; closed: boolean } = {
    sent: [],
    closed: false,
    onmessage: null,
    postMessage(message: unknown) {
      this.sent.push(message);
    },
    close() {
      this.closed = true;
    }
  };
  return channel;
}

const shells: PresentShell[] = [];
const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  while (shells.length > 0) shells.pop()?.destroy();
});

it("posts local slidechange index to peitho-sync", async () => {
  const channel = mockChannel();
  const root = document.createElement("main");
  const shell = await mountPresentShell({ root, fetcher: standardFetch(), window });
  shells.push(shell);
  cleanups.push(installSyncBridge(window, () => channel));

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(channel.sent).toEqual([{ index: 1 }]);
});

it("turns remote index messages into navigate requests", () => {
  const channel = mockChannel();
  const requests: unknown[] = [];
  const onNavigate = (event: Event) => requests.push((event as CustomEvent).detail);
  window.addEventListener("peitho:navigate", onNavigate);
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));
  cleanups.push(installSyncBridge(window, () => channel));

  channel.onmessage?.({ data: { index: 1 } });

  expect(requests).toEqual([{ to: { index: 1 } }]);
});

it("does not echo forever when remote index equals current slide", async () => {
  const channel = mockChannel();
  const root = document.createElement("main");
  const shell = await mountPresentShell({ root, fetcher: standardFetch(), window });
  shells.push(shell);
  cleanups.push(installSyncBridge(window, () => channel));

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(channel.sent).toEqual([{ index: 1 }]);
  channel.onmessage?.({ data: { index: 1 } });

  expect(shell.currentIndex).toBe(1);
  expect(channel.sent).toEqual([{ index: 1 }]);
});
```

Expected Red before implementation:

```text
Module '"../src/index"' has no exported member 'installSyncBridge'
```

Verification:

```bash
cd packages/peitho-present
npm test -- sync
```

### Task 5 - Implement and Export installSyncBridge

Goal: bridge local DOM slide changes to a `BroadcastChannel` and remote messages back to `peitho:navigate`.

Files:

- `packages/peitho-present/src/sync.ts`
- `packages/peitho-present/src/index.ts`

Implementation:

```ts
// packages/peitho-present/src/sync.ts
export type SyncMessage = { index: number };

export type SyncChannel = {
  onmessage: ((event: { data: unknown }) => void) | null;
  postMessage(message: SyncMessage): void;
  close(): void;
};

export type SyncChannelFactory = (name: string) => SyncChannel;

function defaultChannelFactory(name: string): SyncChannel {
  const channel = new BroadcastChannel(name);
  let onmessage: ((event: { data: unknown }) => void) | null = null;
  channel.onmessage = (event: MessageEvent): void => {
    onmessage?.({ data: event.data });
  };
  return {
    get onmessage() {
      return onmessage;
    },
    set onmessage(next) {
      onmessage = next;
    },
    postMessage(message: SyncMessage): void {
      channel.postMessage(message);
    },
    close(): void {
      channel.close();
    }
  };
}

export function installSyncBridge(
  win: Window = window,
  channelFactory: SyncChannelFactory = defaultChannelFactory
): () => void {
  const channel = channelFactory("peitho-sync");
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<{ index: number }>).detail;
    if (typeof detail?.index !== "number") return;
    channel.postMessage({ index: detail.index });
  };
  channel.onmessage = (event: { data: unknown }): void => {
    const data = event.data as Partial<SyncMessage>;
    if (typeof data.index !== "number") {
      console.error("Invalid peitho sync message");
      return;
    }
    win.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: data.index } } }));
  };
  win.addEventListener("peitho:slidechange", onSlideChange);
  return () => {
    win.removeEventListener("peitho:slidechange", onSlideChange);
    channel.onmessage = null;
    channel.close();
  };
}
```

```ts
// packages/peitho-present/src/index.ts
export { installSyncBridge } from "./sync";
export type { SyncChannel, SyncChannelFactory, SyncMessage } from "./sync";
```

Verification:

```bash
cd packages/peitho-present
npm test -- sync
npm run typecheck
```

### Task 6 - Bundle shell.js with Sync Bridge

Goal: ensure `shell.js` exports and contains the sync bridge while still not embedding slide bodies.

Files:

- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/esbuild.config.mjs`

Test:

```bash
cd packages/peitho-present
npm run build
rg -n "installSyncBridge|peitho-sync|BroadcastChannel|peitho:navigate|peitho:slidechange" dist/shell.js
! rg -n "Peitho Architecture|data-slide-key=\\\"arch-1\\\"" dist/shell.js
```

Implementation:

```ts
// packages/peitho-present/src/index.ts
export { installKeyboardNavigation } from "./keyboard";
export { mountPresentShell } from "./shell";
export { installSyncBridge } from "./sync";
export type {
  NavigateDetail,
  NavigateTarget,
  PresentShell,
  ShellOptions,
  SlideChangeDetail
} from "./shell";
export type { SyncChannel, SyncChannelFactory, SyncMessage } from "./sync";
```

Verification:

```bash
cd packages/peitho-present
npm run build
rg -n "installSyncBridge|peitho-sync|BroadcastChannel" dist/shell.js
```

### Task 7 - Refactor CLI Build Pipeline into Reusable Artifacts

Goal: let `build` and `present` share parse→map→check→render→theme→manifest without changing build output.

Files:

- `crates/peitho/src/main.rs`

Test:

```rust
// crates/peitho/tests/build.rs
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
```

Implementation:

```rust
// crates/peitho/src/main.rs
struct BuildArtifacts {
    slide_count: usize,
    rendered: peitho_core::Deck<peitho_core::Rendered>,
    manifest_json: String,
    css: String,
}

fn build_artifacts(
    input: &Path,
    template_path: &Path,
    base_path: &Path,
    overrides_path: &Path,
) -> miette::Result<BuildArtifacts> {
    let markdown = fs::read_to_string(input).into_diagnostic()?;
    let template_html = fs::read_to_string(template_path).into_diagnostic()?;
    let base_css = fs::read_to_string(base_path).into_diagnostic()?;
    let overrides_css = fs::read_to_string(overrides_path).into_diagnostic()?;
    let template = core(peitho_core::parse_template(template_name(template_path), &template_html))?;
    let parsed = core(peitho_core::parse_markdown(&markdown))?;
    let mapped = core(peitho_core::map_by_convention(parsed, &template))?;
    let checked = core(peitho_core::check_deck(mapped, &template))?;
    let slide_count = checked.slide_count();
    let manifest_json = core(peitho_core::manifest_json(&peitho_core::build_manifest(&checked)))?;
    let css = core(peitho_core::build_theme_css(
        &base_css,
        &overrides_css,
        checked.slide_keys(),
        &template,
    ))?;
    let rendered = core(peitho_core::render_deck(checked, &template))?;
    Ok(BuildArtifacts { slide_count, rendered, manifest_json, css })
}

fn emit_distribution(out: &Path, artifacts: &BuildArtifacts) -> miette::Result<()> {
    fs::create_dir_all(out).into_diagnostic()?;
    fs::write(out.join("peitho.css"), &artifacts.css).into_diagnostic()?;
    write_slide_fragments(out, &artifacts.rendered)?;
    fs::write(out.join("manifest.json"), &artifacts.manifest_json).into_diagnostic()?;
    fs::write(out.join("index.html"), peitho_core::render_distribution_index()).into_diagnostic()?;
    Ok(())
}
```

Verification:

```bash
cargo test -p peitho --test build build_still_writes_distribution_after_pipeline_refactor
cargo test -p peitho --test build repository_example_builds_three_slide_distribution
```

### Task 8 - Add present --no-serve Cache Emission

Goal: `peitho present <md> --no-serve --no-open` writes a clean `.peitho/present-cache/` with slides, manifest, `peitho.css`, `notes.json`, `present.html`, and copied `shell.js`.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
use std::{ffi::OsString, fs};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn present_no_serve_writes_clean_present_cache() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(&shell, "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\n").unwrap();
    let stale = dir.path().join(".peitho/present-cache/stale.txt");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    fs::write(&stale, "old").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(fixture.present_args(&shell))
        .args(["--no-serve", "--no-open"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".peitho/present-cache"));

    let cache = dir.path().join(".peitho/present-cache");
    assert!(!stale.exists());
    assert!(cache.join("present.html").exists());
    assert!(cache.join("shell.js").exists());
    assert!(cache.join("peitho.css").exists());
    assert!(cache.join("manifest.json").exists());
    assert!(cache.join("notes.json").exists());
    assert!(cache.join("slides/000-arch-1.html").exists());
    assert!(fs::read_to_string(cache.join("notes.json")).unwrap().contains(r#""notes": {}"#));
    assert!(fs::read_to_string(cache.join("present.html")).unwrap().contains("installSyncBridge(window)"));
}

struct Fixture {
    deck: std::path::PathBuf,
    template: std::path::PathBuf,
    base: std::path::PathBuf,
    overrides: std::path::PathBuf,
}

impl Fixture {
    fn write(root: &std::path::Path) -> Self {
        let deck = root.join("deck.md");
        let template = root.join("template.html");
        let base = root.join("base.css");
        let overrides = root.join("overrides.css");
        fs::write(&deck, "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody").unwrap();
        fs::write(&template, r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="0..*"></slot></section>"#).unwrap();
        fs::write(&base, ".slot-title { color: red; }").unwrap();
        fs::write(&overrides, r#"[data-slide-key="arch-1"] .slot-title { color: blue; }"#).unwrap();
        Self { deck, template, base, overrides }
    }

    fn present_args(&self, shell: &std::path::Path) -> Vec<OsString> {
        vec![
            OsString::from("present"),
            self.deck.as_os_str().to_owned(),
            OsString::from("--template"),
            self.template.as_os_str().to_owned(),
            OsString::from("--base-css"),
            self.base.as_os_str().to_owned(),
            OsString::from("--overrides-css"),
            self.overrides.as_os_str().to_owned(),
            OsString::from("--shell"),
            shell.as_os_str().to_owned(),
        ]
    }
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
const PRESENT_CACHE: &str = ".peitho/present-cache";

#[derive(Debug, Subcommand)]
enum Command {
    Build { /* existing fields */ },
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
}

fn present(
    input: &Path,
    template: &Path,
    base_css: &Path,
    overrides_css: &Path,
    shell: &Path,
    port: u16,
    no_open: bool,
    no_serve: bool,
) -> miette::Result<()> {
    let cache = PathBuf::from(PRESENT_CACHE);
    if cache.exists() {
        fs::remove_dir_all(&cache).into_diagnostic()?;
    }
    fs::create_dir_all(&cache).into_diagnostic()?;

    let artifacts = build_artifacts(input, template, base_css, overrides_css)?;
    emit_present_cache(&cache, &artifacts, shell)?;
    if no_serve {
        println!("generated present cache at {}", cache.display());
        return Ok(());
    }

    let server = server::PresentServer::bind(cache, port)?;
    let url = server.url();
    println!("serving presentation at {url}");
    if !no_open {
        open_browser(&url);
    }
    server.serve_forever()
}

fn emit_present_cache(cache: &Path, artifacts: &BuildArtifacts, shell: &Path) -> miette::Result<()> {
    if !shell.exists() {
        return Err(miette::miette!(
            "shell bundle not found: {}\nhelp: run `cd packages/peitho-present && npm run build` or pass --shell <path>",
            shell.display()
        ));
    }
    fs::write(cache.join("peitho.css"), &artifacts.css).into_diagnostic()?;
    write_slide_fragments(cache, &artifacts.rendered)?;
    fs::write(cache.join("manifest.json"), &artifacts.manifest_json).into_diagnostic()?;
    fs::write(cache.join("notes.json"), core(peitho_core::notes_json(&peitho_core::Notes::empty()))?).into_diagnostic()?;
    fs::write(cache.join("present.html"), peitho_core::render_present_index()).into_diagnostic()?;
    fs::copy(shell, cache.join("shell.js")).into_diagnostic()?;
    Ok(())
}
```

Verification:

```bash
cargo test -p peitho --test present present_no_serve_writes_clean_present_cache
```

### Task 9 - Error When shell.js Is Missing

Goal: missing shell bundle is a build-time CLI error with actionable help.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
#[test]
fn present_fails_with_help_when_shell_bundle_is_missing() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let missing_shell = dir.path().join("missing-shell.js");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(fixture.present_args(&missing_shell))
        .args(["--no-serve", "--no-open"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("shell bundle not found"))
        .stderr(predicate::str::contains("cd packages/peitho-present && npm run build"))
        .stderr(predicate::str::contains("--shell <path>"));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn ensure_shell_bundle(shell: &Path) -> miette::Result<()> {
    if shell.exists() {
        return Ok(());
    }
    Err(miette::miette!(
        "shell bundle not found: {}\nhelp: run `cd packages/peitho-present && npm run build` or pass --shell <path>",
        shell.display()
    ))
}
```

Call `ensure_shell_bundle(shell)?;` at the start of `emit_present_cache`.

Verification:

```bash
cargo test -p peitho --test present present_fails_with_help_when_shell_bundle_is_missing
```

### Task 10 - Implement Static Server Path Resolution and Content Types

Goal: reject traversal, map `/` to `present.html`, and provide minimal content types for html/css/js/json.

Files:

- `crates/peitho/src/server.rs`
- `crates/peitho/src/main.rs`

Test:

```rust
// crates/peitho/src/server.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn resolves_root_to_present_html() {
        assert_eq!(
            resolve_request_path(Path::new("/cache"), "/").unwrap(),
            Path::new("/cache").join("present.html")
        );
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(resolve_request_path(Path::new("/cache"), "/../manifest.json").is_none());
        assert!(resolve_request_path(Path::new("/cache"), "/slides/../../secret").is_none());
        assert!(resolve_request_path(Path::new("/cache"), "http://x/manifest.json").is_none());
    }

    #[test]
    fn maps_content_types() {
        assert_eq!(content_type(Path::new("present.html")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("peitho.css")), "text/css; charset=utf-8");
        assert_eq!(content_type(Path::new("shell.js")), "text/javascript; charset=utf-8");
        assert_eq!(content_type(Path::new("manifest.json")), "application/json; charset=utf-8");
        assert_eq!(content_type(Path::new("slide.bin")), "application/octet-stream");
    }
}
```

Implementation:

```rust
// crates/peitho/src/server.rs
use std::path::{Component, Path, PathBuf};

pub(crate) fn resolve_request_path(root: &Path, url: &str) -> Option<PathBuf> {
    let path = url.split('?').next().unwrap_or(url);
    if path.contains("://") {
        return None;
    }
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(root.join("present.html"));
    }

    let mut out = root.to_path_buf();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

pub(crate) fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}
```

```rust
// crates/peitho/src/main.rs
mod server;
```

Verification:

```bash
cargo test -p peitho server::tests::resolves_root_to_present_html
cargo test -p peitho server::tests::rejects_path_traversal
cargo test -p peitho server::tests::maps_content_types
```

### Task 11 - Serve Cache over tiny_http

Goal: serve GET requests from the cache directory, return 404/plain text for missing or rejected paths, and expose the assigned port.

Files:

- `crates/peitho/src/server.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
use std::{
    io::{Read, Write},
    net::TcpStream,
    thread,
};

#[test]
fn present_server_serves_manifest_over_http() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();
    fs::write(cache.join("manifest.json"), r#"{"version":1}"#).unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one().unwrap());
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(b"GET /manifest.json HTTP/1.0\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    handle.join().unwrap();

    assert!(response.contains("200 OK"));
    assert!(response.contains("application/json"));
    assert!(response.contains(r#"{"version":1}"#));
}
```

Implementation:

```rust
// crates/peitho/src/server.rs
use std::{fs, net::SocketAddr, path::PathBuf};

use miette::IntoDiagnostic;
use tiny_http::{Header, Method, Response, Server, StatusCode};

pub struct PresentServer {
    root: PathBuf,
    server: Server,
}

impl PresentServer {
    pub fn bind(root: PathBuf, port: u16) -> miette::Result<Self> {
        let server = Server::http(("127.0.0.1", port)).into_diagnostic()?;
        Ok(Self { root, server })
    }

    pub fn addr(&self) -> SocketAddr {
        self.server.server_addr().to_ip().expect("present server binds TCP")
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/present.html", self.addr().port())
    }

    pub fn serve_forever(self) -> miette::Result<()> {
        for request in self.server.incoming_requests() {
            self.respond(request)?;
        }
        Ok(())
    }

    pub fn handle_one(self) -> miette::Result<()> {
        if let Some(request) = self.server.incoming_requests().next() {
            self.respond(request)?;
        }
        Ok(())
    }

    fn respond(&self, request: tiny_http::Request) -> miette::Result<()> {
        if request.method() != &Method::Get {
            request.respond(Response::empty(StatusCode(405))).into_diagnostic()?;
            return Ok(());
        }
        let Some(path) = resolve_request_path(&self.root, request.url()) else {
            request.respond(Response::from_string("404\n").with_status_code(404)).into_diagnostic()?;
            return Ok(());
        };
        match fs::read(&path) {
            Ok(bytes) => {
                let header = Header::from_bytes("Content-Type", content_type(&path))
                    .map_err(|_| miette::miette!("failed to build Content-Type header"))?;
                request.respond(Response::from_data(bytes).with_header(header)).into_diagnostic()?;
            }
            Err(_) => {
                request.respond(Response::from_string("404\n").with_status_code(404)).into_diagnostic()?;
            }
        }
        Ok(())
    }
}
```

To make the integration test import the server module:

```rust
// crates/peitho/src/lib.rs
pub mod server;
```

and keep the binary using the library module:

```rust
// crates/peitho/src/main.rs
// replace the Task 10 private `mod server;` with this import
use peitho::server;
```

Verification:

```bash
cargo test -p peitho --test present present_server_serves_manifest_over_http
```

### Task 12 - Start Server from present and Print URL

Goal: `peitho present` without `--no-serve` binds to `127.0.0.1`, prints the actual URL, and serves `present.html`.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
use std::{
    io::{BufRead, BufReader},
    sync::mpsc,
    time::Duration,
};

#[test]
fn present_no_open_server_prints_assigned_url() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(&shell, "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\n").unwrap();

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("peitho"))
        .current_dir(dir.path())
        .args(fixture.present_args(&shell))
        .args(["--no-open", "--port", "0"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.contains("serving presentation at") {
                tx.send(line).unwrap();
                break;
            }
        }
    });
    let line = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("present server did not print serving URL within 5 seconds");
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();

    assert!(line.contains("http://127.0.0.1:"));
    assert!(line.contains("/present.html"));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
let server = server::PresentServer::bind(cache, port)?;
let url = server.url();
println!("serving presentation at {url}");
if !no_open {
    open_browser(&url);
}
server.serve_forever()
```

Verification:

```bash
cargo test -p peitho --test present present_no_open_server_prints_assigned_url
```

### Task 13 - Browser Open Is Best Effort

Goal: macOS uses `open`, Linux uses `xdg-open`, unsupported platforms and command failures warn but do not fail `present`.

Files:

- `crates/peitho/src/main.rs`

Test:

```rust
// crates/peitho/src/main.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_command_matches_supported_platforms() {
        let command = browser_command();
        if cfg!(target_os = "macos") {
            assert_eq!(command, Some("open"));
        } else if cfg!(target_os = "linux") {
            assert_eq!(command, Some("xdg-open"));
        } else {
            assert_eq!(command, None);
        }
    }
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn browser_command() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("open")
    } else if cfg!(target_os = "linux") {
        Some("xdg-open")
    } else {
        None
    }
}

fn open_browser(url: &str) {
    let Some(command) = browser_command() else {
        eprintln!("warning: browser auto-open is not supported on this platform");
        return;
    };
    if let Err(err) = std::process::Command::new(command).arg(url).spawn() {
        eprintln!("warning: failed to open browser with {command}: {err}");
    }
}
```

Verification:

```bash
cargo test -p peitho browser_command_matches_supported_platforms
```

### Task 14 - Repository present Smoke with Built Shell

Goal: repository example can generate a present cache using the real bundled shell.

Files:

- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
#[test]
fn repository_example_present_no_serve_uses_bundled_shell() {
    let shell = workspace_root().join("packages/peitho-present/dist/shell.js");
    assert!(
        shell.exists(),
        "shell bundle not built; run npm run build"
    );

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "present",
            "examples/deck.md",
            "--template",
            "templates/title-body-code.html",
            "--base-css",
            "themes/base.css",
            "--overrides-css",
            "themes/overrides.css",
            "--no-serve",
            "--no-open",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated present cache"));

    let cache = workspace_root().join(".peitho/present-cache");
    assert!(cache.join("present.html").exists());
    assert!(cache.join("shell.js").exists());
    assert!(fs::read_to_string(cache.join("manifest.json")).unwrap().contains(r#""slideCount": 3"#));
}
```

Implementation:

```rust
// crates/peitho/tests/present.rs
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}
```

Verification:

```bash
test -f packages/peitho-present/dist/shell.js || (echo "shell bundle not built; run npm run build" && false)
cargo test -p peitho --test present repository_example_present_no_serve_uses_bundled_shell
```

### Task 15 - Build shell.js Before Rust Tests in CI

Goal: keep `cargo test --workspace` in CI valid after Rust tests require `packages/peitho-present/dist/shell.js`.

Files:

- `.github/workflows/ci.yml`

Test:

```bash
sed -n '/  test:/,/  lint:/p' .github/workflows/ci.yml | rg -n "actions/setup-node@v4"
sed -n '/  test:/,/  lint:/p' .github/workflows/ci.yml | rg -n "working-directory: packages/peitho-present"
sed -n '/  test:/,/  lint:/p' .github/workflows/ci.yml | rg -n "npm run build"
```

Expected Red before implementation:

```text
the test job does not build packages/peitho-present/dist/shell.js before cargo test
```

Implementation:

```yaml
# .github/workflows/ci.yml
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: packages/peitho-present/package-lock.json
      - run: npm ci
        working-directory: packages/peitho-present
      - run: npm run build
        working-directory: packages/peitho-present
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace
      - run: git diff --exit-code bindings/
```

The existing `node` job remains unchanged; it still runs `npm ci`, `npm run build`, `npm test`, and `npm run typecheck`.

Verification:

```bash
sed -n '/  test:/,/  lint:/p' .github/workflows/ci.yml | rg -n "actions/setup-node@v4"
sed -n '/  test:/,/  lint:/p' .github/workflows/ci.yml | rg -n "working-directory: packages/peitho-present"
sed -n '/  test:/,/  lint:/p' .github/workflows/ci.yml | rg -n "npm run build"
```

## Final Verification

After Task 15 is green, run the full acceptance command set:

```bash
cd packages/peitho-present
npm ci
npm run build
npm test
npm run typecheck
cd ../..
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p peitho -- present examples/deck.md \
  --template templates/title-body-code.html \
  --base-css themes/base.css \
  --overrides-css themes/overrides.css \
  --no-serve \
  --no-open
test -f .peitho/present-cache/present.html
test -f .peitho/present-cache/shell.js
test -f .peitho/present-cache/notes.json
test -f .peitho/present-cache/slides/000-arch-1.html
rg -n "installSyncBridge\\(window\\)|notes.json" .peitho/present-cache/present.html
rg -n "installSyncBridge|peitho-sync" .peitho/present-cache/shell.js
```

## Summary

全15タスクで、まず `tiny_http` と揮発cache ignoreを追加し、`notes_json` と `present.html` をRust coreから生成できるようにする。次にTS側へ `installSyncBridge` を追加し、同期の送受信とno-opによるエコー停止をテストする。その後CLIのbuild pipelineを再利用可能に切り出し、`peitho present` が毎回 `.peitho/present-cache/` を作り直してslides/manifest/CSS/notes/present.html/shell.jsをemitする。最後にtiny_httpサーバ、パストラバーサル防止、URL表示、best-effortブラウザ起動、実shellを使ったrepository smoke、CIのRust test前shell buildを通して、発表用の揮発エントリを完成させる。
