# Inline edit: a plain click on a link edits its block

## Problem

Preview inline editing (`docs/specs/2026-09-20-preview-inline-edit-design.md`)
starts an edit when a click in single mode reaches a `[data-peitho-src]`
element on the current slide — unless the click is inside an `<a>`, which kept
opening the link. A paragraph, list item, or table cell that is mostly (or
entirely) a link is therefore hard or impossible to edit by click: every click
lands on the link and opens it (external links in a new tab).

## Author decision (2026-09-25)

Reverses the spec's "links keep opening" rule for editable blocks:

1. In preview single mode, on the current slide, a **plain click** (no
   Meta/Ctrl/Alt/Shift) on a link **inside** an editable block does not
   navigate (`preventDefault`) and starts editing that block. The editor shows
   the block's Markdown, so the link URL is editable too.
2. A click with any modifier keeps the browser's behavior (Cmd/Ctrl+click opens
   the link in a new tab), as does a middle click (no `click` event).

Unchanged: links outside an editable block (layout HTML, a layout `<a>` wrapping
a slot), links in the overview grid and filmstrip, and every output other than
preview. Edit annotations exist only in the preview cache, so no other surface
can reach this path.

## Implementation

- `packages/peitho-present/src/preview.ts`: split the path walk out of
  `tryStartSlideEdit` so the tile click handler learns both the editable target
  and whether an `<a>` sits between the click origin and that target (inside the
  target's shadow tree). A plain click on such a link calls `preventDefault`
  and bypasses the click guard's interactive check; everything else follows the
  existing path.
- `interactiveTarget.ts` / `clickNavigationGuard.ts` are shared with present
  and the dist viewer and stay unchanged: this is a preview-only decision.

## Tests (vitest, `test/preview.test.ts`)

- `inline_edit_link_click_keeps_browser_behavior` is replaced by:
  - a plain click on the link inside an editable block prevents the default and
    starts editing that block with its `data-peitho-md`;
  - Meta, Ctrl, Alt, and Shift clicks do not prevent the default and start no
    edit.

## Docs

- `site/content/guide/cli.md` "Where it works": describe plain click edits,
  Cmd/Ctrl+click opens.
- `CLAUDE.md` inline-edit bullet: record the author decision.

## E2E (real Chrome)

Preview a deck with `[text](https://example.com)` in a paragraph: a plain click
enters editing with `[text](https://example.com)` and opens no tab; Cmd+click
opens the link in a new tab without editing.
