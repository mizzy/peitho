# `peitho preview` — one-command edit loop (Issue #170)

Design finalization comment: https://github.com/mizzy/peitho/issues/170#issuecomment-4904080199

## Goal

A single `peitho preview [deck.md]` runs the edit loop: watch + rebuild + serve + automatic browser reload. The preview page toggles between "single-slide large view" and "all-slide tile view" with `o`, in the style of Deckset.

## Out of scope

- Edit loop with a presenter view (`present --watch`, plan C) — separate issue
- Speaker notes rendered in preview
- Partial updates via fragment swap (v1 is full reload + state restore)

## Architecture

```
peitho preview deck.md
  ├─ Watch registration (shared plumbing with build --watch. Registered before the first build to close the gap where saves during startup would be lost)
  ├─ build_artifacts → emit into .peitho/preview-cache/build-<generation>/
  │    fragments + manifest.json + peitho.css + assets/ + fonts/ + index.html (contains preview shell) + preview.js
  ├─ Serve via PresentServer (root is the generation directory. Swap after emit completes; keep the immediately previous generation for in-flight requests)
  ├─ Open one tab in the default browser (macOS: open / Linux: xdg-open. Failure is a warning only; serving continues. Suppressed with --no-open)
  └─ Watch thread: emit each successful rebuild into a new generation → swap root → increment generation
```

## Design decisions (finalized)

1. **Preview-dedicated shell page**: do not serve the bare dist index.html. Add a `preview.ts` entry in `packages/peitho-present`; esbuild bundles it into `dist/preview.js`. Same discipline as `dist/shell.js`: committed + embedded into the binary (`include_str!`) + CI drift check
2. **Reload detection uses the generation approach (absolute state)**: the initial `{"reload":true}` transient message proposal fails because the channel coalesces and windows not yet subscribed during load silently drop the event (a known failure pattern learned from display swap). The server holds `generation: u64` and puts `"generation":N` on **every** /sync GET response (handshake and poll). The client handshakes **before** fetching content to record baseline G, and on any subsequent response where generation != G, saves state and calls location.reload(). Comparing state (level) rather than events (edge) removes the timing hole structurally. Injecting reload via POST /sync is not allowed (400)
3. **§16 contract**: key input emits request events → the preview shell executes transitions. Add `peitho:overviewrequest` (toggle/enter/exit/activate). Reuse the existing `peitho:navigate` for slide movement
4. **Key bindings**: `o` = toggle single ⇄ tile (reveal.js / Slidev convention). Esc in tile = back to single. ←/→ = slide movement in single mode; selection movement in tile mode; Enter shows the selected slide in single. All shortcuts go through the existing `hasChordModifier` guard
5. **State restore**: save `{mode, index}` in `sessionStorage` and restore after reload (tab-scoped, survives reload — matches the requirement. Do not use URL hash — avoid URL side effects like the Chrome placement key problem seen with swap)
6. **Tile rendering**: draw all fragments with the same DOM structure as single view (same shadow DOM + CSS injection as the present shell), scaled down via CSS `transform: scale()`. aspect_ratio comes from manifest. No dedicated thumbnail generation (keep a single rendering path). Fragment fetches use Promise.all for parallelism
7. **Generation-directory serving**: do not remove_dir_all the directory currently being served. Each build emits into `build-<generation>/` and swaps the server root (Arc<RwLock<PathBuf>>). Retain one previous generation and prune older ones. '/' is index.html for preview (default document is a parameter of PresentServer::bind)
8. **Loop starts even for a broken deck**: on first-build failure, serve a minimal page with escaped error text + generation polling, and auto-recover on the next successful save (same discipline as build --watch continuing to watch even when the deck is broken from the start). On failures from the second onward, keep displaying the last successful build and report to stderr
9. **Watch plumbing shared with build --watch**: a single shared loop (WatchRuntime) that takes the rebuild action as an injected closure. Fatal watcher errors surface via process exit(1) (do not let threads die silently)
10. **Keep `build --watch`**: reposition it in the README as "a primitive for feeding output into external servers / pipelines"
11. **No dist/ contamination**: preview only uses its own cache. Do not touch publish's contamination check at all

## Verification

- All gates (cargo test x3 / clippy / fmt / bindings drift / npm build+test+typecheck / shell.js and preview.js drift)
- Real-browser E2E (done, all pass):
  1. Tab opens in single view → `o` toggles tile ⇄ single, selection frame, arrows, Enter, click transition
  2. Edit and save deck.md → auto reload + mode/position preserved
  3. Rapid saves → converge on the latest generation (no missed updates)
  4. Start with a broken deck → line-numbered error page → fix and save → auto recover
  5. '/' serves index, no console errors
