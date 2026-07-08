# Issue 181 follow-up: Escape toggles preview overview

## Problem

Preview keyboard handling currently emits `peitho:overviewrequest` with
`{ action: "exit" }` for Escape. That preserves grid-to-single behavior, but it
does not let users enter grid overview from single mode even though `o`, Enter,
and reveal.js-style Escape conventions are toggle-oriented.

## Plan

1. Update red Vitest coverage so keyboard Escape emits `toggle` instead of
   `exit`, while chord-modified Escape remains ignored.
2. Replace the old shell behavior test that expected single-mode Escape/exit to
   be a no-op with coverage for Escape toggling single-to-grid and grid-to-single
   through keyboard events.
3. Change only the keyboard mapping from `exit` to `toggle`; keep the `exit`
   action and shell handler for internal/direct request paths.
4. Update README preview controls to describe `o`, Enter, and Esc as overview
   toggles.
5. Rebuild `packages/peitho-present/dist/preview.js`; `dist/shell.js` should
   remain unchanged.

## Gates

- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --check`
- Verify `packages/peitho-present/dist/shell.js` is unchanged and
  `packages/peitho-present/dist/preview.js` is rebuilt.
