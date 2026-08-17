# Route fenced code inside containers through the real pipeline (Issue #437)

Date: 2026-08-17
Issue: #437

## Problem

Fenced code that sits inside a container fragment (list item, blockquote)
rides the container's markdown and is re-rendered by `push_html` — it never
becomes `FragmentKind::Code`, so it bypasses unknown-language validation,
syntect highlighting, code line emphasis, and `code_images` dispatch. For
lists this is a long-standing silent gap (` ```notalang ` in a list builds
clean; ` ```rust ` renders with zero `hl-*` spans). #424 closed the
blockquote side with a hard error as a stopgap; this issue makes both
containers uniform with real support.

## Design

**Parse-time validation** (single seam, both containers): while a container
is open, fenced-code events are validated exactly like top-level fences:

- unknown language tag → the existing line-numbered `unknown code language`
  error;
- a tag that resolves to a `code_images` renderer (built-in mermaid/math/
  embed or a frontmatter-declared external command) → named line-numbered
  error: these produce image/embed fragments that cannot ride inside a
  container ("move the block to the top level");
- a positional `{…}` emphasis spec on a container fence → named
  line-numbered error (emphasis is a fragment-level feature; supporting it
  inside containers would need per-fence step spaces — out of scope, loud).

The #424 "code block inside a blockquote" hard error is replaced by this
support; lists lose their silent path the same way (a deck with a bad tag
inside a list now errors — the breaking change this issue exists to make,
loud and line-numbered).

**Render-time highlighting** (single seam): the container-markdown
re-render intercepts `CodeBlock` events and emits the same
syntect-highlighted `hl-*` span HTML as top-level code (shared helper with
the existing code path — one highlighting authority, not a second copy).
Plain (untagged) fences stay unhighlighted, exactly like top level.

**Plain text**: container code text stays in the body text stream (it is
part of the item/quote flow, unlike top-level code which routes to the
`code` field) — unchanged from today, documented here as deliberate.

## Tests (TDD order)

1. ` ```rust ` inside a list item renders `hl-*` spans identical in class
   vocabulary to the same fence at top level; same inside a blockquote.
2. ` ```notalang ` inside a list item and inside a quote → line-numbered
   unknown-language error (was silent for lists).
3. ` ```mermaid ` inside a container → named code_images error with line.
4. ` ```rust {2} ` inside a container → named emphasis error with line.
5. Untagged fence inside a container renders as plain code (no spans), and
   the fence content is never markdown-parsed.
6. The #424 blockquote-code error tests are replaced by support tests;
   existing container tests stay green.
7. Reveal: a list with a code fence inside `::: {reveal}` keeps its step
   counting unchanged.

## Non-goals

- No emphasis/steps inside containers (loud error instead).
- No code_images rendering inside containers (loud error instead).
- No change to top-level code handling or the `code` slot contract.

## Amendments (2026-08-17, after adversarial review)

- **Theme reach**: the built-in theme's `hl-*` color rules were scoped
  `.slot-code .hl-*`, so container code carried classes but no colors (and
  none of the code box styling) — visually indistinguishable from before
  the feature. The color rules widen to slide scope (the `hl-` prefix
  already guarantees no collision — that is its documented purpose), and
  body-slot `pre` gains the same box styling / `pre-wrap` treatment as the
  code slot. `examples/footnotes/css/base.css` stays byte-identical.
- **Div-marker scanner symmetry**: the line-first `:::` scanner missed a
  fence OPENED under a list/quote prefix (`- ```rust`) but its trimmed
  CLOSER matched as an opener, desyncing fence state and silently
  swallowing later `:::` markers as fence interior. Opening detection now
  strips container prefixes (`>`, list markers) symmetrically with the
  closer's whitespace handling, closing the mangling path.
- **Typed parse→render carry**: container fences validated at parse ride
  the fragment as an ordered list of validated languages; render consumes
  them positionally from that single seam instead of re-parsing fence info
  under a three-site convention. A count/order mismatch is one named
  internal error.
- **Line numbers**: container-code errors derive from each fragment's own
  line, not run-start plus joined-buffer newlines.
- **Indented code inside a blockquote is accepted** as an untagged plain
  block (uniform with lists), pinned by test — replacing #424's stopgap
  rejection wholesale, not just for fenced blocks.
- Trailing-newline trim lives in the shared highlight seam so container
  and top-level output match byte-for-byte; error kinds on the container
  path mirror the top-level highlight path's conventions.
- Restored coverage: bare `mermaid` in a blockquote, `> - ```rust`
  nesting, and wrapper-class (`language-*`) assertions alongside the
  hl-vocabulary parity test.
