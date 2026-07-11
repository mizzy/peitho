# breaks frontmatter option

Issue: #245.

## Motivation

CommonMark treats a single newline inside a paragraph as a space, so
`first\nsecond` renders as `first second`. In slides, authors often want
a single newline to become a hard break — line-broken lists, haiku-style
bullet items, layout-sensitive rhythm. Today the only way to force a
break is `<br>` HTML or two trailing spaces; both are non-obvious in
Markdown.

k1LoW/deck exposes this via a `breaks` frontmatter key that flips
pulldown-cmark's `ENABLE_HARDBREAKS`. peitho already accepts flat
`key: value` frontmatter with strict validation; adding `breaks: true`
is a natural fit.

## Design

- Add a `breaks: bool` field to `DeckSettings` (default `false`).
- Wire it into `DeckFrontmatter` deserialization (validated by
  `#[serde(deny_unknown_fields)]`; `breaks: xyz` becomes a
  line-numbered YAML parse error already).
- The line-tracking key list (`frontmatter_key_lines`, `frontmatter_help`,
  the CLAUDE.md-documented supported-keys help string) all learn the new
  key.

### The propagation surface (both grammars + render)

peitho's Markdown pipeline has three `Parser::new_ext` entry points that
matter for hard-breaks:

1. `parser_options()` — metadata-enabled grammar used only for the
   leading frontmatter block and inside `parse_slide` (event loop that
   produces `SourceFragment`s).
2. `slide_split_options()` — legacy grammar used to split slides on
   `---` rules.
3. `render_block_slot` in `render.rs` — re-parses fragment markdown to
   produce the final `<div class="slot-*">…</div>` HTML. **This is where
   `<br>` actually appears in the output.**

Two other `Options::empty()` sites are intentional and stay
unchanged:

- `render_heading_inline` — extracts inline events under a heading; a
  heading cannot contain a hard-break in CommonMark.
- `plain.rs::body_fragment_text` — produces manifest search text where
  soft and hard breaks are both normalized to a single space.

### Why only render needs the flag

pulldown-cmark 0.13 (workspace-pinned) does **not** expose an
`ENABLE_HARDBREAKS` option — the feature is not in `Options`. The
`SoftBreak` / `HardBreak` distinction is only observable when Markdown
is rendered to HTML; at parse time the two events flow through the
event stream unchanged, and every downstream consumer treats them the
same way (`plain.rs::body_fragment_text` normalizes both to a single
space; the parse-side event loop in `parser.rs::parse_slide` does not
inspect them at all).

The block-slot HTML step in `render.rs::render_block_slot` is where
`<br>` actually enters the output. That is the only site the flag
needs to reach, and the implementation applies the semantics by
mapping `SoftBreak → HardBreak` on the render-time event stream. This
is behaviorally identical to what `ENABLE_HARDBREAKS` would do if it
existed on this pulldown-cmark version.

### Threading through the pipeline

- `breaks` is stored on `DeckSettings` (single source of truth).
- `render_deck` destructures `DeckSettings` from the deck and passes
  `settings.breaks()` down through `render_slide` → `render_slot` →
  `render_block_slot`.
- `parser_options()` and `slide_split_options()` stay zero-argument.
  Adding a dead `breaks: bool` parameter to them would be a bandaid
  disguised as a type-safety seam — the parameter would be `_breaks`
  everywhere it appeared, giving the shape of enforcement without any
  actual effect.

No new lookup or fallback path is introduced; `breaks` rides on
`DeckSettings` from parse to render.

## TDD scope

Test-first, one behavior at a time:

1. Frontmatter parses `breaks: true` into `DeckSettings::breaks() ==
   true`, and `breaks: false` / missing key → `false`.
2. `breaks: not-a-bool` → line-numbered YAML parse error with a help
   line pointing at supported keys.
3. Unknown-key help mentions `breaks` alongside existing keys
   (regression guard on the docs string).
4. When `breaks: true`, a slide body of `first\nsecond` renders with a
   `<br>` between the two words in the body slot's HTML.
5. When `breaks: false` (or omitted), the same slide renders as
   `first\nsecond` collapsing to a single space (existing CommonMark
   behavior — regression guard).
6. `breaks: true` does not change slide splitting: a source with
   `first\nsecond\n\n---\n\n# Next` still yields two slides.
7. `plain.rs::body_fragment_text` continues to normalize both soft and
   hard breaks to spaces regardless of `breaks` (manifest search text
   stays stable).

## Non-goals

- No global config file (peitho's zero-config stance stands).
- No per-slide override — the flag is deck-wide, matching k1LoW/deck.
- No change to existing inline `<br>` HTML handling (both work).

## Bindings

`DeckSettings` is not exported to TS via ts-rs (only manifest types
are), so `bindings/` is unchanged.

## Verification

Standard gates: `cargo test --workspace` (3×), `cargo clippy
--workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
`git diff --exit-code bindings/`, plus the `packages/peitho-present`
build + tests + typecheck + shell/preview drift check. Also render a
tiny deck locally with `breaks: true` and inspect the emitted HTML to
confirm `<br>` appears.
