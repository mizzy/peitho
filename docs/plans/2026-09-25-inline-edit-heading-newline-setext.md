# Inline edit: a newline in an ATX heading becomes a setext heading

## Problem

Preview inline editing (`docs/specs/2026-09-20-preview-inline-edit-design.md`)
lets Shift+Enter insert a newline into the edited block. For a heading written
as ATX (`# Title`), the newline cannot survive: an ATX heading is one line, so
`# Title\nline two` reparses as a heading plus a paragraph and the save is
refused ("block structure"). The only way to put a two-line title on a slide is
a setext heading, which the author must write in the editor:

```markdown
IaCツールをつくってわかった
使う側からは見えなかった難しさ
======
```

The source newline survives into the rendered heading, and a layout that wants
it visible sets `white-space: pre-line` on the slot (real deck:
`decks-yapc-tokyo-2026`, `css/overrides.css`). How the newline *looks* is the
layout's job (pillar 1); this change is only about the source form.

## Author decisions (2026-09-25)

1. A newline inserted into an ATX **H1/H2** heading rewrites the whole heading
   to setext form: the edited text, a newline, then an underline (`=` for H1,
   `-` for H2). No `\` hard break is added.
2. A newline in an ATX **H3–H6** heading is refused (setext has no such levels).
3. A setext heading stays setext, even when an edit removes its last newline.
   No setext→ATX conversion.

## Design

**The parser stays the only authority.** Whether a heading is ATX, its level,
and the source range of its whole line are parser evidence, not something
`slide_edit` re-derives from the source string. `EditableSpan` gains an
optional `AtxHeading { level, line }`:

```rust
pub struct EditableSpan {
    source: SourceSpan,
    kind: EditableBlockKind,
    atx_heading: Option<AtxHeading>,
}
```

The only way to attach it is `EditableSpan::atx_heading(source, heading)`,
which fixes `kind` to `Heading`, so the evidence can never ride a paragraph or
list item. `EditableBlockKind` itself is unchanged, which keeps the postcondition's
kind comparison form-insensitive with no special case.

`editable_spans_for_markdown` records the evidence from the heading's opening
event: pulldown-cmark does not expose ATX vs setext, so a heading is ATX when
its event source, after at most three leading spaces, begins with `#`. `line`
runs to the end of the first line (`\r` excluded) and is shifted into
combined-source coordinates in `with_source_provenance`. The shared
`editable_inline_ranges` walker and the renderer are untouched.

**One rewrite seam.** In `rewrite_block`, after the replacement is normalized:

- span carries `AtxHeading` with level 1 or 2 and the replacement contains
  `\n` → the candidate replaces the heading's `line` (not the inline span)
  with `replacement + "\n" + underline`, where the underline is `====` /
  `----` (four characters: `---` alone is the slide separator);
- level 3–6 with `\n` → refusal "an HN heading cannot span lines; only H1 and
  H2 have a multi-line (setext) form";
- everything else → the unchanged inline-span splice.

**The postcondition is not relaxed.** The existing reparse comparison already
proves the conversion safe: same slide count, sections, other slides, fragment
kinds and heading level, same editable block count, every other span
byte-identical, and the edited span's text equal to the replacement. A heading
inside a container (`> # Title`) converts to lines without the `>` prefix, so
the reparse changes its structure and it is refused by the comparison — no
special case.

**The write scope comes from the splice.** `rewrite_block` returns
`BlockRewrite { source, replaced }`, where `replaced` is the range it actually
spliced (the heading line for a conversion, the inline span otherwise), and
`write_preview_slide_edit` passes that range as the origin writer's
`EditableBlock` scope. Found in E2E: with the scope restated as the inline span,
the writer's outside-scope check saw the removed `# ` and answered 409 — the
core unit tests could not see it. One statement of the range means the two can
never disagree again.

**Keys.** A derived key follows the heading text as it does today
(`target_key_change_allowed`); an explicit key is preserved.

**Shell.** No change: it already sends the editor's `textContent`, newline
included.

## Tasks

1. Parser: `AtxHeading` evidence on ATX heading spans; ATX detection and
   line range. Tests: ATX H1 / H2 / H3, ATX with closing `#`s, indented ATX,
   setext `=`/`-`, heading inside a blockquote.
2. `slide_edit::rewrite_block`: conversion and H3+ refusal. Tests: `# A` +
   `A\nB` → `A\nB\n====`; `## A` → `----`; `### A` refused with the level
   message; setext edit keeps setext; removing the newline from a setext
   heading keeps setext; closing-`#` ATX converts; blockquote and list-item
   headings refused; derived key changes, explicit key preserved; CRLF origin
   and BOM round-trip; converted H2 underline is not taken as a
   slide separator (slide count unchanged).
3. Guide (`site/content/guide/cli.md`, "Editing slide text in preview"): a
   newline in an H1/H2 heading switches it to the multi-line (setext) form,
   H3–H6 refuse it, and making the break visible is `white-space: pre-line` in
   layout CSS. Update the CLAUDE.md inline-edit bullet.
4. Server: `write_preview_slide_edit` scopes the origin write by
   `BlockRewrite::replaced`. Tests: top-level and included ATX headings are
   written as setext through `write_preview_slide_edit`.
5. E2E in Chrome: `peitho preview` on a copy of an example deck, click an H1,
   Shift+Enter, Enter; the file holds the setext form and the preview reloads
   on the same slide.
