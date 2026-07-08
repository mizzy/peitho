# PowerPoint export (Issue #140)

## Summary

Add `peitho export pptx <deck.md> -o out.pptx`. The engine is **Option B: full structural conversion** (author decision 2026-07-06). Slide slot content is structurally mapped to pptx text boxes and images so the recipient can edit and reuse the file in PowerPoint/Keynote. Geometry and typography are measured with headless Chrome (see below). pptx generation does not depend on any library — it uses hand-written zip+XML (author decision 2026-07-06).

Author decisions (2026-07-06, confirmed via AskUserQuestion):

1. Engine: **Option B (full structural conversion)** — adopts the Issue motivation "the recipient should be able to edit and reuse"
2. Keynote: **ship pptx only** — no direct `.key` generation. Guide users through Keynote's pptx import in the README. No `export keynote` subcommand
3. Speaker notes: **include in the pptx notes pane (default, no flag)** — pptx is not an "audience-facing distributable" but an "editable source" handed off. The "notes never enter dist/" invariant is interpreted as "must not contaminate audience-facing distributables (dist/, PDF)"; the pptx notes pane is included as a canonical pptx feature
4. pptx generation: **hand-written zip+XML** — the only dependency is the `zip` crate. ppt-rs (0.2.x, born 2025-11) is too young and output quality is unverified, so it is not adopted

## Motivation

- Enterprise environments that require PowerPoint delivery; sharing with Keynote teams
- Unlike PDF (#109), the recipient can edit text and reuse slides
- The second instance of §13 "adding an emit for the same intermediate representation". Adds `pptx` to the `export` subgroup (introduced in #109)

## Central design: DOM-walk measurement (single-source approach)

Option B's biggest challenge is "the gap between CSS layout and pptx layout". Solve it **where layout actually happens = the browser**:

1. core generates a **measurement HTML** (`render_measure_document`). All slide `<section>`s are laid out at the logical canvas size (`aspect_ratio().width() x height()` px), and the measurement script (`measure.js`) is embedded
2. Launch headless Chrome with `--headless=new --dump-dom --virtual-time-budget=<N>`. `measure.js` walks the DOM after `document.fonts.ready` and content image readiness; on failure it emits `peitho-measure-error`, on success it collects, per slide:
   - **rect + computed style** for `class^="slot-"` elements and the block elements and text runs (per text node) inside them
   - `<img>` rect + src + alt
   - `<section>` background-color
   Then serializes to JSON and appends it to body as `<script type="application/json" id="peitho-measure">`
3. The CLI extracts the JSON from the dump-dom output and hands it to core's pptx builder
4. core assembles a pptx zip from the measurement JSON + `Deck<Rendered>` (notes, SlideKey) + `ResolvedImageAsset` (image bytes)

### Why DOM-walk instead of IR + geometry reconciliation (adopted without author confirmation, all three lenses aligned)

The alternative was "keep Checked IR slot→fragment structure as the source of truth, and use Chrome only for geometry that reconciles into it". But:

- **long-term**: DOM-walk automatically follows custom layouts (pillar ②: layout = HTML+CSS the user writes). IR reconciliation requires "which DOM element does this fragment correspond to" rules, and every new layout accumulates carve-outs to the reconciliation rules
- **type-safety**: The measurement JSON schema becomes the sole contract; ts-rs makes Rust⇔TS a single source (rides on `bindings/` drift check). Dual sources (IR structure + measurement) surface correspondence drift only at runtime
- **root-cause**: "Not knowing the result of CSS layout" is Option B's root problem, and having the browser measure it is the root fix. Guessing layout on the IR side is symptom treatment

DOM (rendered HTML) is a deterministic projection of the Rendered IR, so this is still "structural conversion from the IR". Text runs are taken per text node so PowerPoint can re-wrap them (line wraps do not split runs).

### Measurement JSON schema (contract, ts-rs target)

Defined in peitho-core `domain.rs` (or a new module) as serde + ts-rs types, generated into `bindings/` and committed (rides on the existing drift check):

```
MeasuredDeck    { canvasWidth, canvasHeight, slides: Vec<MeasuredSlide> }
MeasuredSlide   { key: String, backgroundColor: String, boxes: Vec<MeasuredBox>, images: Vec<MeasuredImage> }
MeasuredBox     { slot: String, rect: MeasuredRect, style: MeasuredBoxStyle, paragraphs: Vec<MeasuredParagraph> }
MeasuredBoxStyle{ backgroundColor, borderColor, borderWidth, borderRadius }   // for visual chrome reproduction
MeasuredParagraph { align, bulletLevel: Option<u8>, numbered: bool, bulletContinuation: bool, numberingStartAt: Option<u16>, runs: Vec<MeasuredRun> }
MeasuredRun     { text, color, fontFamily, fontSizePx, bold, italic, underline, breaksBefore }
MeasuredImage   { src, alt, rect: MeasuredRect }
MeasuredRect    { x, y, w, h }   // logical canvas px, section top-left origin
```

- rect unit is logical canvas px. EMU conversion is `px * 9525` (96dpi). **16:9 canvas 1280x720 → 12192000x6858000 EMU = exact match with PowerPoint standard 16:9**. 4:3's 960x720 → 9144000x6858000 = matches standard 4:3. This match backs the "logical canvas px as the coordinate system" choice
- The `resolution` frontmatter key is **not used by pptx** (PDF-only physical raster size. pptx uses vector + measured px coordinates so it is unnecessary). Answers the Issue's open question "relationship with resolution"
- `\n` inside code blocks (`<pre>`) is split into paragraphs on the script side. Syntax highlight colors ride on runs directly from the computed color of `hl-*` spans (theme CSS → browser cascade → measurement, no CSS parsing required)
- Lists become `<li>` → `bulletLevel` (nesting depth) and `numbered` (true if the nearest list ancestor is `ol`). The first item paragraph of each `<ol>` carries `numberingStartAt` from the `start` attribute or 1. Second and later paragraphs of a multi-paragraph list item become `bulletContinuation` with the same indent and no bullet. `<br>` becomes `breaksBefore` on the next run to represent consecutive counts. Headings and paragraphs ride computed font-size/weight on runs

### measure.js placement and contract check

Written as `packages/peitho-present/src/measure.ts`, built into `dist/measure.js` and **committed**, embedded into core via `include_str!` — same pattern as shell.js (build artifact but committed + CI drift check). measure.ts imports `bindings/MeasuredDeck.ts` etc. and is type-checked, so any Rust⇔TS schema drift trips at compile time.

### pptx zip layout (hand-written XML)

Required parts (minimum):

```
[Content_Types].xml
_rels/.rels
docProps/core.xml, docProps/app.xml
ppt/presentation.xml (+ _rels)          … sldSz = canvas px*9525 EMU
ppt/slideMasters/slideMaster1.xml (+ _rels)
ppt/slideLayouts/slideLayout1.xml (+ _rels)   … one blank layout
ppt/theme/theme1.xml
ppt/slides/slideN.xml (+ _rels)         … 1 IR slide = 1 slide part
ppt/notesMasters/notesMaster1.xml (+ _rels)   … only for decks with notes
ppt/notesSlides/notesSlideN.xml (+ _rels)     … only for slides that have notes
ppt/media/*                             … bytes read from ResolvedImageAsset.source_abs
```

- Text: 1 slot = 1 `<p:sp>` text box (rect position, no autofit). Reproduce solidFill / border / rounded corners from `MeasuredBoxStyle`. paragraph→`<a:p>` (align, buChar+indent), run→`<a:r><a:rPr sz b i u><a:solidFill>`. Font size is `px * 0.75 * 100` (px→pt→1/100pt). Font is the first family name from font-family
- Images: only content images resolved from Markdown (with `src` starting with `assets/`) become `<p:pic>` with rect position, copied to `ppt/media/` and referenced by rel. alt goes into `<p:nvPicPr>` descr
- notes: put `RenderedSlide::notes()` into the notesSlide body placeholder as plaintext
- Slide background: solidFill from the section's background-color
- XML assembly uses string templates + escape functions, like the existing `html-escape`. Return Vec<u8> via the `zip` crate (added to workspace deps, core side)

### CLI shape

```
peitho export pptx <deck.md> -o out.pptx
peitho export pptx <deck.md>              # when -o is omitted, <deck>.pptx (next to deck.md)
```

- Add a `Pptx` variant to `ExportCommand` (main.rs). `export_pptx`, isomorphic to `export_pdf`: `build_artifacts` → measurement workspace in tempdir (measure.html + peitho.css + assets/) → `locate_chrome` (reuse existing) → `--dump-dom --virtual-time-budget=20000` → JSON extraction (between `id="peitho-measure"` markers) → core pptx builder → write out
- Missing Chrome, dump failure, and JSON extraction failure are clear errors at the same level as PDF

## Non-goals

- **No direct `.key` generation and no `export keynote` subcommand** (author decision). The README documents opening pptx in Keynote
- v1 does **not** carry over **hyperlink hrefs** (links become measured-style colored text). Future non-breaking addition via slide rels is possible
- v1 does not support **gradients or background images** (background is solid color only). Decorative elements outside slots (layout-specific ornamental divs, etc.) are also not carried over
- v1 does not carry **layout-baked / remote `<img>`** into pptx. Only Markdown content images resolved to `assets/` paths are output as editable picture parts
- v1 does not do **font embedding** (typeface name only; substitution on the viewer side is acceptable)
- The `resolution` key is not consumed by pptx (see above)

## Spike results (measured 2026-07-06, Chrome 149.0.7827.201 / macOS 15.7.7)

Measurement pipeline viability verified with real Chrome:

- With `--headless=new --virtual-time-budget=5000 --dump-dom`, page JS (wait `document.fonts.ready` → DOM walk → append `<script type="application/json" id="peitho-measure">` to body) **runs and the post-execution DOM is dumped correctly**. Takes about 3 seconds. rect (x:96, y:80, w:335.9, h:84), computed font-size (56px), color, and font-weight all match expected values. The implementation uses `--virtual-time-budget=20000` to leave headroom for large image / font decode waits
- **Critical finding: Chrome 149 does not exit after one-shot completion**. Right after completion it wakes macOS GoogleUpdater (`--wake-all`), which keeps the parent process alive. `--disable-background-networking --disable-component-update` do not prevent it (measured)
- As a result, **the already-landed `peitho export pdf` (#109/#139) hangs in the current environment** (measured: PDF is written but `.output()` blocks forever). Field regression caused by Chrome auto-update
- Completion signals are available: `--print-to-pdf` writes `N bytes written to file <path>` to stderr; `--dump-dom` terminates with `</html>` on stdout

### Root fix: shared one-shot Chrome runner

"Running and exiting one-shot headless Chrome" is a shared seam that both pdf/pptx consumers break on identically, so fix it in one place:

- Replace `run_chrome_print`'s `.output()` (exit wait) with a runner that **spawns + reads stdout/stderr through pipes + detects a completion signal + times out (default around 60s) + kills/waits the child after detection**
- Completion predicates: pdf = stderr's `bytes written to file` (and non-empty output file); dump-dom = stdout's `</html>` terminator
- The child uses a throwaway tempdir profile, so killing is fine (the pitfall "SIGTERM registers as a crash" concerns the user's persistent profile. Throwaway profiles suffer no harm from crash recovery)
- Timeout also closes the general failure mode "waiting forever on a broken page"
- Migrate `export pdf` onto this runner (fix the same root cause at the same time. No per-consumer carve-out)
- Add this measured fact to CLAUDE.md pitfalls

jsdom has no layout so rects are always 0 — measure.ts's vitest verifies only the DOM-walk logic / run extraction / paragraph splitting; geometry is verified with Chrome-gated E2E (`#[ignore]`, export_pdf.rs style). After E2E, do one manual verification of actually opening in PowerPoint/Keynote.

## Tasks (TDD)

1. ~~**Spike**~~ Complete (see "Spike results" above)
2. **Shared Chrome runner** (crates/peitho): spawn + completion signal + timeout + kill. Migrate `export pdf` (hang fix). Unit tests (completion predicate, timeout, non-empty check). Follow the fake-chrome-script approach of existing export_pdf tests
3. **Measurement schema**: add `Measured*` types (serde + ts-rs) to core domain; generate + commit bindings
4. **`render_measure_document`** (core render.rs): generate measurement HTML. Unit tests (canvas size, section enumeration, measure.js embedding, JSON marker element)
5. **measure.ts** (packages/peitho-present): DOM-walk implementation. vitest (structure walking, run integration, pre newline splitting, li bullet levels, hl-span color capture logic. jsdom has no layout so rect values are out of scope). Add dist/measure.js build + commit + drift check to CI
6. **pptx writer** (new core module `pptx.rs`): measurement JSON + Rendered + images → zip bytes. Unit tests read the zip back and assert on XML fragments (slide count, sldSz EMU, run color/size, bullets, notesSlide, media, Content_Types)
7. **CLI `export pptx`** (main.rs): workspace emit → dump-dom via Chrome runner → JSON extraction → write out. Unit tests on error paths (missing Chrome etc., follow export_pdf tests)
8. **E2E** (crates/peitho/tests/export_pptx.rs, `#[ignore]` Chrome-gated): sample deck → pptx → unzip and assert text/images/notes exist. Also verify that export_pdf's E2E passes in the current environment (regression check for the runner fix)
9. **Docs**: README (export pptx, Keynote via pptx import), CLAUDE.md structure section + pitfalls (Chrome 149 non-exit as measured)

## Related

- Issue #140 / #109 (PDF export) / #23 (aspect_ratio)
- `docs/plans/2026-07-05-pdf-export.md` (export subgroup and Chrome launch precedent)
- §13 of `docs/PEITHO_KICKOFF.md`
