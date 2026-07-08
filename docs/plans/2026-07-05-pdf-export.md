# PDF export (Issue #109)

## Summary

Add `peitho export pdf <deck.md> -o out.pdf`. Go through the same Markdown → Rendered IR, and run the equivalent of `Page.printToPDF` in headless Chrome to produce one PDF page per slide. The first concrete example of the "additional emit target against the same IR" foreshadowed in spec §13.

## Motivation

- PDF distribution (no browser required, printing, archive)
- Actually exercise the §13 extension point and add the 4th emit target to the CLI alongside build / present / publish
- With Issue #23 we split `aspect_ratio` (for display) and `resolution` (for PDF); introduce the latter with its consumer so we don't keep an "accepted but unused" key (which the invariants forbid)

## Non-goals

- v1 does **not include speaker notes in the PDF**. No `--with-notes` flag either. Aligned with the "notes do not go into distributed artifacts" invariant and keeps implementation small. Can be added non-destructively as a sub-feature of the `export pdf` CLI shape when needed
- v1 does **not integrate with `peitho publish`**. PDF is self-contained under the `export pdf` subcommand. `publish --pdf` can be added non-destructively later if needed
- v1 **leaves font embedding to the browser**. Chrome's `printToPDF` embeds WebFont / system fonts by default. No additional embedding done
- v1 does **not accept paper names (A4, Letter) for `resolution`**. WxH px only. Non-destructive future extension

## Design decisions

### CLI shape: `peitho export pdf`

```
peitho export pdf <deck.md> -o out.pdf
peitho export pdf <deck.md>              # default `<deck>.pdf` (next to deck.md) when -o is omitted
```

- `export` is a subgroup with future emit targets (thumbnails, ...) in mind. v1 has only `pdf`
- Only `-o`/`--out` accepted. No `--watch` (PDF is one-shot output)
- A missing/failing Chrome environment gets a clear error message following the line-numbered build-error style (existence check happens at export time, not build time)

Not a three-lens forced answer, so this is an author decision (proceed with this choice): `peitho export pdf` per the Issue proposal.

### Engine: headless Chrome via `--print-to-pdf`

**Adopted**: call Chrome via the existing `std::process::Command` style and pass just `--headless=new --print-to-pdf=<out> <URL>`. No CDP WebSocket needed.

- Three lenses agree: long-term (automatically follows CSS changes), type-safety (browser build ↔ PDF start from the same Rendered IR), root-cause (as §13 predicted: "add an emit target against the same IR")
- Reuses existing Chrome dependency (browser.rs already launches Chrome); no new crate
- Alternatives `chromiumoxide` / `headless_chrome` pull in heavy async dependencies — not adopted
- Rust-native (printpdf etc.) breaks pillar ① ("layouts single-sourced in HTML/CSS") — not adopted

Chrome invocation shape:
```
$CHROME --headless=new \
        --disable-gpu \
        --no-sandbox \
        --no-pdf-header-footer \
        --print-to-pdf=<absolute_out_path> \
        --user-data-dir=<temp_dir>/chrome-profile \
        <url>
```

- `--no-sandbox` is set always since it is needed in CI / containers (limited safety impact in headless)
- `--user-data-dir` is a dedicated temp directory for export (doesn't pollute the existing `chrome-profile-slides` / `presenter`)
- `--disable-gpu` is headless convention

### Chrome discovery: reuse browser.rs

`browser.rs` already has Chrome discovery logic. Extend it for PDF export use as well. When Chrome is not found, the error is CLI-level miette (`install Google Chrome or set PATH to a Chromium-based browser`) rather than the line-numbered build-error style with help (Chrome is not an input to build artifacts).

Linux/Windows: accept Chromium as well. macOS: `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`. Reuse the existing `browser.rs::detect_chrome` (or equivalent).

### `resolution` frontmatter key

Follow the spec finalized in the comments on Issue #109:

- **Value**: `WxH` px string only (e.g. `1920x1080`). Paper names not supported
- **Default**: derived from `aspect_ratio` when unset
  - `16:9` → `1920x1080`
  - `4:3` → `1440x1080`
- **Consistency rule**: if `resolution`'s aspect ratio does not match `aspect_ratio`, **line-numbered build error**
  - e.g. `aspect_ratio: 16:9` + `resolution: 1024x768` → error
- **Invalid values** (empty, `WxH` parse error, 0×0, overflow) are all line-numbered build errors

**Default resolution**: `aspect_ratio: 16:9` → `1920×1080` is higher than the canvas logical size (1280×720). Reason: PDFs can be printed / zoomed, so we pick a high physical resolution. CSS px is interpreted at `96 DPI`, so 1920×1080 becomes a 20 inch × 11.25 inch page inside Chrome. Interpret it as the physical size of a per-slide page, not as a paper size.

### Type: `Resolution` newtype in `peitho-core`

Introduce `Resolution` as the counterpart to the `AspectRatio` enum.

```rust
/// A physical PDF page size in CSS pixels (96 dpi).
/// Constructed only via `Resolution::from_frontmatter` or
/// `Resolution::from_aspect_ratio_default`, so raw (u32, u32)
/// pairs cannot masquerade as a validated resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Resolution {
    width: u32,
    height: u32,
}

impl Resolution {
    /// Frontmatter parse — the only entry point for user-authored values.
    /// Validates `WxH` shape, non-zero, and no overflow.
    pub fn from_frontmatter(raw: &str) -> Result<Self, String> { ... }

    /// Default derived from aspect_ratio when frontmatter omits `resolution`.
    /// - 16:9 → 1920x1080
    /// - 4:3 → 1440x1080
    pub fn from_aspect_ratio_default(ratio: AspectRatio) -> Self { ... }

    pub fn width(self) -> u32 { self.width }
    pub fn height(self) -> u32 { self.height }

    /// Verify `self`'s aspect ratio matches the deck's `AspectRatio`.
    /// Returns Ok if they match, Err with a message if they diverge.
    pub fn check_matches(self, ratio: AspectRatio) -> Result<(), String> { ... }
}
```

**Rationale — express the invariant in the type**:
- `Resolution`'s fields are not `pub` → external crates can only build validated values
- Comparison paths against raw `(u32, u32)` are closed (mislabel prevention)
- `check_matches` is called inside `DeckSettings::new`. Callers other than the frontmatter parser also go through the integrity check at construction

### Add `resolution` to DeckSettings

```rust
pub struct DeckSettings {
    // ... existing
    aspect_ratio: AspectRatio,
    resolution: Resolution,      // new
}
```

Add `resolution: Option<Resolution>` to `DeckSettings::new`. If `None`, use `Resolution::from_aspect_ratio_default(aspect_ratio)`; if `Some`, call `check_matches(aspect_ratio)` internally. Update all existing call sites (including test fixtures).

**Not on Manifest**: `resolution` is a concern of PDF export alone; browser build / present have no reason to read it. Putting it on the manifest creates an "accepted but unused" payload, so we deliberately omit it.

### Parser: `resolution` key handling

Add `resolution: Option<String>` to `parser.rs::DeckFrontmatter`:

1. Raw is `None` → pass `None` to `DeckSettings::new(..., None, ...)`; constructor uses `Resolution::from_aspect_ratio_default(aspect_ratio)`
2. Raw is `Some(s)` → `Resolution::from_frontmatter(s)` → pass to `DeckSettings::new(..., Some(resolution), ...)`; constructor calls `check_matches(aspect_ratio)`
3. Errors from either path are line-numbered build errors

Add `"resolution"` to `frontmatter_key_lines`.

### PDF entry HTML: `pdf.html`

PDF must actually be laid out in a browser, so generate a dedicated entry HTML:

```html
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="peitho.css">
  <style>
    @page {
      size: {{resolution.width}}px {{resolution.height}}px;
      margin: 0;
    }
    html, body { margin: 0; padding: 0; background: #fff; }
    .peitho-slide {
      width: {{resolution.width}}px;
      height: {{resolution.height}}px;
      page-break-after: always;
      page-break-inside: avoid;
      break-after: page;
      break-inside: avoid;
      overflow: hidden;
      /* Scale the 1280x720 (or 960x720) canvas up to resolution. */
      transform: scale({{resolution.width / aspect_ratio.width}});
      transform-origin: top left;
    }
    .peitho-slide:last-child { page-break-after: auto; break-after: auto; }
  </style>
</head>
<body>
  {{#each slides}}<div class="peitho-slide-wrap">{{slide.html}}</div>{{/each}}
</body>
</html>
```

In practice, no Handlebars — assemble on the Rust side as a string (same style as the existing `render_distribution_index`).

**Sizing calculus**:
- The layout's `.peitho-slide` declares 1280×720 (or 960×720) via the `--peitho-canvas-width/height` CSS variables (introduced by Issue #23)
- The PDF `.peitho-slide-wrap` creates a `resolution.width × resolution.height` frame, and inside it the inner `.peitho-slide` (canvas logical size) is scaled up with `transform: scale()`
- The key is to separate logical and physical sizes. Do not break the base.css font-size design

For `resolution: 1920x1080` + `aspect_ratio: 16:9` (canvas 1280×720), scale = 1.5.

**Rendering location**: add `render_pdf_document(&Deck<Rendered>) -> String` to `render.rs`. Separate from the existing `render_distribution_index` (different role — distribution index is fetch-driven and dynamic; PDF entry is static and inline).

### Export pipeline

```rust
fn export_pdf(input: PathBuf, out: Option<PathBuf>) -> miette::Result<()> {
    // 1. Same as build: parse → map → check → render.
    let artifacts = build_artifacts(&input)?;

    // 2. Determine output path.
    let out = out.unwrap_or_else(|| input.with_extension("pdf"));

    // 3. Emit static PDF entry into a temp directory:
    //    <tmp>/pdf.html
    //    <tmp>/peitho.css
    //    <tmp>/assets/*   (images)
    //    (No slides/, no manifest — the entry HTML inlines slide HTML.)
    let tmp = tempfile::tempdir()?;
    emit_pdf_workspace(tmp.path(), &artifacts)?;

    // 4. Locate Chrome.
    let chrome = locate_chrome()?;

    // 5. Invoke Chrome:
    //    $chrome --headless=new --disable-gpu --no-sandbox
    //            --no-pdf-header-footer
    //            --user-data-dir=<tmp>/chrome-profile
    //            --print-to-pdf=<abs_out_path>
    //            file://<tmp>/pdf.html
    run_chrome_print(&chrome, tmp.path(), &out)?;

    println!("exported {} slide(s) to {}", artifacts.slide_count, out.display());
    Ok(())
}
```

**Why `file://` not a local HTTP server**:
- pdf.html references `peitho.css` and `assets/*` via same-origin relative paths
- Under `file://` origin, relative fetches through img/link/script tags don't hit CORS (fetch API is restricted, but pdf.html doesn't use fetch — everything is inline and static)
- Standing up a server adds ephemeral port management and shutdown; `file://` is enough for static HTML that doesn't fetch

**Image assets**: copy the existing `ResolvedImageAsset` into `assets/` (same logic as build). Relative src resolves from pdf.html's document location.

**Syntax highlighting**: the `hl-*` class spans generated by `render_slide` depend on `peitho.css` (theme). Reuse the existing CSS pipeline as-is.

### Notes stripping

`RenderedSlide::notes` is never embedded in the PDF entry HTML. To uphold the invariant "notes do not go into distributed artifacts", assert explicitly (verify inside `emit_pdf_workspace` that notes are excluded from the rendered HTML).

### Chrome discovery module

Detect the Chrome executable path:

```rust
fn locate_chrome() -> miette::Result<PathBuf> {
    // macOS: /Applications/Google Chrome.app/Contents/MacOS/Google Chrome
    // Linux: PATH lookup for `google-chrome` / `google-chrome-stable` / `chromium` / `chromium-browser`
    // Windows: registry / PATH lookup (defer to v1.1 if not trivial; unsupported in v1)
    // Env override: PEITHO_CHROME_PATH
}
```

- `PEITHO_CHROME_PATH` env var takes top priority (for CI or custom paths)
- If not found: `miette::miette!("Chrome not found\nhelp: install Google Chrome or Chromium, or set PEITHO_CHROME_PATH=<absolute-path>")`

## Scope of changes (implementation scope for Codex)

### Rust: peitho-core

- [ ] `domain.rs`: add `Resolution` newtype (private fields, `from_frontmatter`, `from_aspect_ratio_default`, `width`, `height`, `check_matches`, serde `try_from = "String"`)
- [ ] `phase.rs::DeckSettings`: add a `resolution: Resolution` field; add `resolution: Option<Resolution>` to `DeckSettings::new`; run default derivation on `None` and `check_matches(aspect_ratio)` on `Some` internally; update all existing call sites (including fixtures); `resolution()` accessor
- [ ] `parser.rs::DeckFrontmatter`: add `resolution: Option<String>` field; on build, run `Resolution::from_frontmatter` and pass to `DeckSettings::new(..., Some(resolution), ...)`, converting errors into line-numbered build errors
- [ ] `parser.rs::frontmatter_key_lines`: add `"resolution"`
- [ ] `render.rs`: add `render_pdf_document(deck: &Deck<Rendered>) -> String`. Stack `.peitho-slide`s and inline `@page {size:WxH; margin:0}` plus scale transform. Internal flow that asserts no notes embedding

### Rust: peitho crate

- [ ] `src/main.rs`: add `Command::Export { command: ExportCommand }`. `ExportCommand::Pdf { input, out }` variant
  ```rust
  #[derive(Debug, Subcommand)]
  enum Command {
      Build { ... },
      Present { ... },
      Publish { ... },
      Export {
          #[command(subcommand)]
          command: ExportCommand,
      },
  }
  #[derive(Debug, Subcommand)]
  enum ExportCommand {
      Pdf {
          input: PathBuf,
          #[arg(short, long)]
          out: Option<PathBuf>,
      },
  }
  ```
- [ ] `src/main.rs::export_pdf`: implement pipeline (reuse build_artifacts → emit_pdf_workspace → locate_chrome → run_chrome_print)
- [ ] `src/main.rs::emit_pdf_workspace`: write `pdf.html`, `peitho.css`, `assets/*` into a temp dir (reuse / refactor portions of the existing `emit_distribution`)
- [ ] `src/main.rs::locate_chrome`: `PEITHO_CHROME_PATH` first → macOS default path → Linux PATH lookup (reuse `browser.rs`'s Chrome detection if any)
- [ ] `src/main.rs::run_chrome_print`: `Command::spawn` + wait + exit-code check → output file existence check
- [ ] `Cargo.toml`: move `tempfile.workspace = true` from dev-dependencies to regular dependencies (there is already a workspace setting; verify)

### Tests

- [ ] `peitho-core/tests/parser`: `resolution: 1920x1080` → parse OK; `resolution: 1024x768` + `aspect_ratio: 16:9` → line-numbered error; `resolution: abc` → error; `resolution: 0x1080` → error; `resolution: 9999999999x1080` (10 digits > u32::MAX) → u32 parse-fail error; default derivation (16:9 → 1920×1080, 4:3 → 1440×1080)
- [ ] `peitho-core/src/domain.rs`: unit tests for `Resolution::from_frontmatter`, `from_aspect_ratio_default`, `check_matches`
- [ ] `peitho-core/src/render.rs`: `render_pdf_document` includes all slides; includes `@page {size:1920px 1080px}`; includes `page-break-after: always`; includes no notes at all
- [ ] `peitho/tests/export_pdf.rs`: new integration test. Export a fixture deck.md and verify output PDF exists, has non-zero size, and starts with `%PDF-` header. **Skip in environments where Chrome cannot be detected** (CI has `ChromeDpEnvVar`, otherwise treat as `#[ignore]` or `PEITHO_CHROME_PATH=$(command -v chromium || echo /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome) cargo test`)

### Bindings

Unlike `AspectRatio`, `Resolution` is not on the manifest, so no ts-rs binding is needed.

### Examples

- [ ] `examples/pdf-export/`: sample deck for smoke test (with README). Set `aspect_ratio: 16:9` and `resolution: 1920x1080` explicitly

### Docs

- [ ] This plan file (already written)
- [ ] Add `resolution` to the frontmatter key list in `CLAUDE.md` (in the worktree / PR)
- [ ] Close Issue #109 (PR's `Closes #109`)

## Test plan (TDD order)

Instruction order for Codex:

1. Unit test for `Resolution::from_frontmatter` (RED) → implement (GREEN)
2. `Resolution::from_aspect_ratio_default` (RED → GREEN)
3. `Resolution::check_matches` (RED → GREEN)
4. Serde `try_from` / `into` round-trip (RED → GREEN)
5. Parser tests: `resolution` key valid / invalid / default derivation / mismatch (RED → GREEN)
6. Change `DeckSettings::new` signature (`resolution: Option<Resolution>`; internal default derivation + `check_matches`) → update existing callers (REFACTOR)
7. `render_pdf_document` (RED → GREEN); notes exclusion assert (RED → GREEN)
8. `export_pdf` pipeline integration test (in a Chrome-available environment; `#[ignore]` acceptable)
9. Manual E2E: `peitho export pdf examples/pdf-export/deck.md -o /tmp/out.pdf` → open and verify (past flashing / black-screen incidents make real-PDF confirmation mandatory)

## Root-cause / long-term / type-safety self-check

- **Root-cause**: Issue #109's scope is "implement PDF output", and the `resolution` key was foreshadowed at the split of #23 as unimplemented. Implementing it together in the same PR is correct (avoids "just add `aspect_ratio` from #23 and leave `resolution` accepted-but-unused")
- **Long-term / type-safety**:
  - Newtyping `Resolution` means external construction is possible only through `from_frontmatter` or `from_aspect_ratio_default` → the path where a raw `(u32,u32)` masquerades as a Resolution is closed
  - Calling `check_matches(aspect_ratio)` inside `DeckSettings::new` means there is no path where a mismatched `Deck<P>` circulates (no consumer has to remember to call `check_matches`)
  - Grouping into `Command::Export { ExportCommand }` means future export targets (thumbnails etc.) become an enum variant addition — represented in the type
- **Silent path**:
  - Invalid `resolution` → line-numbered error
  - Mismatch between `aspect_ratio` and `resolution` → line-numbered error
  - Chrome not found → clear error with help
  - Chrome non-zero exit → propagated with stderr
  - Empty / missing PDF output → error (sanity check)
- **The "new caller tomorrow" check**: future export targets (thumbnails etc.) just take `Deck<Rendered>` + `Resolution` (the type leads them). The often-forgotten deck resolution rides on `DeckSettings`, so an explicit argument is required

## Verification gates

CLAUDE.md-mandated:
- `cargo test --workspace` (3 times in a row)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` (expected: no changes this time)
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js`

E2E (manual):
- In a Chrome-available environment: `cargo run -p peitho -- export pdf examples/pdf-export/deck.md -o /tmp/out.pdf`
- Open the produced `out.pdf` and verify all slides render correctly (including code / images / background colors)
- Same verification with an `aspect_ratio: 4:3` fixture
- `resolution: 1024x768` (mismatch with 16:9) results in a line-numbered error

## Open questions (check with the author if they surface during implementation)

- Fallback order for Chrome detection in Linux / Windows / CI environments: PEITHO_CHROME_PATH → which specific binary names to try (google-chrome-stable / google-chrome / chromium / chromium-browser). Better to confirm at implementation time rather than fix in the Issue
- Whether `@page` size can be specified in CSS px: needs measurement. If not, convert to inches (width_px / 96)
- Handling of an empty Deck (0 slides): skip PDF generation and error, or emit an empty PDF

## Shipped divergence / adjustments during review (2026-07-06)

1. `Resolution` serde shipped not as a private wrapper but as `#[serde(try_from = "String", into = "String")]`. `TryFrom<String>` delegates to `Resolution::from_frontmatter`, and `From<Resolution> for String` produces the `WxH` wire string.
2. `DeckSettings::new` shipped taking `resolution: Option<Resolution>` rather than a required resolved value. `None` resolves via `Resolution::from_aspect_ratio_default(aspect_ratio)`, and `Some` passes through `check_matches(aspect_ratio)` and the ≥ canvas logical size check inside the constructor, so callers other than the parser also cannot circulate mismatched / undersized resolutions.
