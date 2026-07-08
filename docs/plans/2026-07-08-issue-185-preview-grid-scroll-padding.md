# Issue 185: preview grid scroll padding

## Problem

In `peitho preview` grid mode, selected tiles can be scrolled exactly against
the top or bottom edge of the scroll viewport. The selected tile remains
visible, but there is no breathing room to show adjacent rows.

## Plan

1. Add red Vitest coverage in `packages/peitho-present/test/preview.test.ts`
   that grid mode sets `scroll-padding-top` and `scroll-padding-bottom` on the
   preview root.
2. Extend that coverage to switch back to single mode and verify the scroll
   padding is removed.
3. Set the grid scroll padding in `applyGridLayout` to match `GRID_PADDING`
   (`24px`) so existing `scrollIntoView({ block: "nearest" })` behavior can use
   the same edge inset as the grid padding.
4. Clear only those scroll-padding properties in `applySingleLayout`.
5. Rebuild `packages/peitho-present/dist/preview.js`; `dist/shell.js` should not
   change.

## Gates

- `cd packages/peitho-present && npm ci && npm run build && npm test && npm run typecheck`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --check`
- Verify `packages/peitho-present/dist/shell.js` is unchanged and
  `packages/peitho-present/dist/preview.js` is rebuilt.
