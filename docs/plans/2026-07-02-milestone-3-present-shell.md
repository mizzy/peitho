# Peitho Milestone 3 Present Shell Plan

## Purpose

Milestone 3 connects the Rust contract source of truth to TypeScript and builds the in-window presentation shell core. The build CLI still does not gain a `present` command. This milestone produces committed generated TS bindings, a `packages/peitho-present` browser bundle, and a tested shell that loads `manifest.json` plus slide fragments through `fetch`, places each fragment inside a Shadow root, exposes shell-owned host handles, and owns all navigation state through DOM events.

Out of scope: `peitho present`, `present.html` generation, browser launch, BroadcastChannel synchronization, presenter view, notes Markdown syntax, `publish`, `--watch`, and writing `notes.json` from build output.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `Cargo.toml` | workspace dependency pin for `ts-rs = 12.0.1` | crates.io |
| `crates/peitho-core/Cargo.toml` | `ts-bindings` feature and test-time `ts-rs` dependency | workspace deps |
| `crates/peitho-core/src/domain.rs` | `SlideKey` TS string representation for exported fields | `ts-rs` cfg attrs |
| `crates/peitho-core/src/manifest.rs` | `Manifest` / `ManifestSlide` serde + TS export | `domain.rs` |
| `crates/peitho-core/src/notes.rs` | `Notes` schema only; no build emission | `domain.rs` |
| `crates/peitho-core/src/lib.rs` | export `Notes` and keep manifest exports | core modules |
| `bindings/Manifest.ts` | generated TS type committed to git | `cargo test -p peitho-core` |
| `bindings/ManifestSlide.ts` | generated TS type committed to git | `cargo test -p peitho-core` |
| `bindings/Notes.ts` | generated TS type committed to git | `cargo test -p peitho-core` |
| `packages/peitho-present/package.json` | npm package scripts and exact dev dependency versions | npm |
| `packages/peitho-present/package-lock.json` | committed lockfile used by `npm ci` | package.json |
| `packages/peitho-present/tsconfig.json` | strict TS config including root `bindings/` | generated bindings |
| `packages/peitho-present/esbuild.config.mjs` | bundles `src/index.ts` to `dist/shell.js` | esbuild |
| `packages/peitho-present/vitest.config.ts` | jsdom test environment | vitest |
| `packages/peitho-present/src/index.ts` | shell entry and public API | generated bindings |
| `packages/peitho-present/src/shell.ts` | fetch, Shadow DOM, host handles, navigation state | generated bindings |
| `packages/peitho-present/src/keyboard.ts` | keyboard UI adapter that emits `peitho:navigate` | DOM events |
| `packages/peitho-present/test/shell.test.ts` | jsdom shell tests with mocked fetch | shell.ts |
| `.github/workflows/ci.yml` | Rust bindings drift check plus Node build/test/typecheck job | Rust + npm |
| `.gitignore` | ignores `node_modules` and TS bundle output, not `bindings/` | repo root |

Dependency direction: Rust `peitho-core` owns schemas. TypeScript imports generated `bindings/*.ts` only as types. The shell reads `manifest.json` and slide fragment files at runtime and never embeds slide bodies into its bundle.

## Implementation Tasks

### Task 1 - Wire ts-rs as Test-Time Contract Generator

Goal: add `ts-rs` 12.0.1 with `serde-compat` enabled by default and a `ts-bindings` feature for explicit non-test exports, while keeping normal core builds free of mandatory TS generation.

Files:

- `Cargo.toml`
- `crates/peitho-core/Cargo.toml`

Test:

```bash
cargo metadata --format-version 1 --no-deps | rg '"ts-rs"'
```

Expected Red before implementation:

```text
no matches found for "ts-rs"
```

Implementation:

```toml
# Cargo.toml
[workspace.dependencies]
ts-rs = { version = "12.0.1", default-features = true }
```

```toml
# crates/peitho-core/Cargo.toml
[features]
ts-bindings = ["dep:ts-rs"]

[dependencies]
ts-rs = { workspace = true, optional = true }

[dev-dependencies]
ts-rs.workspace = true
```

Version note: before implementing, verify `ts-rs 12.0.1` exists and is a current stable release with `cargo info ts-rs@12.0.1`; if it does not exist, pin the latest stable version found in crates.io and report the adjustment. `ts-rs 12.0.1` default features include `serde-compat`, so serde field renames such as `peithoVersion` and `slideCount` are mirrored in generated TypeScript. `#[ts(export_to = "../../bindings/")]` relies on ts-rs default `CARGO_MANIFEST_DIR`-relative export path resolution; introduce `.cargo/config.toml` only if implementation proves that default cannot write root `bindings/`, and report it as a plan deviation.

Verification:

```bash
cargo metadata --format-version 1 --no-deps | rg '"ts-rs"'
cargo test -p peitho-core --no-run
```

### Task 2 - Export Manifest and ManifestSlide Bindings

Goal: derive TS for `Manifest` and `ManifestSlide`, generate committed bindings, and ensure serde-renamed field names match TypeScript names.

Files:

- `crates/peitho-core/src/manifest.rs`
- `crates/peitho-core/src/domain.rs`
- `bindings/Manifest.ts`
- `bindings/ManifestSlide.ts`

Test:

```rust
// crates/peitho-core/src/manifest.rs
#[cfg(test)]
mod ts_tests {
    use std::{fs, path::Path};
    use ts_rs::TS;

    use super::{Manifest, ManifestSlide};

    #[test]
    fn exports_manifest_bindings_with_serde_field_names() {
        Manifest::export_all().unwrap();
        ManifestSlide::export_all().unwrap();

        let root_bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
        let manifest = fs::read_to_string(root_bindings.join("Manifest.ts")).unwrap();
        let slide = fs::read_to_string(root_bindings.join("ManifestSlide.ts")).unwrap();

        assert!(manifest.contains("peithoVersion: string"));
        assert!(manifest.contains("slideCount: number"));
        assert!(manifest.contains("slides: Array<ManifestSlide>"));
        assert!(slide.contains("key: string"));
        assert!(slide.contains("hasNotes: boolean"));
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/domain.rs
#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "string"))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SlideKey(String);
```

```rust
// crates/peitho-core/src/manifest.rs
#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(any(test, feature = "ts-bindings"), ts(export, export_to = "../../bindings/"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
#[cfg_attr(any(test, feature = "ts-bindings"), ts(export, export_to = "../../bindings/"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestSlide {
    index: usize,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "string"))]
    key: SlideKey,
    src: String,
    #[serde(rename = "hasNotes")]
    has_notes: bool,
}
```

Expected generated files:

```ts
// bindings/ManifestSlide.ts
export type ManifestSlide = {
  index: number,
  key: string,
  src: string,
  hasNotes: boolean,
};
```

```ts
// bindings/Manifest.ts
import type { ManifestSlide } from "./ManifestSlide";

export type Manifest = {
  version: number,
  peithoVersion: string,
  title: string,
  slideCount: number,
  slides: Array<ManifestSlide>,
};
```

Verification:

```bash
cargo test -p peitho-core manifest::ts_tests::exports_manifest_bindings_with_serde_field_names
test -f bindings/Manifest.ts
test -f bindings/ManifestSlide.ts
```

### Task 3 - Add Notes Schema Type and Binding Only

Goal: define the future `notes.json` schema and export its TS type without writing `notes.json` during build.

Files:

- `crates/peitho-core/src/notes.rs`
- `crates/peitho-core/src/lib.rs`
- `bindings/Notes.ts`

Test:

```rust
// crates/peitho-core/src/notes.rs
#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};
    use ts_rs::TS;

    use crate::domain::SlideKey;
    use super::Notes;

    #[test]
    fn serializes_empty_notes_schema() {
        let notes = Notes::empty();
        let json = serde_json::to_string_pretty(&notes).unwrap();

        assert!(json.contains(r#""version": 1"#));
        assert!(json.contains(r#""notes": {}"#));
    }

    #[test]
    fn exports_notes_binding_as_keyed_record() {
        Notes::export_all().unwrap();

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings/Notes.ts");
        let ts = fs::read_to_string(path).unwrap();
        assert!(ts.contains("notes: Record<string, string>"));
    }

    #[test]
    fn notes_are_keyed_by_slide_key_not_index() {
        let mut map = BTreeMap::new();
        map.insert(SlideKey::new("arch-1").unwrap(), "speaker note".to_owned());
        let notes = Notes::new(map);
        let json = serde_json::to_string(&notes).unwrap();

        assert!(json.contains(r#""arch-1":"speaker note""#));
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/notes.rs
use std::collections::BTreeMap;

use serde::Serialize;

use crate::domain::SlideKey;

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(any(test, feature = "ts-bindings"), ts(export, export_to = "../../bindings/"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notes {
    version: u8,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "Record<string, string>"))]
    notes: BTreeMap<SlideKey, String>,
}

impl Notes {
    pub fn empty() -> Self {
        Self { version: 1, notes: BTreeMap::new() }
    }

    pub fn new(notes: BTreeMap<SlideKey, String>) -> Self {
        Self { version: 1, notes }
    }
}
```

```rust
// crates/peitho-core/src/lib.rs
pub mod notes;
pub use notes::Notes;
```

Expected generated file:

```ts
// bindings/Notes.ts
export type Notes = {
  version: number,
  notes: Record<string, string>,
};
```

Verification:

```bash
cargo test -p peitho-core notes::tests::serializes_empty_notes_schema
cargo test -p peitho-core notes::tests::exports_notes_binding_as_keyed_record
cargo test -p peitho-core notes::tests::notes_are_keyed_by_slide_key_not_index
test ! -f dist/notes.json
```

### Task 4 - Commit Bindings and Add Rust Drift Check

Goal: make generated bindings part of the repository contract and fail CI if `cargo test` changes them without committing the diff.

Files:

- `.github/workflows/ci.yml`
- `bindings/Manifest.ts`
- `bindings/ManifestSlide.ts`
- `bindings/Notes.ts`

Test:

```bash
cargo test -p peitho-core manifest::ts_tests::exports_manifest_bindings_with_serde_field_names
cargo test -p peitho-core notes::tests::exports_notes_binding_as_keyed_record
git diff --exit-code bindings/
```

Implementation:

```yaml
# .github/workflows/ci.yml
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace
      - run: git diff --exit-code bindings/
```

Verification:

```bash
cargo test --workspace
git diff --exit-code bindings/
```

### Task 5 - Add Node Ignore Rules and TS Package Scaffold

Goal: create `packages/peitho-present` with exact npm scripts and committed lockfile; ignore local installs and bundle output.

Files:

- `.gitignore`
- `packages/peitho-present/package.json`
- `packages/peitho-present/package-lock.json`
- `packages/peitho-present/tsconfig.json`
- `packages/peitho-present/esbuild.config.mjs`
- `packages/peitho-present/vitest.config.ts`
- `packages/peitho-present/src/index.ts`

Test:

```bash
npm view typescript@6.0.3 version
npm view vitest@4.1.9 version
npm view jsdom@29.1.1 version
npm view esbuild@0.28.1 version
npm view @types/jsdom@28.0.3 version
test -f packages/peitho-present/package.json
cd packages/peitho-present
npm ci
npm run typecheck
```

Implementation:

Version note: before writing `package-lock.json`, verify all pinned versions above exist in the npm registry and are stable releases. If any pinned version does not exist, choose the latest stable version returned by `npm view <package> version`, update `package.json`, regenerate the lockfile, and report the adjustment. Keep `types` limited to ambient packages that TypeScript can resolve; if `@types/jsdom` does not provide a usable `types: ["jsdom"]` entry, do not list `jsdom` in `types` and rely on `"lib": ["DOM"]` plus Vitest's jsdom environment.

```gitignore
# .gitignore
/target
/dist
node_modules/
packages/peitho-present/node_modules/
packages/peitho-present/dist/
```

```json
// packages/peitho-present/package.json
{
  "name": "@peitho/present",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "build": "node esbuild.config.mjs",
    "test": "vitest run",
    "typecheck": "tsc --noEmit"
  },
  "devDependencies": {
    "@types/jsdom": "28.0.3",
    "esbuild": "0.28.1",
    "jsdom": "29.1.1",
    "typescript": "6.0.3",
    "vitest": "4.1.9"
  }
}
```

```json
// packages/peitho-present/tsconfig.json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2022", "DOM"],
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["vitest/globals"]
  },
  "include": ["src/**/*.ts", "test/**/*.ts", "../../bindings/**/*.ts"]
}
```

```js
// packages/peitho-present/esbuild.config.mjs
import { build } from "esbuild";

await build({
  entryPoints: ["src/index.ts"],
  outfile: "dist/shell.js",
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  sourcemap: true
});
```

```ts
// packages/peitho-present/vitest.config.ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    globals: true
  }
});
```

```ts
// packages/peitho-present/src/index.ts
export { mountPresentShell } from "./shell";
export type {
  NavigateDetail,
  NavigateTarget,
  PresentShell,
  ShellOptions,
  SlideChangeDetail
} from "./shell";
```

Lockfile command:

```bash
cd packages/peitho-present
npm install --package-lock-only
```

Verification:

```bash
cd packages/peitho-present
npm ci
npm run typecheck
```

### Task 6 - Import Generated Manifest Type in TS

Goal: prove `peitho-present` consumes Rust-generated TS contracts instead of hand-written manifest types.

Files:

- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/shell.test.ts`

Test:

```ts
// packages/peitho-present/test/shell.test.ts
import { describe, expect, it } from "vitest";
import type { Manifest } from "../../../bindings/Manifest";

describe("generated manifest contract", () => {
  it("uses the Rust-generated Manifest type shape", () => {
    const manifest: Manifest = {
      version: 1,
      peithoVersion: "0.1.0",
      title: "Demo",
      slideCount: 1,
      slides: [{ index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false }]
    };

    expect(manifest.slides[0].key).toBe("intro");
  });
});
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
import type { Manifest } from "../../../bindings/Manifest";
import type { ManifestSlide } from "../../../bindings/ManifestSlide";

export type NavigateTarget =
  | "next"
  | "prev"
  | "first"
  | "last"
  | { key: string }
  | { index: number };

export type NavigateDetail = { to: NavigateTarget };

export type SlideChangeDetail = {
  key: string;
  index: number;
  total: number;
  previousIndex: number | null;
};

export type PresentShell = {
  manifest: Manifest | null;
  currentIndex: number;
  navigate(to: NavigateTarget): void;
  destroy(): void;
};

export type ShellOptions = {
  root: HTMLElement;
  baseUrl?: string;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  console?: Pick<Console, "error">;
};

type SlideView = {
  meta: ManifestSlide;
  host: HTMLElement;
};
```

Verification:

```bash
cd packages/peitho-present
npm run typecheck
npm test -- generated
```

### Task 7 - Load Manifest and Slide Fragments into Shadow Hosts

Goal: fetch `manifest.json`, fetch each slide fragment, create one Shadow DOM host per slide, and place fragment HTML inside the shadow root.

Files:

- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/shell.test.ts`

Test:

```ts
// packages/peitho-present/test/shell.test.ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { mountPresentShell } from "../src/index";
import type { PresentShell } from "../src/index";

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

const mountedShells: PresentShell[] = [];
const windowListenerCleanups: Array<() => void> = [];

afterEach(() => {
  while (mountedShells.length > 0) {
    mountedShells.pop()?.destroy();
  }
  while (windowListenerCleanups.length > 0) {
    windowListenerCleanups.pop()?.();
  }
});

async function mountForTest(options: Parameters<typeof mountPresentShell>[0]): Promise<PresentShell> {
  const shell = await mountPresentShell(options);
  mountedShells.push(shell);
  return shell;
}

function listenWindow(type: string, listener: EventListener): void {
  window.addEventListener(type, listener);
  windowListenerCleanups.push(() => window.removeEventListener(type, listener));
}

function standardFetch(): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-arch-1.html") return okText("<section><pre>code</pre></section>");
    return {
      ok: false,
      status: 404,
      text: async () => "not found",
      json: async () => ({})
    } as Response;
  }) as unknown as typeof fetch;
}

it("loads manifest and fragments into shadow roots", async () => {
  const root = document.createElement("main");
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-arch-1.html") return okText("<section><pre>code</pre></section>");
    throw new Error(`unexpected ${url}`);
  });

  await mountForTest({ root, fetcher: fetcher as unknown as typeof fetch });

  const hosts = root.querySelectorAll<HTMLElement>(".peitho-slide");
  expect(hosts).toHaveLength(2);
  expect(hosts[0].shadowRoot?.innerHTML).toContain("<h1>Intro</h1>");
  expect(hosts[1].shadowRoot?.innerHTML).toContain("<pre>code</pre>");
});
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
export async function mountPresentShell(options: ShellOptions): Promise<PresentShell> {
  const shell = new PresentShellController(options);
  await shell.load();
  return shell;
}

class PresentShellController implements PresentShell {
  manifest: Manifest | null = null;
  currentIndex = -1;
  private readonly slides: SlideView[] = [];
  private readonly root: HTMLElement;
  private readonly fetcher: typeof fetch;
  private readonly win: Window;
  private readonly doc: Document;
  private readonly log: Pick<Console, "error">;

  constructor(options: ShellOptions) {
    this.root = options.root;
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.log = options.console ?? console;
  }

  async load(): Promise<void> {
    try {
      const manifest = await this.fetchJson<Manifest>("manifest.json");
      this.manifest = manifest;
      for (const slide of manifest.slides) {
        const html = await this.fetchText(slide.src);
        const host = this.doc.createElement("section");
        host.classList.add("peitho-slide");
        host.dataset.slideKey = slide.key;
        host.dataset.slideIndex = String(slide.index);
        const shadow = host.attachShadow({ mode: "open" });
        shadow.innerHTML = html;
        this.root.appendChild(host);
        this.slides.push({ meta: slide, host });
      }
      this.show(0);
    } catch (error) {
      this.root.textContent = error instanceof Error ? error.message : String(error);
    }
  }

  private async fetchJson<T>(url: string): Promise<T> {
    const response = await this.fetchOk(url);
    return response.json() as Promise<T>;
  }

  private async fetchText(url: string): Promise<string> {
    const response = await this.fetchOk(url);
    return response.text();
  }

  private async fetchOk(url: string): Promise<Response> {
    const response = await this.fetcher(url);
    if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
    return response;
  }

  navigate(to: NavigateTarget): void {
    this.show(this.resolveTarget(to));
  }

  destroy(): void {}

  private resolveTarget(to: NavigateTarget): number {
    if (to === "first") return 0;
    if (to === "last") return this.slides.length - 1;
    if (to === "next") return Math.min(this.currentIndex + 1, this.slides.length - 1);
    if (to === "prev") return Math.max(this.currentIndex - 1, 0);
    if ("index" in to) return to.index;
    return this.slides.findIndex((slide) => slide.meta.key === to.key);
  }

  private show(index: number): void {
    if (index < 0 || index >= this.slides.length) {
      this.log.error(`Unknown slide target: ${index}`);
      return;
    }
    this.slides.forEach((slide, slideIndex) => {
      slide.host.hidden = slideIndex !== index;
    });
    const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
    this.currentIndex = index;
    const slide = this.slides[index];
    this.win.dispatchEvent(new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: {
        key: slide.meta.key,
        index: slide.meta.index,
        total: this.slides.length,
        previousIndex
      }
    }));
  }
}
```

Verification:

```bash
cd packages/peitho-present
npm test -- loads
```

### Task 8 - Expose Host Handles and Show One Slide

Goal: host elements expose `data-slide-key`, `data-slide-index`, and class `peitho-slide`; only the current slide is visible.

Files:

- `packages/peitho-present/test/shell.test.ts`
- `packages/peitho-present/src/shell.ts`

Test:

```ts
// packages/peitho-present/test/shell.test.ts
it("puts shell handles on hosts and hides non-current slides", async () => {
  const root = document.createElement("main");
  await mountForTest({ root, fetcher: standardFetch() });

  const hosts = [...root.querySelectorAll<HTMLElement>(".peitho-slide")];
  expect(hosts[0].dataset.slideKey).toBe("intro");
  expect(hosts[0].dataset.slideIndex).toBe("0");
  expect(hosts[0].hidden).toBe(false);
  expect(hosts[1].hidden).toBe(true);
});
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
host.classList.add("peitho-slide");
host.dataset.slideKey = slide.key;
host.dataset.slideIndex = String(slide.index);

this.slides.forEach((slide, slideIndex) => {
  slide.host.hidden = slideIndex !== index;
});
```

Verification:

```bash
cd packages/peitho-present
npm test -- handles
```

### Task 9 - Handle peitho:navigate Events for Relative and Absolute Moves

Goal: shell listens to `peitho:navigate` and performs `next`, `prev`, `first`, and `last`; callers do not call shell methods directly.

Files:

- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/shell.test.ts`

Test:

```ts
// packages/peitho-present/test/shell.test.ts
it("navigates from DOM events for next prev first and last", async () => {
  const root = document.createElement("main");
  const shell = await mountForTest({ root, fetcher: standardFetch(), window });
  const changes: Array<{ index: number }> = [];
  listenWindow("peitho:slidechange", (event) => {
    changes.push((event as CustomEvent).detail);
  });

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(shell.currentIndex).toBe(1);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
  expect(shell.currentIndex).toBe(0);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "last" } }));
  expect(shell.currentIndex).toBe(1);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "first" } }));
  expect(shell.currentIndex).toBe(0);
  expect(changes.map((change) => change.index)).toEqual([1, 0, 1, 0]);
});
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
private readonly onNavigate = (event: Event): void => {
  const detail = (event as CustomEvent<NavigateDetail>).detail;
  if (!detail || !("to" in detail)) {
    this.log.error("Invalid peitho:navigate event");
    return;
  }
  this.navigate(detail.to);
};

constructor(options: ShellOptions) {
  // existing assignments
  this.win.addEventListener("peitho:navigate", this.onNavigate);
}

destroy(): void {
  this.win.removeEventListener("peitho:navigate", this.onNavigate);
}
```

Verification:

```bash
cd packages/peitho-present
npm test -- navigates
```

### Task 10 - Navigate by Key or Index and Report Invalid Targets

Goal: `{ key }` and `{ index }` targets work, while unknown key/index logs `console.error` and leaves state unchanged.

Files:

- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/shell.test.ts`

Test:

```ts
// packages/peitho-present/test/shell.test.ts
it("navigates by key or index and reports invalid targets", async () => {
  const root = document.createElement("main");
  const error = vi.fn();
  const shell = await mountForTest({
    root,
    fetcher: standardFetch(),
    console: { error },
    window
  });

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { key: "arch-1" } } }));
  expect(shell.currentIndex).toBe(1);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 0 } } }));
  expect(shell.currentIndex).toBe(0);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { key: "missing" } } }));
  expect(shell.currentIndex).toBe(0);
  expect(error).toHaveBeenCalledWith("Unknown slide key: missing");
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 99 } } }));
  expect(shell.currentIndex).toBe(0);
  expect(error).toHaveBeenCalledWith("Unknown slide index: 99");
});
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
private resolveTarget(to: NavigateTarget): number | null {
  if (to === "first") return 0;
  if (to === "last") return this.slides.length - 1;
  if (to === "next") return Math.min(this.currentIndex + 1, this.slides.length - 1);
  if (to === "prev") return Math.max(this.currentIndex - 1, 0);
  if ("index" in to) {
    if (to.index < 0 || to.index >= this.slides.length) {
      this.log.error(`Unknown slide index: ${to.index}`);
      return null;
    }
    return to.index;
  }
  const index = this.slides.findIndex((slide) => slide.meta.key === to.key);
  if (index < 0) {
    this.log.error(`Unknown slide key: ${to.key}`);
    return null;
  }
  return index;
}

navigate(to: NavigateTarget): void {
  const index = this.resolveTarget(to);
  if (index === null) return;
  this.show(index);
}
```

Verification:

```bash
cd packages/peitho-present
npm test -- invalid
```

### Task 11 - Emit slidechange Payload with Previous Index

Goal: every successful transition broadcasts `{ key, index, total, previousIndex }` after visibility changes.

Files:

- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/shell.test.ts`

Test:

```ts
// packages/peitho-present/test/shell.test.ts
it("emits slidechange with previousIndex after navigation", async () => {
  const root = document.createElement("main");
  const changes: unknown[] = [];
  listenWindow("peitho:slidechange", (event) => {
    changes.push((event as CustomEvent).detail);
  });

  await mountForTest({ root, fetcher: standardFetch(), window });
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(changes).toEqual([
    { key: "intro", index: 0, total: 2, previousIndex: null },
    { key: "arch-1", index: 1, total: 2, previousIndex: 0 }
  ]);
});

it("does not emit slidechange when next at the last slide is a no-op", async () => {
  const root = document.createElement("main");
  const changes: Array<{ index: number }> = [];
  listenWindow("peitho:slidechange", (event) => {
    changes.push((event as CustomEvent).detail);
  });

  const shell = await mountForTest({ root, fetcher: standardFetch(), window });
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "last" } }));
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(shell.currentIndex).toBe(1);
  expect(changes.map((change) => change.index)).toEqual([0, 1]);
});
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
private show(index: number): void {
  if (index < 0 || index >= this.slides.length) {
    this.log.error(`Unknown slide target: ${index}`);
    return;
  }
  if (index === this.currentIndex) return;

  const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
  this.slides.forEach((slide, slideIndex) => {
    slide.host.hidden = slideIndex !== index;
  });
  this.currentIndex = index;
  const slide = this.slides[index];
  this.win.dispatchEvent(new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
    detail: {
      key: slide.meta.key,
      index: slide.meta.index,
      total: this.slides.length,
      previousIndex
    }
  }));
}
```

Verification:

```bash
cd packages/peitho-present
npm test -- previousIndex
```

### Task 12 - Implement Keyboard as Navigate Event Emitter

Goal: keyboard handling is a UI adapter that emits `peitho:navigate`; it never calls shell internals.

Files:

- `packages/peitho-present/src/keyboard.ts`
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/shell.test.ts`

Test:

```ts
// packages/peitho-present/test/shell.test.ts
import { installKeyboardNavigation } from "../src/index";

it("keyboard emits navigate events instead of calling shell directly", () => {
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });

  const teardown = installKeyboardNavigation(window);
  windowListenerCleanups.push(teardown);
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: " " }));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft" }));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Home" }));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "End" }));

  expect(requests).toEqual([
    { to: "next" },
    { to: "next" },
    { to: "prev" },
    { to: "first" },
    { to: "last" }
  ]);
});
```

Implementation:

```ts
// packages/peitho-present/src/keyboard.ts
import type { NavigateTarget } from "./shell";

const keyMap = new Map<string, NavigateTarget>([
  ["ArrowRight", "next"],
  ["PageDown", "next"],
  [" ", "next"],
  ["ArrowLeft", "prev"],
  ["PageUp", "prev"],
  ["Home", "first"],
  ["End", "last"]
]);

export function installKeyboardNavigation(win: Window = window): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    const to = keyMap.get(event.key);
    if (!to) return;
    event.preventDefault();
    win.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
```

```ts
// packages/peitho-present/src/index.ts
export { installKeyboardNavigation } from "./keyboard";
export { mountPresentShell } from "./shell";
```

Verification:

```bash
cd packages/peitho-present
npm test -- keyboard
```

### Task 13 - Display Fetch Failures and Stop Loading

Goal: manifest or fragment fetch failure writes visible text into the root and does not inject partial slide HTML after the failed fetch.

Files:

- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/shell.test.ts`

Test:

```ts
// packages/peitho-present/test/shell.test.ts
it("shows a visible error when a fragment fetch fails", async () => {
  const root = document.createElement("main");
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "slides/000-intro.html") return okText("<section>Intro</section>");
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  });

  await mountForTest({ root, fetcher: fetcher as unknown as typeof fetch });

  expect(root.textContent).toContain("Failed to load slides/001-arch-1.html: 404");
  expect(root.querySelectorAll(".peitho-slide")).toHaveLength(0);
});

it("shows a visible error when manifest fetch fails", async () => {
  const root = document.createElement("main");
  const fetcher = vi.fn(async () => ({ ok: false, status: 500, json: async () => ({}) }) as Response);

  await mountForTest({ root, fetcher: fetcher as unknown as typeof fetch });

  expect(root.textContent).toContain("Failed to load manifest.json: 500");
  expect(root.querySelectorAll(".peitho-slide")).toHaveLength(0);
});
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
async load(): Promise<void> {
  try {
    const manifest = await this.fetchJson<Manifest>("manifest.json");
    const pending: SlideView[] = [];
    for (const slide of manifest.slides) {
      const html = await this.fetchText(slide.src);
      const host = this.createSlideHost(slide, html);
      pending.push({ meta: slide, host });
    }
    this.manifest = manifest;
    for (const view of pending) {
      this.root.appendChild(view.host);
      this.slides.push(view);
    }
    this.show(0);
  } catch (error) {
    this.root.replaceChildren();
    this.root.textContent = error instanceof Error ? error.message : String(error);
  }
}

private createSlideHost(slide: ManifestSlide, html: string): HTMLElement {
  const host = this.doc.createElement("section");
  host.classList.add("peitho-slide");
  host.dataset.slideKey = slide.key;
  host.dataset.slideIndex = String(slide.index);
  host.attachShadow({ mode: "open" }).innerHTML = html;
  return host;
}
```

Verification:

```bash
cd packages/peitho-present
npm test -- fetch
```

### Task 14 - Bundle shell.js with esbuild

Goal: create a browser ESM bundle at `packages/peitho-present/dist/shell.js` without embedding slide bodies.

Files:

- `packages/peitho-present/esbuild.config.mjs`
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/package.json`

Test:

```bash
cd packages/peitho-present
npm run build
test -f dist/shell.js
rg -n "peitho:navigate|peitho:slidechange|attachShadow|fetchOk" dist/shell.js
! rg -n "Peitho Architecture|data-slide-key=\\\"arch-1\\\"" dist/shell.js
```

Implementation:

```js
// packages/peitho-present/esbuild.config.mjs
import { build } from "esbuild";

await build({
  entryPoints: ["src/index.ts"],
  outfile: "dist/shell.js",
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  sourcemap: true,
  logLevel: "info"
});
```

Verification:

```bash
cd packages/peitho-present
npm run build
test -f dist/shell.js
```

### Task 15 - Add Node CI and Binding Drift CI

Goal: CI verifies Rust, generated bindings, and TS shell build/test/typecheck.

Files:

- `.github/workflows/ci.yml`

Test:

```bash
cargo test --workspace
git diff --exit-code bindings/
cd packages/peitho-present
npm ci
npm run build
npm test
npm run typecheck
```

Implementation:

```yaml
# .github/workflows/ci.yml
name: CI

on:
  pull_request:
  push:
    branches:
      - main

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace
      - run: git diff --exit-code bindings/

  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo fmt --all --check

  node:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: packages/peitho-present
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: packages/peitho-present/package-lock.json
      - run: npm ci
      - run: npm run build
      - run: npm test
      - run: npm run typecheck
```

Verification:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cd packages/peitho-present
npm ci
npm run build
npm test
npm run typecheck
```

## Final Verification

After Task 15 is green, run the full acceptance command set:

```bash
cargo test --workspace
cargo test -p peitho-core --features ts-bindings
git diff --exit-code bindings/
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cd packages/peitho-present
npm ci
npm run build
npm test
npm run typecheck
test -f dist/shell.js
rg -n "attachShadow|peitho:navigate|peitho:slidechange|fetchOk" dist/shell.js
```

## Summary

Across all 15 tasks, we first wire `ts-rs` into `peitho-core` and generate committed `bindings/*.ts` from `Manifest` / `ManifestSlide` / `Notes`. Next, we newly set up `packages/peitho-present` with npm+TypeScript+esbuild+vitest and implement a presentation shell that imports the generated types. The shell fetches the manifest and fragments, exposes handles on Shadow DOM hosts, handles navigate/slidechange purely through DOM events, and surfaces fetch failures or invalid navigate targets either visibly or via console error. Finally, we add a bindings drift check and a Node job to CI, to stop drift between the Rust contract and the TS shell.
