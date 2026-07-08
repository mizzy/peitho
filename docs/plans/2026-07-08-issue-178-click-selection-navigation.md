# Issue 178: ignore selection gestures in click navigation

## Problem

Mouse-based slide navigation currently treats every `click` as navigation.
Browsers still fire `click` after drag-selecting text or double-click-selecting a
word, so the slide changes and the user's selection is immediately cleared.

The behavior exists in three places: the present shell canvas click installer,
the distribution page inline script, and preview grid tile clicks.

## Plan

1. Add red Vitest coverage for selection and drag-ending clicks in the present
   click installer, plus the same guard on preview grid tile clicks.
2. Add a shared TypeScript helper in `packages/peitho-present/src/` that records
   pointer-down coordinates and decides whether a click should be ignored when
   selection is non-collapsed or movement exceeds the 5 px threshold.
3. Wire the helper into `controls.ts` and `preview.ts` without changing existing
   control-bar exclusion or touch swipe behavior.
4. Add a Rust template pin test for the distribution inline script, then mirror
   the same selection and movement guard there with comments marking the TS
   helper as the source to keep in sync.
5. Rebuild the presentation package so `dist/shell.js` and `dist/preview.js`
   carry the generated updates.

## Gates

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/`
- `cd packages/peitho-present && npm ci && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js`
- `git diff --exit-code packages/peitho-present/dist/preview.js`
