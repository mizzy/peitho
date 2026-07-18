# Built-in math renderer (Issue #287)

Date: 2026-07-18
Issue: #287
Decision: author picked "katex-rs / HTML+MathML / block-only fence" on
2026-07-18 after measured probes and a two-layer separation review.

## Decision record

Issue #287 proposed a built-in math renderer joining the same typed
resolver seam established by Issue #252 (`mermaid` via `merman`). Three
choices were surveyed and probed on macOS arm64 (2026-07-18):

| Candidate | Result |
|---|---|
| `katex-rs` 0.2.4 (pure-Rust KaTeX re-implementation) | **Adopted.** MIT, tracks KaTeX upstream commit-by-commit, `render_to_string(&ctx, latex, &settings)` returns `Result<String, ParseError>`. Probe rendered `\frac`, `\sqrt`, `\int`, `\sum`, `\lim`, `\pm`, `\begin{pmatrix}` correctly; malformed input returns typed `ParseError` (no panic). Cold `cargo build --release` on the probe: ~8.8 s wall (vs ~27 s extra for `merman`). Probe binary: 2.4 MB. |
| `typst` 0.15.1 (SVG output) | **Rejected for v1.** Pure Rust, SVG output would reuse the Issue #261 intrinsic-size normalization seam, but the crate ships the entire typesetting engine (parser, layout, font embedding) for a math-only use case, and its native syntax is not TeX (`\frac{a}{b}` vs `a/b`) — forcing typst syntax on deck authors is unacceptable, and `tex2typst-rs` conversion adds a second failure surface. Retained as a future engine behind the same trait. |
| `katex` 0.4.6 (JS bindings via QuickJS / WASM) | **Rejected.** Adds a JS runtime to `peitho-core`'s dependency graph, conflicting with the pure-Rust pipeline stance. `katex-rs` covers the same LaTeX surface without the JS backend. |

## Behavior

- A fenced code block tagged `math` with **no** `code_images:` entry for
  `math` renders at build time through the built-in KaTeX-rs renderer.
  Output is an inline HTML+MathML fragment (KaTeX's canonical
  `<span class="katex-display">…</span>` block form) carried through
  the phases as a first-class `FragmentKind::Math` and emitted into
  the body slot as `<div class="peitho-math">…</div>` (see the
  fragment-kind design below).
- The renderer is invoked in **display mode** (block equation, centered,
  own line). Inline `$...$` support is explicitly out of scope for v1
  (see "Out of scope" below).
- An explicit `code_images: math: <command>` entry **overrides** the
  built-in for the `math` tag, following the same
  external-command-escape-hatch policy PR #251 established for `mermaid`.
- All other tags are untouched: `mermaid` remains built-in via `merman`;
  `dot`, `plantuml`, etc. still require an explicit `code_images:`
  entry; unknown tags remain parse-time errors.
- Bare `code_images: math: false` / `true` remain line-numbered errors
  reserving future opt-out syntax (existing guard from PR for #252).
- KaTeX CSS + fonts are embedded in the `peitho` binary. When a deck
  contains a `math` fence, the CSS is prepended to the deck's
  `peitho.css` (library CSS first, so author rules win) and the fonts
  are written to a `katex-fonts/` output directory — in every output
  (dist, preview cache, present cache, PDF export workspace). Decks
  without math see no output change.

## Design: extend the resolver, don't fork it

Issue #252 established `CodeImagesConfig::renderer_for(tag)` as the
single typed resolution point:

```rust
pub enum CodeImageRenderer<'a> {
    External(&'a CodeImageCommand),
    BuiltinMermaid,
}
```

Adding a `math` built-in as a second `if tag == "math"` carve-out in
`parser.rs` and `code_images.rs` would be exactly the per-symptom
bandaid Issue #252 removed. Instead:

```rust
pub enum CodeImageRenderer<'a> {
    External(&'a CodeImageCommand),
    BuiltinMermaid,
    BuiltinMath,
}

impl CodeImagesConfig {
    pub fn renderer_for(&self, tag: &str) -> Option<CodeImageRenderer<'_>> {
        if let Some(command) = self.entries.get(tag) {
            return Some(CodeImageRenderer::External(command));
        }
        match tag {
            "mermaid" => Some(CodeImageRenderer::BuiltinMermaid),
            "math" => Some(CodeImageRenderer::BuiltinMath),
            _ => None,
        }
    }
}
```

Both consumers (`parser.rs` unknown-language validation and
`code_images.rs` transformation) match exhaustively on the new variant.
`_ => {}` is forbidden (pillar 3) — the compiler enforces that every
future built-in is wired at every consumer site.

## Design: math is a first-class fragment kind, not an image

`BuiltinMermaid` is SVG-shaped: run → validate SVG → normalize
intrinsic size → cache `.svg` → `FragmentKind::Image` → routes to
`accepts="image"` slots → `<img src="…svg">`.

`BuiltinMath` cannot ride that path: KaTeX output is an HTML fragment,
and there is no image. A self-review against the current pipeline
found that "inline the HTML at transform time" is not implementable as
originally sketched, because the phase pipeline routes and re-renders
fragments by `FragmentKind`:

- `mapping.rs` picks the slot from the kind (`Code` → code slot,
  `Image` → image slot, `Heading`/`Paragraph`/`List`/`Text` → body).
- `check.rs`/`render.rs` enforce the `(Accepts, FragmentKind)` contract
  matrix (`accepts_fragment`).
- **Body slots do not carry fragment HTML through**: `render_block_slot`
  concatenates the fragments' *markdown* and re-parses the join with
  pulldown-cmark. Smuggling KaTeX HTML through the `markdown` field
  would re-enter a markdown parser as a raw-HTML block — CommonMark
  splits HTML blocks at blank lines, so multi-KB KaTeX markup would be
  silently mangled. That is the silent path pillar 3 forbids.

So math becomes a first-class variant:

```rust
// domain.rs
pub enum FragmentKind<S = RawImagePath> {
    …existing…,
    /// Build-time-rendered math. `html` is the engine's output
    /// (KaTeX HTML+MathML); the LaTeX source stays in `code`/`text`.
    Math { html: String },
}
```

Every consumer is forced by exhaustive matching (pillar 3, no
`_ => {}`) to decide what math means at its site:

- `mapping.rs`: `Math` joins the `Paragraph`/`List`/`Text` arm →
  routes to the body slot. A block equation is body content.
- `check.rs` / `accepts_fragment`: add
  `(Accepts::Blocks, FragmentKind::Math)` to the contract matrix.
  **No new `accepts` token** — the layout vocabulary is unchanged, and
  every existing layout with a `blocks` slot accepts math today.
- `render.rs`: `render_block_slot` is restructured from
  "concat all markdown, parse once" to "render *runs* of markdown
  fragments with pulldown-cmark, splice `Math` fragments in between as
  `<div class="peitho-math">…html…</div>`". Markdown-only slots
  produce byte-identical output to today (single run), pinned by a
  regression test.
- `default_accepts`, `removal_noun`, `Display`, `map_src`: mechanical
  new arms.
- Explicit slot syntax (`::: {slot=name}`) composes for free: a math
  fence inside a slot group transforms recursively, and routing a math
  fragment into a non-`blocks` slot is the existing line-numbered
  contract error.

`FragmentKind` is core-internal (not a ts-rs export), so `bindings/`
does not drift. `ManifestSlideText` extraction sees the LaTeX source
via the fragment's `text` field, which is the right searchable form.

### transform seam in `code_images.rs`

`transform_fragment` matches exhaustively on the resolved renderer:

- `External(command)` / `BuiltinMermaid` → the existing SVG tail
  (validate → normalize → cache `.svg` → `SourceFragment::image`),
  unchanged.
- `BuiltinMath` → render **every build** (no cache) → a new
  `SourceFragment::math(line, html, latex_source)` constructor
  (`pub(crate)`, same discipline as `ExplicitSlot`).

**Math is deliberately uncached** (review decision, 2026-07-18). The
original draft mirrored the mermaid `.html`-in-cache design, but
review confirmed two defects that share one root cause: cached KaTeX
HTML is inlined verbatim into slide HTML, so trusting cache bytes is
a script-injection surface (a poisoned
`.peitho/code-images-cache/<hash>.html` shipping `<script>` would
execute in preview/present and enter published dist), and no
validation weaker than re-rendering can close it. The SVG cache is
structurally different — its artifact is consumed through an inert
`<img>` reference that cannot execute script, and merman rendering is
expensive enough (~100 ms+) to be worth caching. KaTeX
`render_to_string` is sub-millisecond, so dropping the cache removes
the attack surface, the validity-marker convention, and a
check-then-read race for free. The `display_mode` byte reserved in
the draft cache key disappears with the cache.

### Documented behavior notes

- A deck that supplies a custom `math.sublime-syntax` via `syntaxes:`
  and has no `code_images:` entry previously got highlighted source;
  it now gets a rendered equation. Same resolution rule as the
  Mermaid precedent ("a code-image tag never reaches the
  highlighter"). Escape hatch: a ````md wrapper fence, or an external
  override.
- An explicit `code_images: math: <command>` override takes the plain
  external-command path: the output is an **SVG image fragment** that
  routes to `image` slots, not a body-inline equation. Built-in and
  override differ in slot routing because external commands are
  SVG-in-SVG-out by contract (`SvgRunner`). This is documented, not
  hidden: the override help text says so.

## Design: two-layer separation for future engine swap

The author asked whether the engine choice is reversible. It is,
provided the input and output boundaries are typed contracts:

```
Markdown layer (peitho contract, engine-independent):
    block:  ```math ... ```
    inline: $...$   (v2, opt-in via frontmatter — not implemented)

    ↓ extract LaTeX string

Engine layer (swappable behind a trait):
    trait MathRenderer {
        fn render(&self, latex: &str, display: bool)
            -> Result<MathOutput, MathError>;
    }
    enum MathOutput {
        HtmlFragment(String),  // KaTeX: HTML+MathML
        // an Svg variant arrives with a real SVG engine (typst);
        // exhaustive matches will force every consumer site then
    }
```

- **Input is always LaTeX.** The peitho-side delimiter (`` ```math ``
  fence, future `$...$`) is engine-independent. Deck authors write
  LaTeX and stay unaffected by future engine swaps.
- **A typst swap goes through `tex2typst-rs` internally.** Not exposed
  to authors. The pipeline becomes LaTeX → typst syntax → SVG, and the
  outer contract stays LaTeX-in.
- **`MathOutput` is deliberately single-variant in v1** (review
  decision, 2026-07-18: a speculative `Svg(Vec<u8>)` variant forced an
  unreachable error arm at the only consumer). Adding the variant
  together with the engine that produces it gets the same
  compiler-driven exhaustiveness at the moment it is actually needed.

v1 implements `MathRenderer` for `katex-rs` only, but the trait is
present from day one so a follow-up PR can add a typst impl behind
frontmatter or feature flag without touching consumer sites.

## Cache key

Not applicable: built-in math has no cache (see the transform-seam
section for the security rationale). External `code_images: math:`
overrides ride the unchanged SVG cache path with the existing
external-entry key. The mermaid built-in cache key is untouched.

## KaTeX asset embedding

KaTeX HTML fragments require a matching stylesheet and font files.
Measured asset set (KaTeX 0.16.25, the version katex-rs 0.2.4
tracks; see version note below):

- `katex.min.css` — 23,335 bytes
- 20 woff2 fonts — ~240 KB total, largest ~28 KB
  (AMS-Regular, Main-Regular, Math-Italic, Size1-4, etc.)

**Version pinning:** `katex-rs` 0.2.4 tracks a specific upstream KaTeX
commit (`785315c0…`, stated in its README). The vendored CSS + fonts
must come from **that KaTeX version**, not from whatever is latest —
class names and font metrics are coupled to the emitting renderer.
Implementation step 0 resolves the commit to its release tag and
vendors that exact `dist/katex.min.css` + `dist/fonts/*.woff2`
(woff2 only; woff/ttf fallbacks are for legacy browsers peitho does
not target, and dropping them keeps the embed at ~260 KB).

Approach:

- `crates/peitho-core/assets/katex/` checked into the repo:
  `katex.min.css`, `fonts/*.woff2`, `PROVENANCE.md` (KaTeX version,
  source URL, retrieval date, upstream commit tracked by katex-rs),
  and the KaTeX MIT `LICENSE`.
- `include_bytes!`/`include_str!` embeds them into `peitho-core`
  (~260 KB binary growth; one-time, acceptable next to merman's
  ~12 MB).
- **CSS rides the existing `peitho.css` seam, not a new `<link>`,
  and the join lives in `render_deck`, not the CLI** (review
  decision, 2026-07-18). `render_deck` takes the assembled theme CSS
  and produces the final CSS in `Deck<Rendered>`'s `css` field, so no
  future core consumer can forget the join — the type carries the
  finished stylesheet. When the deck contains at least one `math`
  fragment, the KaTeX CSS is **prepended** (library CSS first, so
  deck/theme rules targeting `.katex` classes win at equal
  specificity — appending after user CSS silently overrode author
  customizations like `.katex { font-size: 1.6em; }`). Font URLs are
  rewritten `url(fonts/` → `url(katex-fonts/` and the `.woff`/`.ttf`
  fallback `src` entries are stripped (only the 20 `.woff2` files are
  emitted; leaving the fallbacks would ship 40 dangling URLs that
  404 on engines not picking the woff2 source). Both rewrites happen
  once at embed time on the deterministic vendored CSS. No template,
  server route, or publish-check change is needed for the CSS.
- **Fonts get their own `katex-fonts/` output directory.** The
  existing user-font seam (`write_fonts_assets`) deletes and
  recreates `fonts/` from the deck's `fonts:` source on every build —
  merging KaTeX fonts into it would either be wiped or collide with
  the "user fonts are copied verbatim" invariant. A sibling
  `katex-fonts/` directory, written by the same artifact-emission
  sites, keeps the two ownership domains separate.
- Whether the deck uses math is a typed artifact field
  (e.g. `artifacts.math_assets: Option<MathAssets>` populated at
  Rendered), so each emission site (dist write, preview generation,
  present cache, PDF workspace) decides explicitly; decks without
  math see zero change to their output.
- Deck CSS validation (keyed selectors / bare `.slot-*`) runs on user
  CSS files before artifact assembly; the prepended KaTeX CSS is not
  user CSS and joins after validation, same as the built-in base
  theme.
- The `SourceFragment::math` constructor stores the LaTeX source in
  the `markdown` and `code` fields and leaves `text` empty, matching
  the `SourceFragment::code` precedent (review decision, 2026-07-18:
  the draft cloned the source into all three). Manifest body text
  reads `code_text().trim_end()` — the parsed fence text carries a
  trailing newline that would otherwise leak a blank-line artifact
  into `manifest.json` body text.

## Error handling (no silent path)

- `Err(katex::ParseError)` → line-numbered build error at the fence's
  opening ` ``` ` line, message includes the KaTeX error string, help
  names the `code_images.math` override. The implementation reuses the
  standard `code_image_error` prefix for consistency with the mermaid
  built-in:

  ```
  code_images 'math' failed: KaTeX parse error: Unexpected end of input in a macro argument, expected '}' at end of input: \frac{1}{
    help: fix the LaTeX source, or set code_images.math to an external command
  ```
- An empty or whitespace-only ` ```math ` fence → line-numbered build
  error ("math block is empty" + override help), matching the stance
  the mermaid built-in takes on undetectable input (review decision,
  2026-07-18: KaTeX happily renders `""` into an invisible
  `katex-display` block, which would ship a phantom box plus the full
  font emission for a drafting stub). A fence containing only a `%`
  comment stays allowed — that is deliberate LaTeX input.
- Any `panic!` from `katex-rs` (defense in depth — the crate does not
  currently panic on documented inputs) → the same `catch_unwind`
  wrapper `merman` uses in `code_images.rs` catches it and returns a
  line-numbered Asset error.

## Out of scope

- **Inline `$...$` syntax.** The delimiter conflicts with literal
  dollar signs in prose (`$100`, `$PATH`), and any addition needs an
  opt-in frontmatter design. v1 is block-only via ` ```math ` fence.
  The `display` parameter on `MathRenderer::render` reserves room for
  a future inline mode.
- **Custom KaTeX macros / trust modes / equation numbering.** All
  `Settings` builder options stay at KaTeX defaults for v1. Future
  frontmatter (e.g. `math: { macros: {...} }`) can layer on.
  **Security note for that future work:** the injection safety of
  inlined math HTML rests on KaTeX's default `trust: false` (audited
  2026-07-18: `\href`/`\url`/`\includegraphics`/`\htmlData` are
  neutralized, all text/attribute content entity-escaped). Any
  frontmatter that exposes `trust` or macro definitions must redo
  that audit before shipping.
- **typst engine.** Design leaves room via the `MathRenderer` trait +
  `MathOutput` enum, but implementation deferred until v1 is stable.
- **Alternate LaTeX renderers** (`mdbook-katex`, `xu-cheng/katex`
  QuickJS backend): rejected above.
- **PDF export math rendering:** KaTeX HTML fragments print through
  Chrome the same as other HTML content; no new `pdf_flatten.js`
  interaction is expected. Verified in E2E rather than special-cased.
- **Publish/notes contamination:** math HTML rides in the slide body,
  so the existing publish contamination check (which gatekeeps
  `notes.json` and shell/preview JS) does not need to change. E2E
  verifies `katex-fonts/` is present only when a deck uses math.

## TDD task list

Every step: red test first, then minimal green, in `peitho-core` unless
noted.

1. **Resolver** (`domain.rs`): `renderer_for` returns `BuiltinMath` for
   bare `math`; existing `BuiltinMermaid` behavior unchanged; explicit
   `math` entry returns `External`.
2. **Parser accepts bare ```math** (`parser.rs`): a deck with no
   `code_images:` frontmatter and a `math` fence parses (no
   unknown-language error). A bare ```notmath fence still errors with
   the existing line-numbered unknown-language message (no accidental
   widening). Existing `mermaid` acceptance stays green.
3. **`FragmentKind::Math` variant** (`domain.rs`): add
   `Math { html: String }` plus the `pub(crate)`
   `SourceFragment::math(line, html, latex_source)` constructor. The
   compiler's exhaustiveness errors enumerate every consumer site;
   each gets a red test before its arm lands: mapping routes math to
   the body slot; `accepts_fragment` gains
   `(Accepts::Blocks, FragmentKind::Math)`; routing math into a
   non-`blocks` slot (e.g. via `::: {slot=code}`) is the existing
   line-numbered contract error; `default_accepts` / `removal_noun` /
   `Display` / `map_src` arms.
4. **`MathRenderer` trait + katex-rs impl** (`math.rs`, new): trait
   with `render(latex, display) -> Result<MathOutput, MathError>`;
   `KatexRenderer` renders through a module-level
   `LazyLock<KatexContext>` static (same pattern as merman's
   `HeadlessRenderer`) and calls
   `katex::render_to_string` with `display_mode`, returning
   `MathOutput::HtmlFragment`. Malformed LaTeX returns `MathError`
   carrying KaTeX's message.
5. **Built-in Math transform** (`code_images.rs`): a `math` fence with
   empty config becomes a `FragmentKind::Math` fragment carrying the
   KaTeX HTML; nothing is written to the code-image cache directory
   (no `.svg`, no `.html` — math is uncached by design);
   `FakeRunner.calls == 0`.
6. **Override wins** (`code_images.rs`): with `code_images: math:
   <cmd>` config, `FakeRunner.calls == 1` and the external SVG path
   is taken — the result is a `FragmentKind::Image` fragment (slot
   routing difference documented in the design). Built-in did not run.
7. **Built-in error paths** (`code_images.rs`): malformed LaTeX →
   `ErrorKind::Asset`, `line` = fence line, message contains KaTeX's
   error text, help names the `code_images.math` override. The render
   call runs under the same `catch_unwind` discipline as merman
   (in-process render against mid-keystroke preview input).
8. **Embedded CSS shape** (`math.rs`): the rewritten KaTeX CSS
   contains exactly 20 `url(katex-fonts/` references, zero `.woff)`
   and zero `.ttf)` occurrences (fallback `src` entries stripped to
   match the woff2-only emission), and no remaining `url(fonts/`.
9. **Body slot splicing** (`render.rs`): `render_block_slot` renders
   markdown runs and splices `<div class="peitho-math">…</div>`
   between them. Regression pin: a body slot with no math renders
   byte-identical to the current implementation; a body slot with
   `paragraph / math / paragraph` keeps order and paragraph
   integrity; `breaks: true` still applies to the markdown runs.
10. **KaTeX asset emission** (`render.rs` + `crates/peitho`
    emission sites): `render_deck` takes the theme CSS and produces
    the final CSS in `Rendered.css` — KaTeX CSS **prepended** for a
    math deck (order pinned: library first, theme/user rules win),
    theme CSS unchanged otherwise — and `katex-fonts/*.woff2` are
    written (byte-exact with the embedded assets) in dist, preview
    generation, present cache, and PDF export workspace. A deck
    without math emits neither (byte-identical `peitho.css` to
    today, no `katex-fonts/`
    directory).
11. **CLI integration + publish** (`crates/peitho`): build a temp deck
    with a `math` fence and no `code_images:` frontmatter → `dist/`
    contains the equation HTML, prepended CSS, and `katex-fonts/`;
    publish passes the contamination check (no `notes.json`, no
    shell/preview JS). (Exercises the full `parse_deck_and_transform`
    path plus asset emission.)
12. **Docs**: update `README.md`,
    `site/content/guide/frontmatter.md`,
    `site/content/guide/writing-decks.md`,
    `site/content/examples/code-images.md` (or add a new example page
    if math warrants one), and add `examples/math/deck.md` +
    `site/content/examples/math.md` + `$(DEMO_DECKS)` entry.  Update
    the invariant paragraph in `CLAUDE.md` describing the resolver
    ("`mermaid` and `math` are built in; every other tag stays
    user-declared", plus the `MathRenderer`/`MathOutput` seam, the
    `SvgCodeImageRenderer` narrowing, and the
    KaTeX asset emission policy).

## Verification beyond `cargo test`

- Full gates: 3× `cargo test --workspace`, clippy `-D warnings`, fmt
  check, bindings drift (`CodeImageRenderer` is core-internal, no
  ts-rs export — expect no drift), shell/preview drift.
- Real-browser E2E (Opus side; Codex sandbox cannot run Chrome):
  `peitho build` a zero-config math deck, open in Chrome, confirm the
  equation displays with correct KaTeX styling (fonts loaded, no
  broken font glyphs, no CORS errors on `file://` for fonts).
- PDF export of the same deck: verify the printed page shows the
  equation with correct glyphs, including in Preview.app via
  `pdfseparate` + `sips`.
- Publish check: `peitho publish` on the math deck produces a `dist/`
  with `katex-fonts/` present, `notes.json` and shell/preview JS
  absent (contamination check passes).
- Preview watch: change the LaTeX source in a `math` fence, verify
  the preview reloads with the new equation (asset copy runs on every
  generation).

## References

- Issue #287 (this task)
- Issue #252 / `docs/plans/2026-07-15-builtin-mermaid.md` (resolver
  seam origin, `code_images.mermaid: false` guard reused verbatim)
- Issue #261 / `docs/plans/2026-07-12-code-images-svg-intrinsic-size.md`
  (why SVG needs normalization; math does not because output is HTML)
- Issue #241 (why external commands remain the fallback)
- KaTeX repository: https://github.com/KaTeX/KaTeX
- `katex-rs` crate: https://crates.io/crates/katex-rs (v0.2.4 probed)
- `tex2typst-rs` crate: https://crates.io/crates/tex2typst-rs
  (referenced for future typst-engine swap)
