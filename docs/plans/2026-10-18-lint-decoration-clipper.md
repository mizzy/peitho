# Lint: slot-free clippers are layout decoration (Issue #716)

## Problem

A layout that clips a decorative element bleeding off the slide edge with a
dedicated container (`.mark { position: absolute; inset: 0; overflow: hidden }`
holding a huge `::before` numeral) fails `peitho lint` with
`content overflows a container`, and a start-side bleed (`left: -24px`) inside
that container also feeds the slide-box bounds. The geometry is real, but no
deck content can be lost there: content only enters a slide through slots.

## Decision

Structural signal, no opt-in (the issue's preferred direction; the three
lenses agree: it needs no author convention, it cannot be put on an element
that holds deck content, and it lives at the one place lint decides what a
clipper hides).

A **decoration clipper** has no `.slot-*` element on itself, on any ancestor up
to the slide, or anywhere in its subtree. Lint therefore treats everything
inside it as layout-authored. The exemption applies independently on each axis
and only when that axis's computed overflow is `hidden` or `clip`.
`overflow: clip` qualifies only when computed `overflow-clip-margin` is `0px`.
`overflow: auto` and `scroll` keep reporting because they paint a visible
scrollbar. An unused slot renders nothing, so a clipper around an empty slot is
decoration too; a layout class that merely starts with `slot-` counts as a slot
(the conservative direction).

`lint_measure.js`:

- `measureSlotOverflows` skips a decoration clipper only on an axis whose
  overflow is `hidden` or `clip` (what it hides is not content the audience
  would miss).
- In `contentBounds`, a descendant contributes nothing on an axis when the
  first clipper in its containing-block chain is a decoration clipper that
  hides that axis. On every other axis it keeps today's clipped-element
  behavior and contributes its start edge only. The clipper's own rect still
  counts on all four edges, so a decoration clipper whose own box escapes the
  slide is still reported: the clipper's box is the declared paint region and
  must stay inside the slide.
- The no-slot predicate is cached per clipper for one slide measurement, so a
  clipper containing many descendants is scanned only once.
- A positioned descendant whose containing-block chain bypasses the clipper is
  not clipped by it and keeps counting, unchanged.

## Not covered (by design)

- Decoration clipped by the slide itself (the slide holds slots, so it is not
  a decoration clipper). The fix for that layout is to wrap the decoration in
  a slot-free clipper; the guide says so.
- `contain: paint` clippers stay invisible to lint, unchanged.
- A layout script can move deck text into a slot-free clipper. Lint accepts the
  result because slot classes are its only structural signal for deck content.
- Layout-authored text can be truncated inside a slot-free clipper without an
  ellipsis note. This is accepted for the same reason: lint judges deck content,
  and slot classes are its only structural signal.

## Tests (Chrome, `crates/peitho/tests/lint.rs`, `#[ignore]` like the rest)

1. Issue repro (`.section-mark` inset-0 clipper with a `right: -24px;
   bottom: -130px` 560px `::before`): exit 0, no warnings.
2. Same clipper, decoration pulled out on the start side (`left: -24px;
   top: -130px` on a real child element): exit 0, no slide-box warning.
3. A separate layout with a fixed-height `.body-clipper` around the body slot:
   overflowing deck content still reports overflow from the `body` slot.
4. A decoration clipper whose own box escapes the slide: still
   `overflows the slide box`.
5. A slot-free `overflow: auto` clipper: still `overflows a container`.
6. A slot-free clipper with `overflow-x: clip` and `overflow-y: visible`:
   decoration escaping the top still `overflows the slide box vertically`.
7. A slot-free `overflow: clip` clipper with a nonzero
   `overflow-clip-margin`: still `overflows the slide box`.
8. An absolutely positioned descendant whose containing block is the slide
   bypasses its static decoration clipper: still `overflows the slide box`.

Docs: `site/content/guide/cli.md` lint section.
