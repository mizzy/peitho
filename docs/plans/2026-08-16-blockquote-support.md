# Blockquote support in slide bodies (Issue #424)

Date: 2026-08-16
Issue: #424

## Problem

`>` blockquotes fail the build with `unsupported construct 'blockquote'`
(help still says "for milestone 1"). Quoting other material is a common
slide need with real-world demand (deck-writing report, 2026-08-16).

## Design

Blockquotes become a first-class block fragment, mirroring how `List`
works today: `FragmentKind::Blockquote` (unit variant), the fragment's
markdown slice carries the source, and rendering re-parses that slice with
`BODY_MARKDOWN_OPTIONS` through the existing `push_html` path — so inline
content (emphasis, strong, strikethrough, links, code spans, footnote
references) and nested block content (paragraphs, lists, nested quotes,
fenced code) work without new rendering machinery.

- **Parser**: `Tag::BlockQuote` leaves the unsupported list. The walk
  tracks blockquote depth like it tracks lists; inner constructs are
  validated with the same rules as inside lists — images inside a
  blockquote are a line-numbered error (images route to image slots),
  tables stay `unsupported construct 'table'` (until #425), footnote
  *definitions* inside a quote stay rejected as today, inline/HTML events
  keep their existing handling. No arm becomes `_ => {}`.
- **Mapping / check**: convention mapping routes blockquotes to the body
  (blocks) slot. `Accepts::Blocks` accepts `Blockquote`; every other
  accepts kind rejects it with the existing named-error shape.
- **Reveal**: a top-level blockquote inside `::: {reveal}` is one step,
  falling out of the existing fragment-level span behavior (lists keep
  their per-item steps; blockquotes are not lists).
- **Plain text / manifest / keys**: `plain.rs` collects the quote's inner
  text through the existing event walk (BlockQuote tags fall through, text
  events accumulate) — verify with a test.
- **Error text**: `unsupported_construct`'s help drops the stale "for
  milestone 1" and now reads
  "rewrite this slide using headings, paragraphs, lists, blockquotes, or
  fenced code blocks" (table keeps pointing at this help until #425).
- **Theme**: `themes/base.css` gains a modest blockquote rule (left
  border + muted foreground, spacing consistent with existing vars); the
  example copy `examples/footnotes/css/base.css` mirrors it if that file
  is a verbatim theme copy.

## Tests (TDD order)

1. Parse: `> quoted` produces a `Blockquote` fragment with the source
   markdown; multi-paragraph and nested quotes parse.
2. Render: `<blockquote>` with inner `<p>`; nested list and fenced code
   inside a quote render; strikethrough/emphasis inside a quote render.
3. Image inside a blockquote → line-numbered error naming the construct.
4. Table inside a blockquote → still `unsupported construct 'table'`.
5. Check: blockquote mapped to a non-blocks slot (e.g. explicit slot with
   accepts="inline") is a check error.
6. Reveal: a blockquote inside `::: {reveal}` contributes exactly one step.
7. Plain text: manifest body text contains the quote's inner text without
   `>` markers.
8. Help text: unsupported-table error carries the new help wording (no
   "milestone 1").

## Non-goals

- Table support (#425).
- Attribution/citation syntax (`<cite>`), callout/admonition styling.

## Amendments (2026-08-17, after adversarial review)

- **Fenced code inside a blockquote is a line-numbered build error** for v1
  ("move the code block out of the quote"). The first cut swallowed it into
  the quote's markdown, bypassing unknown-language validation, syntect
  highlighting, emphasis errors, and code_images dispatch — silent behavior
  the project forbids. Lists have the same pre-existing gap; that sibling
  stays untouched here (it pre-exists on main independent of this feature)
  and gets its own issue covering both containers uniformly.
- **HTML comments inside container fragments no longer leak into rendered
  HTML.** A comment inside a blockquote (or list — same pre-existing bug)
  was collected as a speaker note AND kept in the fragment markdown, so the
  note text shipped in dist/, violating the notes-never-enter-dist
  invariant. Fixed once at the render seam: comment HTML events are dropped
  when container markdown is re-rendered. Covered for both containers.
- **No GFM alert processing.** The first cut enabled the GFM blockquote
  extension, which consumed `[!NOTE]` markers and stamped `markdown-alert-*`
  classes no CSS defines — the marker text silently vanished. Admonitions
  stay a non-goal: without the extension the marker renders literally,
  which is visible and honest.
- **Thematic break inside a blockquote is a named error** ("thematic break
  inside a blockquote"), not the misleading "thematic break inside slide"
  whose help recommends blockquotes.
- **Tables inside list items become hard errors** (previously silently
  rendered as literal pipe text). Accepted: #425 lands real table support in
  the same release, so the intermediate state never ships.
- **Container tracking is one seam**: a single container-state
  (kind + depth + start) with one shared close-container path, instead of
  parallel list/blockquote copies of the guard and close logic.
- Plain-text extraction inserts separators at block boundaries inside a
  quote (consecutive quoted paragraphs no longer concatenate).
