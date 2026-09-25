# One `<pre>` per fenced code block in a code slot (Issue #666)

## Problem

`render_code_slot` (`crates/peitho-core/src/render.rs`) merges code fragments:

- Non-reveal path: every fragment is rendered, `join("\n")`ed, and wrapped in
  one `render_code_block`; `language` becomes `None` when there is more than
  one fragment.
- Reveal path: `flush_code_run` merges consecutive non-revealed fragments into
  one `<pre>`, while each revealed fragment gets its own.

Two fences therefore become one visual panel, and whether they stay separate
depends on whether `reveal` is used. Markdown already expresses the boundary;
the renderer erases it, and layout CSS cannot restore it.

## Root cause

The renderer has a "code run" concept (merge adjacent code fragments) that no
design record asks for. The fix is to remove that concept, not to special-case
the reveal/non-reveal split: one fragment is one `<pre>`, always.

## Change

- `render_code_slot` becomes a single loop over fragments (no separate
  non-reveal fast path): for each fragment, `append_code_separator`, then
  either `render_revealed_fragment` (revealed) or
  `render_code_block(class_name, &render_code_fragment(..)?, fragment.language(), None)`.
- Delete `flush_code_run` and the `code_run` accumulator.
- A slot holding one fragment renders byte-identically to today (the separator
  is only emitted between blocks), so single-fence decks are unchanged.

## Tests (TDD)

1. Two fences (`rust` + untagged) in a `code` slot without reveal render as two
   `<pre class="slot-code">` elements joined by `\n`; the first carries
   `class="language-rust"`, the second has no language class.
2. Reveal path: two consecutive non-revealed fences next to a revealed one
   render as three `<pre>` elements.
3. Existing single-fence output is unchanged (existing snapshot tests).

Any example-deck snapshot that changes must be because that deck puts several
fences in one code slot; inspect and update deliberately.
