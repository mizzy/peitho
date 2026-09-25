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
`slide_edit` re-derives from the source string. `EditableBlockKind::Heading`
gains the form:

```rust
pub(crate) enum EditableBlockKind {
    Paragraph,
    Heading(HeadingForm),
    TightListItem,
    TableCell,
}

pub(crate) enum HeadingForm {
    /// `block` is the heading's own line (from its `Start(Heading)` event
    /// range, trailing line ending excluded), including the `#` run and any
    /// closing `#` sequence.
    Atx { level: HeadingLevel, block: SourceSpan },
    Setext,
}
```

`editable_inline_ranges` records the form at `Tag::Heading`: pulldown-cmark
does not expose ATX vs setext, so the parser classifies it as ATX when the
heading event's source, after at most three leading spaces, begins with `#`.
The block range is shifted into combined-source coordinates alongside the
inline span in `with_source_provenance`. The renderer shares the walker but
matches annotations by range only, so it is unaffected.

**One rewrite seam.** In `rewrite_block`, after the replacement is normalized:

- span kind `Heading(Atx { level: H1 | H2, block })` and the replacement
  contains `\n` → the candidate replaces `block` (not the inline span) with
  `replacement + "\n" + underline`, where the underline is `====` / `----`
  (four characters: `---` alone is the slide separator);
- `Heading(Atx { level: H3..=H6, .. })` with `\n` → refusal
  "a heading of level N cannot span lines; only H1 and H2 have a multi-line
  (setext) form";
- everything else → the unchanged inline-span splice.

**The postcondition is not relaxed.** The existing reparse comparison already
proves the conversion safe: same slide count, sections, other slides, fragment
kinds and heading level, same editable block count, every other span
byte-identical, and the edited span's text equal to the replacement. The only
change is that the editable-kind comparison compares kinds ignoring
`HeadingForm` (an ATX→setext edit is expected to change the form). A heading
inside a container (`> # Title`) converts to lines without the `>` prefix, so
the reparse changes its structure and it is refused by the comparison — no
special case.

**Keys.** A derived key follows the heading text as it does today
(`target_key_change_allowed`); an explicit key is preserved.

**Shell.** No change: it already sends the editor's `textContent`, newline
included.

## Tasks

1. Parser: `HeadingForm` on `EditableBlockKind::Heading`; ATX detection and
   block range. Tests: ATX H1 / H2 / H3, ATX with closing `#`s, indented ATX,
   setext `=`/`-`, heading inside a blockquote.
2. `slide_edit::rewrite_block`: conversion and H3+ refusal; form-insensitive
   kind comparison. Tests: `# A` + `A\nB` → `A\nB\n====`; `## A` → `----`;
   `### A` refused with the level message; setext edit keeps setext; removing
   the newline from a setext heading keeps setext; closing-`#` ATX converts;
   blockquote heading refused; derived key changes, explicit key preserved;
   CRLF origin and BOM round-trip; converted H2 underline is not taken as a
   slide separator (slide count unchanged).
3. Guide (`site/content/guide/cli.md`, "Editing slide text in preview"): a
   newline in an H1/H2 heading switches it to the multi-line (setext) form,
   H3–H6 refuse it, and making the break visible is `white-space: pre-line` in
   layout CSS. Update the CLAUDE.md inline-edit bullet.
4. E2E in Chrome: `peitho preview` on a copy of an example deck, click an H1,
   Shift+Enter, Enter; the file holds the setext form and the preview reloads
   on the same slide.
