# Preview opens in grid mode by default

Date: 2026-08-01
Status: implemented

## Decision

`peitho preview` opens in grid (overview) mode on a fresh session instead of
single-slide mode. Preview's job is looking over the whole deck while
authoring, so the natural flow is overview first, then zoom into a slide with
Enter or a click. `peitho present` already owns the one-slide-at-a-time role.

## What changes

- The fresh-session default in the preview shell becomes `"grid"`. The default
  lives in one place (`DEFAULT_PREVIEW_MODE` in
  `packages/peitho-present/src/preview.ts`), used both for the pre-load field
  value and the no-restored-state branch at load.
- Nothing else. Session restore is untouched: a rebuild-triggered reload
  restores `{mode, index}` from `sessionStorage`, so an author who switched to
  single mode stays in single mode across rebuilds. The restored-index
  contract (exact index, including skipped slides) and the fresh-session
  first-non-skipped-slide selection are unchanged — the latter now selects the
  initial grid tile instead of the initially shown slide.

## Test updates

Tests that exercised grid behavior no longer need the entering `toggle`
request; tests that exercise single-mode behavior from a fresh mount first
leave grid via an `exit` overview request (public event contract, not a
storage seed, so the fresh-load path stays covered). The save/restore
round-trip test now saves single mode so it proves restore overrides the
default rather than coinciding with it.
