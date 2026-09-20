# Preview BFCache restore reload

Date: 2026-09-21

<!-- derived-from ./2026-09-20-preview-inline-edit.md#task-10-settle-transitions-defer-reloads-save-on-page-exit-and-clean-up -->

## Problem

The preview entry page disposes its keyboard, shell, and reload handles on
`pagehide`. Chrome can then restore that disposed document from the
back/forward cache without re-running the bootstrap, leaving the restored DOM
visible but inert. A restored preview is stale even if those handles had not
been disposed, because its generation, notes, and edit annotations may no
longer match the server.

## Scope

Audit the preview, present, presenter, and remote entry-page bootstraps in
`crates/peitho-core/src/render.rs` and `crates/peitho/src/main.rs`. Change only
entry pages that dispose their bootstrap handles on `pagehide`; keep the
existing preview disposal so page-exit keepalive saves still run.

## Test-first implementation

1. Extend the existing preview bootstrap Rust tests to require a `pageshow`
   listener whose persisted branch reloads the document and whose ordinary
   branch does nothing.
2. Run the targeted test and record its failure before changing the template.
3. Add the listener to the preview bootstrap. It must remain installed after
   `pagehide`, because that is how the BFCache-restored document detects that
   it needs a fresh load.
4. Add a TypeScript unit test only if the audit finds a TypeScript-side
   installer for this bootstrap behavior.

## Verification

Run the requested workspace Rust gates, package build/tests/typecheck, and
`git status --short`, recording every exit code. The package build must leave
committed bundles consistent with their TypeScript sources.

## Summary

<!-- derived-from #problem -->
<!-- derived-from #scope -->
<!-- derived-from #test-first-implementation -->

BFCache restoration becomes an explicit fresh-load boundary for the preview,
while the existing `pagehide` disposal and keepalive save paths remain intact.

## Measured (real Chrome, 2026-09-21)

Deterministic A/B, driven from the page so tab focus cannot interfere: load the
preview, `location.href = '/manifest.json'`, `history.back()`, then dispatch a
`PageDown` keydown on `window` and read the `N / total` position line.

| Build | navigation entry after Back | PageDown |
| --- | --- | --- |
| before the fix | `navigate` (the original entry: a BFCache restore) | `1 / 2` → `1 / 2` (dead shell; thumbnail clicks dead too) |
| with the fix | `reload` | `1 / 2` → `2 / 2` |

## Task 12 acceptance record (Issue #546)

Run against `target/preview-inline-edit-e2e/` (ignored fixture: a top deck that
only includes `included.md`, `css/base.css` copied from `themes/base.css`, and a
keyed `css/overrides.css`), `peitho preview … --port 6175 --no-open`, real
Google Chrome.

- [x] Paragraph click shows exactly `Peitho is a *fast* tool.`; replacing it
      with `Peitho is a **very fast** tool.` + Enter wrote exactly those bytes
      and reloaded on the same slide.
- [x] `編集します` → `編集できます`: an Enter keydown with `isComposing: true`
      and one with `keyCode 229` did not commit (0 POSTs, editor still
      `plaintext-only`); the following plain Enter committed. **Caveat:** the
      composition keys were synthetic events — browser automation cannot drive
      the macOS IME. A pass with a real Japanese IME is still the author's to
      do before relying on it.
- [x] `Derived Heading` → `Renamed Heading`: manifest keys became
      `renamed-heading`, `css-guard`; the preview stayed on the slide.
- [x] `CSS Guard` → `Broken CSS Guard`: the rebuild failed with
      `overrides.css: unknown slide key 'css-guard' in override selector` and
      the full banner appeared over the last good slide.
- [x] Pointing the selector at `broken-css-guard` cleared the banner on the
      next successful generation (`buildError: null`).
- [x] Every accepted edit changed `included.md`; the top `deck.md` checksum
      never changed.
- [x] Cmd-click on the rendered link opened `https://peitho.gosu.ke/` in a new
      tab and started no edit.

The BFCache bug above was found by this run (Back after visiting another page).
