# PDF export: fix silent `image.decode()` hang in Linux headless Chrome (Issue #155)

## Background

#150 (black rectangles around box-shadow in Preview.app) was supposed to be fixed in #153 / v0.8.1, but the fix did not take on PDFs exported from Linux (decks.gosu.ke CI = ubuntu-latest). After bisecting, the root cause is confirmed: the `await image.decode()` in `pdf_flatten.js`'s `applyOuterShadow` **never resolves and never rejects under Linux headless Chrome + `--virtual-time-budget`, hanging forever** (Issue #155 has an instrumented Docker trace).

- No reject means `catch` never runs and no `console.error` is printed (silent)
- The whole flatten stalls there, and once the virtual time budget expires Chrome prints the PDF with the box-shadow still unhandled
- On macOS `decode()` just happened to finish before the virtual time ran out, so it worked by accident

## Scope of the root cause

The broken invariant is: **in-page scripts run in headless Chrome under `--virtual-time-budget` must not rely on `image.decode()`**.

At planning time, `packages/peitho-present/src/measure.ts`'s `waitForImage` (PPTX export measurement, run under `--virtual-time-budget=20000`) had the same class of `decode()` dependency, and this PR removed it in the same pass. PPTX export itself was later removed by PR #156 (Issue #152), so after rebase the only remaining target is `pdf_flatten.js`.

## Changes

### 1. `pdf_flatten.js` — drop the decode branch in `applyOuterShadow`

```js
// before
var image = document.createElement("img");
image.src = raster.dataUrl;
if (image.decode) {
  await image.decode();
} else {
  await loadImage(raster.dataUrl);
}

// after
var image = await loadImage(raster.dataUrl);
```

Use the img element returned by the existing `loadImage` (`new Image()` + wait on load/error events) as-is. This is even more unified than the Issue's verification patch (set `image.src`, wait on a separate Image via `loadImage`): wait on the load of the element that will actually be appended. It uses the same mechanism (wait on load event) as the approach the Issue author verified with `--print-to-pdf` in a Linux container, where `/Luminosity` went from 8 hits to 0.

### 2. CI — add a Linux E2E job (regression guard)

The reason this defect slipped through to release: the Chrome-executing E2E (including the `/Luminosity` assertion in `export_pdf.rs`) is entirely `#[ignore]`, and CI only runs `cargo test --workspace`, so it has never once run on Linux.

Add an `e2e` job to `.github/workflows/ci.yml`:

- ubuntu-latest (GitHub hosted runners have Google Chrome stable preinstalled)
- Set `PEITHO_CHROME_PATH: /usr/bin/google-chrome` explicitly as a job env var. The test helper is changed so that "when `PEITHO_CHROME_PATH` is set but the file does not exist, panic" (the project principle "explicit paths that don't exist are errors, no silent fallback"). This means missing Chrome always fails loudly in CI, and there's no such thing as a green run from silent E2E skips. On local machines with the env unset, behavior is as before (auto-detect → skip if not found).
- Put a `google-chrome --version` step as an early guard before compilation (fail faster and more clearly than a panic)
- Run the `#[ignore]` E2E (the 4 in `export_pdf.rs`) with `cargo test --workspace -- --ignored --test-threads=1`. `--test-threads=1` avoids a flake where parallel headless Chrome launches on the 4-vCPU runner hit the 60s one-shot timeout
- rust-cache uses `shared-key: tests` so it shares the cache with the existing `test` job and avoids double-compiling the workspace

The test helpers `test_chrome_path` / `find_in_path` live in `crates/peitho/tests/util/mod.rs` (a subdirectory module — `.rs` files directly under tests/ become standalone binaries), and the panic conversion is done in that one place.

Review found that the negated `/S /Luminosity` assertion only matches the whitespace-separated serialization, so it was strengthened to check absence of `/Luminosity` alone (so the tripwire doesn't slip past even if a future Chrome serializes it compactly as `/S/Luminosity`).

## Verification

- All gates (cargo test x3 / clippy / fmt / bindings drift / npm build+test+typecheck / dist drift)
- `cargo test -- --ignored` on macOS local (existing E2E, regression check)
- Ran `cargo test -- --ignored` in Docker (Linux container + Playwright chromium) and confirmed the box-shadow E2E fails before the fix (reproducing Issue #155) and passes after (isomorphic to the Issue's repro steps)
