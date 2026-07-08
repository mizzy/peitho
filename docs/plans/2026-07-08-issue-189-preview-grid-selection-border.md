# Issue 189: preview grid selection border

## Problem

In `peitho preview` grid mode, the selected tile uses a `3px` border while
unselected tiles use a `1px` border. Tiles keep `box-sizing: content-box`, so
the selected tile's outer size grows by 4 px in both axes. CSS grid row height
therefore depends on which tile is selected, and moving selection between rows
can shift an entire source or destination row.

## Plan

1. Add red Vitest coverage in `packages/peitho-present/test/preview.test.ts`
   that enters grid mode and verifies selected and unselected tiles have the
   same `1px` border.
2. Extend that coverage to verify the selected tile carries a non-layout
   selection affordance (`outline`) while unselected tiles do not.
3. Keep the existing `is-selected` class behavior and single-mode tile styling
   unchanged.
4. Change `applyGridLayout` so every grid tile always uses the unselected
   `1px` border, and draw selection with `outline: 3px solid #7dd3fc` plus a
   small `outline-offset`.
5. Rebuild `packages/peitho-present/dist/preview.js`; `dist/shell.js` should not
   change.

## Gates

- `cd packages/peitho-present && npm ci && npm run build && npm test && npm run typecheck`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --check`
- Verify `packages/peitho-present/dist/shell.js` is unchanged and
  `packages/peitho-present/dist/preview.js` is rebuilt.
