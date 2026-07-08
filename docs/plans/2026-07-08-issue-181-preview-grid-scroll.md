# Issue 181: preview grid follows the selected tile

## Problem

In `peitho preview` grid mode, keyboard navigation can move the selected tile
outside the scroll viewport. The selection state updates, but the grid container
does not scroll, so the visible selection outline is lost on decks with many
slides.

## Plan

1. Add red Vitest coverage in `packages/peitho-present/test/preview.test.ts`
   for grid arrow navigation, grid entry, and single-mode navigation. Tests will
   attach a spy to tile-level `scrollIntoView` because jsdom does not implement
   it.
2. Add a small `PreviewShellController` helper that scrolls the selected tile
   with `{ block: "nearest" }` only while the preview is in grid mode.
3. Call the helper from the grid layout path so both `enterGrid` and `setIndex`
   changes are covered, while guarding environments where `scrollIntoView` is
   absent.
4. Rebuild `packages/peitho-present/dist/preview.js`; `dist/shell.js` should not
   change.

## Gates

- `cd packages/peitho-present && npm ci && npm run build && npm test && npm run typecheck`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --check`
- Verify `packages/peitho-present/dist/shell.js` is unchanged and
  `packages/peitho-present/dist/preview.js` is rebuilt.
