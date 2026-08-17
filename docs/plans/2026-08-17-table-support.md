# Table support in slide bodies (Issue #425)

Date: 2026-08-17
Issue: #425

## Problem

Pipe tables fail the build with `unsupported construct 'table'` — the parse
grammar has `ENABLE_TABLES` on purely so tables can be recognized and
rejected. Comparison tables are bread-and-butter slide content
(deck-writing report, 2026-08-16), and #424 left tables inside list items
as a deliberate intermediate hard error for this issue to resolve.

## Design

Tables follow the `List`/`Blockquote` fragment pattern:
`FragmentKind::Table` (unit variant), the fragment's markdown slice carries
the source, and rendering re-parses through the shared `push_html` path.
`BODY_MARKDOWN_OPTIONS` gains `ENABLE_TABLES`, which does double duty:

- top-level tables render as `<table>` (alignment markers become
  pulldown's `style="text-align: …"` attributes for free), and
- table markdown riding a container fragment (list item, blockquote)
  renders as a real table through the same option — so the container walk
  now **allows** Table events instead of erroring, resolving #424's
  documented intermediate state uniformly.

Parser: top-level `Start(Tag::Table)` opens a table fragment; the walk
consumes TableHead/TableRow/TableCell and inline content; images inside
cells are a line-numbered error (images route to image slots — mirrors the
container rule); everything else legal inline is legal in cells. Inside
containers, Table events join the allowed set (no error arm remains).

Mapping routes tables to body; `Accepts::Blocks` accepts `Table`. A table
inside `::: {reveal}` is one step (fragment-level default). The
`unsupported_construct` help gains "tables" in its rewrite list.

Plain text: cell text accumulates with a space between cells and a newline
between rows, so manifest body text stays readable.

Theme: `themes/base.css` (and the verbatim example copy
`examples/footnotes/css/base.css`) gain modest table rules — collapsed
borders, header weight, cell padding — themeable via custom properties
consistent with the blockquote rule.

## Tests (TDD order)

1. Parse: a pipe table produces a `Table` fragment with the source
   markdown.
2. Render: `<table>` with `<thead>`/`<tbody>`; alignment column renders
   the text-align style.
3. Emphasis/code spans inside cells render; image inside a cell is a
   line-numbered error.
4. Table inside a list item and inside a blockquote build and render as
   real tables (the #424 intermediate errors are gone).
5. Check: table mapped to a non-blocks slot is a check error.
6. Reveal: a table inside `::: {reveal}` contributes exactly one step.
7. Plain text: manifest body contains cell text with row separators, no
   pipes.
8. Help text: remaining unsupported constructs list tables among the
   rewrite targets.

## Non-goals

- No column-width/layout vocabulary in Markdown (pillar ①: design belongs
  to CSS).
- No `colspan`/`rowspan` (pipe tables cannot express them).
