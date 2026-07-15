# Built-in Mermaid renderer for code_images (Issue #252)

Date: 2026-07-15
Issue: #252 (reopens the Issue #241 non-goal for Mermaid only)
Decision: author picked "Mermaid only" on 2026-07-15 after measured probes.

## Decision record

Issue #252 asked whether the 2026 pure-Rust crate landscape justifies built-in
runners for `mermaid` / `dot` / `plantuml`. Measurements (2026-07-15, macOS
arm64, release builds):

| Candidate | Result |
|---|---|
| `merman` 0.7.0 (Mermaid) | **Adopted.** crates.io, MIT/Apache-2.0, parity target `mermaid@11.15.0`, golden-snapshot tested upstream. Probe rendered flowchart, sequence, class, state, ER, pie, gantt, and quadrant correctly. Output shape is mmdc-style (`width="100%"` + `viewBox`), which the Issue #261 normalization seam already handles. Errors are typed values with messages. Cost: ~+12 MB binary (7.7 MB → ~20 MB), ~27 s extra cold build. |
| `layout-rs` 0.1.3 (DOT) | **Rejected.** Only +40 KB, but silently drops cluster subgraph frames/labels and ignores `style=rounded` / `fontname` / `fontsize`. Shipping a silently-degraded built-in conflicts with the project's no-silent-degradation stance; real Graphviz is a one-line install and stays the external-command path. |
| `plantuml-little` 1.2026.2-4 | **Rejected.** Not pure Rust despite the issue table: it links `graphviz-anywhere`, whose published crate ships an **empty** `prebuilt/` directory. `cargo build` fails out of the box asking for a native Graphviz static library (env var / manual archive drop / opt-in network download). Unusable as a zero-config built-in. |
| `warpdotdev/mermaid-to-svg` | **Rejected.** Not on crates.io (git dependency only); `merman` dominates on supply and coverage. |

## Behavior

- A fenced code block tagged `mermaid` with **no** `code_images:` entry for
  `mermaid` renders at build time through the built-in `merman` renderer and
  enters the existing code-image pipeline (cache, SVG intrinsic-size
  normalization, image fragment, dist asset).
- An explicit `code_images: mermaid: <command>` entry **overrides** the
  built-in (PR #251 behavior unchanged). External commands remain the escape
  hatch for Mermaid syntax the port does not support.
- All other tags are untouched: `dot`, `plantuml`, etc. still require an
  explicit `code_images:` entry; unknown tags remain parse-time errors.
- A `code_images:` value of bare `false` or `true` is a line-numbered
  frontmatter error ("built-in opt-out is not supported; provide an external
  command instead" + help). Without this guard, `mermaid: false` — the exact
  syntax Issue #252 floats as a future opt-out — would today execute
  `/usr/bin/false` and surface a confusing "command wrote empty stdout" error.
  Rejecting it now keeps the syntax reserved and the failure understandable.
  The actual opt-out stays deferred per the issue ("future, if needed").
- Documented behavior change: a deck that supplies a custom
  `mermaid.sublime-syntax` via `syntaxes:` and has no `code_images:` entry
  previously got highlighted source; it now gets a diagram. The resolution
  rule is the same one PR #251 established for external entries ("a
  code-image tag never reaches the highlighter"), extended to the built-in.
  Escape hatch: show source with a ````md wrapper fence (the existing idiom in
  `examples/code-images`), or override with an external command.

## Design: one typed resolution seam

Today two sites independently consult "is this tag a code-image tag":

- `parser.rs` (`parse_slide`): `code_images.entries.contains_key(language)`
  gates unknown-language validation.
- `code_images.rs` (`transform_fragment`): `config.entries.get(tag)` picks the
  command.

Adding the built-in at both sites as an `|| tag == "mermaid"` carve-out would
be the per-symptom bandaid. Instead, resolution moves into **one** method on
`CodeImagesConfig`, returning a typed renderer both consumers must
destructure:

```rust
// domain.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeImageRenderer<'a> {
    External(&'a CodeImageCommand),
    BuiltinMermaid,
}

impl CodeImagesConfig {
    /// The single resolution point: explicit entry > built-in > None.
    pub fn renderer_for(&self, tag: &str) -> Option<CodeImageRenderer<'_>> {
        if let Some(command) = self.entries.get(tag) {
            return Some(CodeImageRenderer::External(command));
        }
        (tag == "mermaid").then_some(CodeImageRenderer::BuiltinMermaid)
    }
}
```

- `parser.rs` gates unknown-language validation on
  `code_images.renderer_for(language).is_some()`.
- `transform_fragment` matches exhaustively on the resolved renderer:
  - `External(command)` → `runner.run(command, code_text)` (the `SvgRunner`
    trait is **unchanged** — it remains "how to execute an external command",
    so `CliSvgRunner` and every test `FakeRunner` compile as-is).
  - `BuiltinMermaid` → a pure function in `peitho-core`,
    `render_builtin_mermaid(code_text) -> Result<Vec<u8>, String>`, wrapping
    `merman::render::HeadlessRenderer::render_svg_sync`. Because dispatch
    happens on the enum *before* the trait is consulted, no future `SvgRunner`
    implementor can shadow or skip the built-in — the type makes the buggy
    path unrepresentable rather than relying on implementors remembering.
- Both output paths converge on the existing validate → normalize → cache →
  image-fragment tail, so Issue #261 normalization and the publish
  contamination check apply identically.

`merman = { version = "0.7", features = ["render"] }` lands in
`peitho-core/Cargo.toml`. The renderer is pure and deterministic, so core
tests exercise the real thing (no fake needed for the built-in arm).

### merman error mapping (no silent path)

- `Ok(Some(svg))` → continue into validate/normalize.
- `Ok(None)` (input not detected as any Mermaid diagram) → line-numbered
  build error: `code_images 'mermaid' failed: built-in renderer did not
  detect a mermaid diagram`, help: `fix the mermaid source, or set
  code_images.mermaid to an external command like mmdc -i - -o - -e svg`.
  Discovered during implementation: non-diagram input actually surfaces as
  `Err(HeadlessError::Parse(Error::DetectType(_)))`, whose `Display` echoes
  the entire cleaned fence source; that typed variant is therefore mapped to
  the same non-diagram message instead of embedding the raw error text.
- `Err(e)` (parse/render error; merman errors carry messages like
  `Unterminated node label (missing `]`)`) → line-numbered build error
  embedding `e`, same override help. This answers the issue's "fallback UX"
  question: the error names the escape hatch.

### Cache key

External entries keep their exact current key (hash of argv bytes + NUL
separators + code) so existing caches stay valid. The built-in key hashes a
discriminator that no external argv can produce. Review found that shlex
alone does not guarantee this — an empty first word (`'' peitho-builtin-…`)
or a YAML-escaped interior NUL could reproduce the discriminator byte
stream — so `code_images` command strings additionally reject empty shlex
words and interior NUL bytes at frontmatter parse (neither can ever
`execve`, so the rejection is a strict error-UX improvement and makes the
discriminator genuinely unreachable):

```text
sha256( b"\0peitho-builtin-mermaid\0" + CARGO_PKG_VERSION(peitho-core) + b"\0" + code )
```

Keying on the peitho-core version invalidates built-in cache entries on every
release, which is the correct staleness bound: the merman version is pinned by
Cargo.lock per released binary (merman exposes no version constant of its
own). Cache misses self-heal per the Issue #261 policy.

## Hardening added during review

- The built-in render call runs under `catch_unwind`: merman executes
  in-process against arbitrary mid-keystroke input during preview watch, and
  a panic must surface as the standard line-numbered Asset error (same
  contract as an external command crashing), never abort the build.
- The preview watch thread additionally catches unwinds from the watch body
  and routes them to the visible `preview watch error:` branch. Without
  this, any panic in a rebuild silently kills the watch loop while the
  server keeps serving stale content — the project's recorded
  silent-watch-death incident class. This is a class-seam guard, not
  merman-specific; the small `panic_payload_message` helper is deliberately
  duplicated in `peitho` rather than exported from `peitho-core`'s public
  API.
- `code_images` command strings reject empty shlex words and interior NUL
  bytes (see Cache key above).
- SVG validate/normalize errors carry the renderer context so built-in
  failures never blame a nonexistent external command.

## Out of scope

- `dot` / `plantuml` built-ins (rejected above; record reasoning on the issue).
- `code_images: mermaid: false` opt-out semantics (reserved via the
  `false`/`true` guard, not implemented).
- Theming of built-in output beyond what mermaid source-level directives
  (`%%{init: ...}%%`) already provide.
- Raster/PDF-specific merman features (`raster` feature stays off).

## TDD task list

Every step: red test first, then minimal green, in `peitho-core` unless noted.

1. **Resolver** (`domain.rs`): `renderer_for` returns `External` when an
   explicit `mermaid` entry exists; `BuiltinMermaid` for bare `mermaid`;
   `None` for other absent tags. Explicit entry for a non-mermaid tag still
   resolves `External`.
2. **Parser accepts bare ```mermaid** (`parser.rs`): a deck with no
   `code_images:` frontmatter and a `mermaid` fence parses (no
   unknown-language error). A bare ```plantuml fence still errors with the
   existing line-numbered unknown-language message (no accidental widening).
3. **Built-in transform** (`code_images.rs`): a `mermaid` fence with empty
   config becomes an image fragment; the cached SVG exists, is normalized
   (root has usable absolute `width`/`height` — merman emits
   `width="100%"` + `viewBox`, so this proves the #261 seam ran); alt text is
   `diagram (mermaid)`; `FakeRunner.calls == 0` (external runner not
   consulted).
4. **Override wins** (`code_images.rs`): with `code_images: mermaid: mmdc…`
   config, `FakeRunner.calls == 1` and its output is what lands in cache
   (current tests already pin most of this; add the assertion that the
   built-in did not run — distinguishable output).
5. **Built-in error paths** (`code_images.rs`): invalid mermaid source →
   `ErrorKind::Asset`, `line` = fence line, message contains the merman
   error, help names the `code_images.mermaid` override. Non-diagram text
   (if representable) → the `Ok(None)` error message.
6. **Cache behavior** (`code_images.rs`): second transform of the same
   source is a cache hit (file mtime unchanged / content stable). Built-in
   key differs from the key an external `mermaid` entry produces for the
   same source (override must not reuse built-in cache entries and vice
   versa).
7. **Frontmatter guard** (`parser.rs`): `code_images:\n  mermaid: false`
   (and `true`) → line-numbered error + help; other command strings
   unaffected.
8. **CLI integration** (`crates/peitho`): build a temp deck with a `mermaid`
   fence and no `code_images:` frontmatter → `dist/` contains the SVG asset
   and the slide references it. (Exercises the full
   `parse_deck_and_transform` path through `CliSvgRunner`.)
9. **Docs**: update `README.md`, `site/content/guide/frontmatter.md`,
   `site/content/guide/writing-decks.md`, `site/content/examples/code-images.md`,
   and `examples/code-images/deck.md` copy ("No diagram tool is built into
   Peitho" is no longer true — Mermaid is built in, everything else stays
   user-declared). Update the `code_images` invariant paragraph in
   `CLAUDE.md`.

## Verification beyond `cargo test`

- Full gates: 3× `cargo test --workspace`, clippy `-D warnings`, fmt check,
  bindings drift (`CodeImageRenderer` is core-internal, no ts-rs export —
  expect no drift), shell/preview drift.
- Real-browser E2E (Opus side; Codex sandbox cannot run Chrome): `peitho
  build` a zero-config mermaid deck, open in Chrome, confirm the diagram
  displays (merman uses `<foreignObject>` labels; probe already confirmed
  Chrome renders them inside `<img>`, re-verify in the built deck).
- PDF export of the same deck (`pdf_flatten.js` interaction — Issue #252
  explicitly asks; merman SVGs may flatten differently than mmdc output).
  Verify the printed page shows the diagram, including in Preview.app via
  `pdfseparate` + `sips`.
