# Reveal-gated footnotes (Issue #367)

## Bug

When a slide uses `::: {reveal}` and the revealed content contains footnote
references, the footnote block renders fully visible from step 0 while the
referencing content is still hidden. Repro: a deck whose two revealed
paragraphs reference `[^1]` / `[^2]` stamps `data-reveal-step="1"` /
`data-reveal-step="2"` on the paragraphs, but `.peitho-footnotes` and its
`<li>` entries carry no step attribute at all.

## Root cause

`FootnoteEntry` carries no reveal step. The parser records footnote reference
lines (`FootnoteAccumulator::record_reference`) and assigns `RevealSpan`s to
body fragments when a reveal fence dissolves, but never relates the two, so
`render_footnotes_block` has nothing to stamp and renders every entry
unconditionally visible. This is the missing half of the reveal design: the
implementation plan (`docs/plans/2026-07-31-incremental-reveal.md`, step
"Math/Footnotes arms") called for `data-reveal-step` on `.peitho-footnotes`,
but the shipped code made the `Footnotes` arm `unreachable!` because the
fragment itself can never sit inside a reveal fence — which is true, and is
exactly why the step must be derived from the *references*, not from the
fragment's own position.

## Decided behavior

A footnote entry becomes visible at the step where its referencing content
becomes visible:

- Entry step = **min** over all of the label's references of the referencing
  location's reveal step.
- If **any** reference of the label sits in non-revealed content, the entry is
  visible from the start (no step attribute) — a visible marker must never
  point at a hidden footnote.
- A reference inside a revealed **list** resolves to the containing top-level
  item's step (`span.start + item_index`), not the list's first step.
- The `.peitho-footnotes` wrapper carries `data-reveal-step` = min entry step
  **only when every entry has a step** (so the border/top-rule hides until the
  first footnote appears); if any entry is always-visible the wrapper stays
  unstamped.
- References that resolve to no revealed fragment (including references inside
  footnote definition bodies) are always-visible. Definitions are not body
  fragments, so they never match a revealed range by construction.

Decks without reveal fences produce entries with no step everywhere → rendered
HTML is byte-identical to today (preserves the "non-reveal decks build
byte-identical" invariant). PDF/preview/lint/dist continue to show final state
because only the present shell toggles `data-reveal-hidden`; the shell's
`applyRevealState` already queries `[data-reveal-step]` generically, so **no
TypeScript change is needed**.

`visibility:hidden` keeps layout space and keeps `<ol>` counter numbering
stable, so hidden entries do not renumber or reflow the block.

## Implementation

All in `crates/peitho-core`. No bindings, manifest, or shell changes.

### 1. `domain.rs` — `FootnoteEntry.reveal_step`

- Add private field `reveal_step: Option<usize>` (step values are ≥ 1 by
  construction, same convention as `RevealSpan.start`).
- `FootnoteEntry::new(number, label, markdown, line, reveal_step)` — extend the
  existing constructor so every call site must decide (no default that lets a
  future caller silently drop the step). Update the existing test call sites in
  `domain.rs`, `check.rs`, `mapping.rs`.
- Accessor `pub fn reveal_step(&self) -> Option<usize>`.

### 2. `render.rs` — offset-aware shared walker

The "one parse-time source for step counting" invariant means item identity
for lists must come from the same walker the renderer uses. Refactor:

- Add `walk_body_markdown_list_items_with_ranges(markdown, visit: impl
  FnMut(Event, Range<usize>, bool))` built on
  `Parser::new_ext(..).into_offset_iter()` with the existing depth-tracking
  logic moved inside it.
- Re-implement `walk_body_markdown_list_items` as a delegation that drops the
  range, so exactly one depth-tracking implementation exists.

### 3. `parser.rs` — resolve reference lines to steps at slide end

- `FootnoteAccumulator`: record **all** reference lines per label (new
  `BTreeMap<String, Vec<usize>>`), keeping the existing first-reference-only
  `references` vec for ordering/numbering.
- At the `into_fragment` call site (slide end, after all fragments carry final
  reveal spans), build a resolver from `&fragments` and pass it in (e.g.
  `into_fragment(step_for_line: impl Fn(usize) -> Option<usize>)`):
  - Collect top-level fragments with `reveal_span()`, kinds
    `Heading`/`Paragraph`/`List` only (Code/Math/Image cannot contain footnote
    references; SlotGroup/Footnotes cannot be revealed). Line range =
    `[fragment.line(), fragment.line() + markdown().lines().count() - 1]` —
    Heading/Paragraph/List all store the exact source slice.
  - Non-List kinds: any reference line in range → `span.start`.
  - List: walk the fragment markdown with the offset-aware walker; each
    top-level `Start(Tag::Item)`'s relative line = newline count in
    `markdown[..offset]`; reference line maps to the last item start ≤ it →
    `span.start + item_index`.
- `into_fragment` computes each entry's `reveal_step`: `Some(min)` when every
  recorded reference line resolves to `Some(step)`, else `None`.

### 4. `render.rs` — stamp the block

In `render_footnotes_block`:

- `<li data-reveal-step="{step}">` for entries with a step; plain `<li>`
  otherwise.
- `<div class="peitho-footnotes" data-reveal-step="{min}">` only when all
  entries carry a step.

## Tests (TDD order)

Parser (`parser.rs`):

1. Reference in each of two revealed paragraphs → entries step `Some(1)`,
   `Some(2)` (red first: current entries have no step).
2. Reference outside any reveal fence → `None`.
3. Same label referenced both inside a revealed paragraph and in a
   non-revealed paragraph → `None`.
4. References in different top-level items of one revealed list → per-item
   steps `span.start + index`.
5. Reveal fence after a non-revealed paragraph containing the reference →
   `None` (range resolution doesn't leak forward/backward).

Render (`render.rs`):

6. Footnote block HTML carries `data-reveal-step` on stamped `<li>`s and the
   min on `.peitho-footnotes` when all entries are stamped.
7. Mixed stamped/unstamped entries → wrapper has no `data-reveal-step`.
8. Non-reveal deck → footnote block byte-identical to current output (existing
   snapshot-style assertions must not change).

E2E: build the repro deck and assert the emitted slide HTML; the shell
mechanism (`data-reveal-hidden` toggling) is unchanged and already covered by
vitest, so no browser-side change to verify beyond a manual present sanity
check.

## Gates

Standard: `cargo test --workspace` ×3, clippy `-D warnings`, `cargo fmt
--check`, bindings drift check (expected no-op), npm build/test/typecheck +
dist drift checks (expected no-op).
