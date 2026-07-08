# Issue 194: Promote present/preview web fonts to document scope

## Problem

`peitho present` / `peitho preview` fetch `peitho.css` and inject it as `<style>` inside each slide's Shadow DOM. That is correct for slide-CSS isolation, but browsers do not register `@font-face` inside a Shadow DOM as document fonts, so web fonts copied from fontsource via `fonts:` fall back to OS fonts only in present/preview.

The site viewer and PDF load CSS at document scope via `<link rel="stylesheet" href="peitho.css">` in `<head>`, so the same `peitho.css` gets web fonts to apply. The only difference is the scope of the font declarations.

## Scope

- The change is scoped to TypeScript under `packages/peitho-present/src/` only. Do not change the Rust side.
- `packages/peitho-present/dist/shell.js` and `packages/peitho-present/dist/preview.js` are embedded artifacts, so regenerate and commit them with `npm run build` after implementation.
- The Shadow DOM injection of `peitho.css` stays as is. Only font-related rules are promoted to document scope; the whole `peitho.css` is not.

## Plan

1. Red: add a new `packages/peitho-present/test/fontscope.test.ts` with failing unit tests for an `extractFontScopeCss`-equivalent extraction function.
   - `extracts leading imports after charset comments and whitespace`
   - `does not promote imports after ordinary rules`
   - `extracts top level font face blocks from anywhere`
   - `skips comments and strings while scanning font face blocks`
   - `omits non font rules`
2. Green: add a new `packages/peitho-present/src/fontscope.ts`.
   - Scan only the leading prefix; skip whitespace, comments, and `@charset` statements, then extract only the run of valid leading `@import` statements.
   - Do not extract `@import` that appears after an ordinary rule — such imports are invalid per the CSS spec anyway. Do not promote mid-file invalid `@import`s to document scope.
   - Read the whole file with a small lexical scanner; skip comments and string literals while extracting only top-level `@font-face{...}` blocks.
   - Do not follow `@font-face` nested inside `@media` etc. This iteration handles top level only.
   - If the extracted result is empty, do not add a style to the document.
3. Red: add present-shell document font scope tests to `packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts`.
   - `injects document scoped font css once for present shells`
   - `removes document scoped font css when the last present shell is destroyed`
   - Keep the existing `injects peitho css into each shadow root before fragment html` to also assert that Shadow DOM injection is unchanged.
4. Green: in `packages/peitho-present/src/shell.ts`, after fetching `peitho.css`, call an `installDocumentFontScope(document, css)` equivalent and run the returned cleanup in `destroy()`.
   - Keep exactly one `style[data-peitho-font-scope]` per document.
   - Multiple slides, multiple `mountPresentShell`s, and simultaneous current/preview shells in the presenter must still collapse to one.
   - So the first destroyed shell does not break the remaining ones under concurrent mounts, `fontscope.ts` holds a per-Document reference count and removes the injected style only on the last cleanup.
   - If a `style[data-peitho-font-scope]` already exists (installed externally), do not re-inject; and do not remove that external style on destroy.
5. Red: add preview-shell document font scope tests to `packages/peitho-present/test/preview.test.ts`.
   - `injects document scoped font css once for preview shells`
   - `removes document scoped font css when the last preview shell is destroyed`
   - Assert only one style with multiple slides and multiple `mountPreviewShell`s.
6. Green: apply the same `installDocumentFontScope(document, css)` in `packages/peitho-present/src/preview.ts` and clean up in the existing `destroy()`. Preview already has a post-mount cleanup path in `destroy()`, so hook in there.
7. As a finishing step, confirm that fetch failures in present/preview do not leave stray empty styles. If the `peitho.css` fetch fails, extraction has not run so nothing is injected. If a later step after CSS fetch fails, the returned shell's `destroy()` cleans up.

## URL resolution

`@import url("fonts/noto-sans-jp/index.css")` and `@font-face src:url("fonts/...")` inside `<style data-peitho-font-scope>` placed directly under document resolve against the document base URL. present's HTML, preview's index, and `peitho.css` all live in the same output root, so relative URL resolution matches what happens inside `peitho.css`.

## Non-goals

- Do not put the whole `peitho.css` into the document via `<link>` or `<style>`. The shell's own host element carries `.peitho-slide` etc., so slide styles would double-apply to the shell page.
- Do not remove the `@import` from Shadow DOM. Even if it loads, fontsource's `index.css` is `@font-face` only, so the document-scope injection is the one that matters and the Shadow DOM one is harmless.
- Do not run real-browser E2E in Codex's sandbox. Verifying actual web-font application will be done on the Opus side by opening `peitho present` / `peitho preview`.

## Gates

- `cd packages/peitho-present && npm ci && npm run build && npm test && npm run typecheck`
- `git diff --check`
- Confirm the Rust side and bindings are unchanged with `git diff --exit-code -- crates bindings`.
- Confirm that `packages/peitho-present/dist/shell.js` and `packages/peitho-present/dist/preview.js` are included in the commit as regenerated artifacts of `npm run build`.
