# Build-time overflow lint (`peitho lint`)

Issue: #286. Author decisions (2026-07-15): the lint lives in a new `peitho lint`
subcommand (not `build`, not `doctor`), uses a fixed 1px tolerance, and reports
findings as warnings while exiting nonzero so CI can gate on it.

## Problem

Content overflowing the slide frame is a silent failure: the build succeeds and
the author only notices during the presentation or in the exported PDF. The
toolchain can measure this in headless Chrome but currently doesn't.

## Design

### CLI surface

```
peitho lint [input]        # input defaults to deck.md, like build/doctor
```

Flow (new `crates/peitho/src/lint.rs`, wired in `main.rs` like `doctor`):

1. `build_artifacts(&input)` — the full existing pipeline. Any build error
   surfaces exactly as `peitho build` would report it.
2. Emit a lint workspace into a `tempfile::tempdir()`:
   `write_shared_assets(workspace, &artifacts)` plus `lint.html` from a new
   `peitho_core::render_lint_document(&artifacts.rendered)`.
3. `locate_chrome()` (same lookup as PDF export; same
   `PEITHO_CHROME_PATH` override, same "Chrome not found" help).
4. Run the shared one-shot runner (`run_one_shot_chrome` — never
   `Command::output()`, per the Chrome 149 pitfall) with the same invocation
   shape as PDF export plus console logging:
   `--headless=new --disable-gpu --no-sandbox --no-pdf-header-footer
   --virtual-time-budget=10000 --enable-logging=stderr
   --user-data-dir=<workspace>/chrome-profile
   --print-to-pdf=<workspace>/lint.pdf file://<workspace>/lint.html`
   and `CHROME_ONE_SHOT_TIMEOUT`. The lint.pdf is a throwaway byproduct in the
   temp workspace; nothing reads it — `--print-to-pdf` is there because it is
   the measured-reliable one-shot driver (see "Transport" below).
5. Parse the measurement payload out of Chrome's stderr (see below). A missing
   or malformed payload is a hard error with help, never a silent pass.
6. Print one warning line per overflowing axis per slide, then a summary.
   Exit 0 when nothing overflows, exit 1 when at least one slide does.
   On a Chrome/parse error, keep the workspace for inspection
   (`keep_workspace_for_error`, same as PDF export).

### Lint document (`peitho-core`)

New `pub fn render_lint_document(deck: &Deck<Rendered>) -> String` in
`crates/peitho-core/src/render.rs`, alongside `render_pdf_document` but
deliberately simpler — the goal is to measure slides at their natural canvas
size, so:

- `:root { --peitho-canvas-width/-height/-aspect }` from
  `settings.aspect_ratio()` (the shell-canvas size; `resolution` is a PDF-only
  page size and is NOT consulted here).
- `html, body { margin: 0; padding: 0; }`, then each `slide.html()` emitted
  directly (the renderer already guarantees the root `<section>` carries
  `peitho-slide`). No `.peitho-slide-wrap`, no `overflow: hidden`, no
  `pdf_flatten.js` — nothing that could mask or distort the measurement.
- `.peitho-slide { transform: scale(1); }` — an identity transform, visually
  and metrically a no-op, but it establishes each slide as the containing
  block for absolute and fixed descendants (CSS Transforms). Both real
  surfaces already work this way (present's canvas target carries a
  transform; PDF export scales `.peitho-slide`), so without it an
  `position: absolute` element with no positioned ancestor would anchor to
  the page in the lint document only, fabricating cross-slide overflow for
  every stacked slide after the first.
- Embeds a measurement script from a new `crates/peitho-core/src/lint_measure.js`
  via `include_str!` (mirroring `PDF_FLATTEN_JS`).

`lint_measure.js` (runs under `--virtual-time-budget`):

- Waits for the `window` `load` event, then `document.fonts.ready`, then one
  `requestAnimationFrame`. It additionally waits for `load`/`error` on every
  `<img>` — **never `image.decode()`** (Linux headless hang pitfall, Issue #155).
- For each `section.peitho-slide`, in document order, measures whether content
  fits the slide box (1-based document order — this matches the numbering the
  author sees in present/preview; draft slides were already dropped at parse
  time, skipped slides are real slides and ARE measured, matching PDF output).

  The root's `scrollWidth`/`scrollHeight` alone is a blind proxy: content
  clipped by an INNER container never grows the root's scroll area, and the
  built-in theme does exactly that (`.body { overflow: hidden }` in
  `themes/base.css`) — a default-theme deck with too-tall body text is
  visibly cut off while the root scroll metrics stay clean. So the content
  size is the union of the slide's own `getBoundingClientRect()` with the
  border boxes of all descendant elements (starting the union from the
  slide's own rect so one-sided bleed counts), with the root's scroll metrics
  as a floor (rects miss pseudo-element boxes; rects catch inner-clipped
  content and transform bleed that scroll metrics miss). Zero-sized rects are
  skipped (`display: none` elements report 0×0 at the viewport origin, which
  would poison the union for below-the-fold slides). Fixed and absolute
  descendants need no special-casing: the identity transform above anchors
  them to their slide, matching present/PDF.
- Payload per slide: `{"slide": N, "contentWidth": .., "contentHeight": ..,
  "boxWidth": .., "boxHeight": ..}` — fractional CSS px, box from the slide's
  own rect. The CLI rounds and computes overflow per axis; the report never
  derives the box size from `aspect_ratio`, which would fabricate numbers
  when user CSS resizes the root section.
- Publishes the payload as chunked console-log lines (see Transport below).
  Base64 keeps the payload to characters that survive the console-log
  wrapping. The marker strings are composed at runtime
  (`"PEITHO_LINT_" + "CHUNK"`) so raw markers never appear in the script
  source (a verbatim marker in the page would let a completion/extraction
  step match source text instead of a genuinely published payload).

### Transport: why console-log-to-stderr, not `--dump-dom` (measured 2026-07-15)

The first implementation published via `document.title` and read it back with
`--dump-dom`. On real macOS Chrome that transport is unreliable: roughly half
of launches never dump at all — stdout stays empty for the full 60s timeout
while stderr floods with GPU `SharedImageManager::ProduceMemory` errors (the
renderer wedges and virtual time never expires). The `--print-to-pdf`
invocation shape used by PDF export is measured-reliable on the same machine
(4 consecutive E2E launches in 9.5s) and battle-tested in CI. So lint keeps
the print invocation as the one-shot driver and moves the payload to
`--enable-logging=stderr`: Chrome echoes in-page `console.log` lines to stderr
as they happen (wrapped as `[pid:ts:INFO:CONSOLE(n)] "PAYLOAD", source: ...`).
The lint args compose from the same base builder as `chrome_print_args` so a
future measured-pitfall flag added for PDF export cannot silently miss lint.

Chrome's browser and GPU processes share the stderr pipe, and writes beyond
PIPE_BUF (512 bytes on macOS) are not atomic — a GPU log line could splice
into the middle of one long payload line on larger decks (the same GPU error
flood above makes this realistic). So the payload travels as short framed
chunks: `PEITHO_LINT_CHUNK <index>/<total> <base64-slice>` lines (slices
sized so each console line stays under PIPE_BUF), followed by a final
`PEITHO_LINT_DONE` line. Chunk-level pipe atomicity is best-effort, and the
CLI still hard-errors on any reassembly gap.

### Getting the result back (CLI side)

- `ChromeCompletion::LintResultLogged`: `is_ready` scans **stderr** for the
  DONE sentinel (the existing `scan_for_needle` helper), and
  `is_ready_after_successful_exit` re-checks the final stderr for the DONE
  sentinel — a Chrome that exits 0 without ever publishing fails inside the
  runner with its stderr excerpt (the most diagnostic error), not at the
  parse step. `run_one_shot_chrome` exposes stderr to the caller alongside
  stdout so the lint caller can parse it.
- Parsing: collect chunk lines from Chrome's stderr (lossy-converted —
  stderr noise may contain arbitrary bytes and must not fail the lint when
  the payload itself is ASCII base64), validate totals agree and indexes
  `1..=total` each appear exactly once, reassemble, base64-decode,
  `serde_json` into typed measurements. No chunks, inconsistent totals,
  missing/duplicate indexes, bad base64, bad JSON, or a slide count that
  disagrees with `artifacts.slide_count` are each distinct errors with help
  text — no silent path. On parse failure the Chrome stderr log is written to
  `chrome-stderr.log` in the kept workspace so the help text points at the
  artifact the parser actually failed on.

### Threshold and output

- `const OVERFLOW_TOLERANCE_PX: i64 = 1;` — an axis is reported only when its
  overflow is **strictly greater than 1px** (absorbs scrollWidth/clientWidth
  integer-rounding noise; real overflows are typically tens of px).
- One warning line per overflowing axis, existing diagnostics tone
  (message + help), e.g.:

  ```
  warning: slide 3 content overflows the slide box vertically by 42px (content 762px, box 720px)
    help: shrink or split the slide content, or adjust the layout CSS
  ```

- Summary line always printed:
  `checked 12 slide(s): 2 overflow warning(s)` (exit 1) or
  `checked 12 slide(s): no overflow` (exit 0).

## Tests (TDD order)

peitho-core (`render.rs` tests):
1. `render_lint_document` embeds canvas vars from `aspect_ratio` and ignores
   `resolution` (16:9 + 1920x1080 deck → `--peitho-canvas-width: 1280px`, no
   `1920px` page sizing, no `transform: scale`).
2. Contains every slide's HTML in order and the measurement script; does NOT
   contain `pdf_flatten` code or `.peitho-slide-wrap`.
3. `LINT_MEASURE_JS` contains no `</script` (same guard as `PDF_FLATTEN_JS`),
   no `.decode(` (pins the Issue #155 pitfall), and no verbatim
   `PEITHO_LINT_CHUNK`/`PEITHO_LINT_DONE` (markers are runtime-composed).

peitho CLI (unit tests in `lint.rs` / `main.rs`):
4. Payload extraction: valid chunked base64+JSON round-trips (fixtures
   imitate Chrome's console-log stderr wrapping); no chunks / inconsistent
   totals / missing or duplicate chunk index / bad base64 / bad JSON /
   slide-count mismatch are distinct errors mentioning help.
5. Tolerance boundary: overflow of 2px on one axis → one warning; 1px / 0px /
   negative → no warning; both axes over → two warnings for that slide.
6. Report rendering + exit code: warnings formatted as specced; exit 1 iff at
   least one warning; summary line in both cases.
7. Chrome args builder: includes `--print-to-pdf`, `--enable-logging=stderr`,
   `--virtual-time-budget`, workspace profile dir, and the lint.html file URL.
8. `ChromeCompletion::LintResultLogged::is_ready` fires on the DONE sentinel
   in stderr, and `is_ready_after_successful_exit` rejects a successful exit
   whose stderr never carried the sentinel.

E2E (`crates/peitho/tests/lint.rs`, `#[ignore]`, `util::test_chrome_path`,
runs in the existing CI `e2e` job via `cargo test -- --ignored`):
9. A deck using the DEFAULT built-in layout/theme (no custom CSS) with body
   content tall enough to clip inside the theme's `.body { overflow: hidden }`
   → exit code 1, stdout names the slide number, the vertical axis, and a px
   delta. This pins the inner-clipping blind spot: root scroll metrics alone
   report clean here.
10. A trivially small deck → exit 0 and `no overflow` in stdout.

CLI help (`cli_help.rs`): `peitho lint --help` snapshot/assertion consistent
with the existing help tests.

## Docs

- README + `site/content/guide` command list: add `peitho lint` (one short
  section: what it measures, tolerance, exit code, Chrome requirement).
- `CLAUDE.md` structure line for `crates/peitho/` gains `lint`.

## Non-goals

- No `--json` output, no frontmatter escalation-to-error, no configurable
  tolerance (all deliberately deferred until asked; Issue #286 records the
  escalation idea).
- Not a general layout linter: the check is "content fits the slide box".
  Rect-union measurement happens to catch transform bleed (border boxes are
  transform-aware), but ink-only overflow (shadows, outlines) and
  pseudo-element boxes beyond the root scroll area stay out of scope.
