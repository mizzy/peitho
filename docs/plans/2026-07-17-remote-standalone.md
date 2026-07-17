# Standalone (add-to-home-screen) mode for /remote + stable --host port (Issue #306)

Author decisions (2026-07-17, chat): all remaining /remote layout tuning — both
orientations — waits until the page can run without browser chrome, so this
issue is the enabler. Port stability strategy: **`--host` without `--port`
binds a fixed default port** so the remote URL is identical across runs (QR
scanned once, home-screen app keeps working); an explicit `--port` always
wins; if the default port is busy, present fails with a helpful error rather
than silently falling back to a random port (silent instability is the very
bug this fixes). Documentation-only and persisted-port alternatives were
rejected (convention-reliant / hidden state against the zero-config stance).

## CLI: effective port resolution

- `--port` becomes `Option<u16>` (no clap default). "Unspecified" is now
  unrepresentable as a magic value — the old `0` default doubled as both
  "random" and "unset", which would have made `--host --port 0` ambiguous and
  would have silently repointed existing `--port 0` callers (including our own
  integration tests, which would then collide on the fixed port in parallel
  runs).
- Resolution (pure function, unit-tested as a matrix):
  - `Some(p)` → `p` (including `Some(0)` = explicitly ask the OS for a random
    port — existing behavior, existing tests unchanged)
  - `None` + `--host` present → `6173` (`REMOTE_DEFAULT_PORT`; chosen number
    shown to and accepted by the author)
  - `None`, no `--host` → `0` (random; plain local present keeps today's
    behavior)
- Bind failure on the fixed port keeps the underlying `std::io::Error` as a
  typed diagnostic source (`PresentServerBindError`), and the "another
  `peitho present --host` is probably running; pass `--port` to choose
  another, or close the other instance" help is attached ONLY when the io
  kind is `AddrInUse` and the port came from the `--host` default (amended in
  review — a stale/unassignable `--host` IP fails with `AddrNotAvailable`,
  where that advice would be actively wrong; those errors pass through with
  their original diagnostics). No fallback in any case.
- `--port` help text documents the `--host` default.

## Remote template: standalone metadata

In `render_remote_index()` (`crates/peitho-core/src/render.rs`):

- `<meta name="apple-mobile-web-app-capable" content="yes">`
- `<meta name="mobile-web-app-capable" content="yes">`
- `<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">`
  — full-bleed dark; requires the top safe-area inset below.
- `<meta name="apple-mobile-web-app-title" content="Peitho Remote">`
- `<link rel="manifest" href="remote.webmanifest">`
- `<link rel="apple-touch-icon" href="remote-icon.png">`
- Safe-area top inset (new — in standalone the status bar overlays the page;
  in-browser the inset is 0 so nothing changes there):
  - base `#peitho-remote-root`: `padding-top: calc(14px + env(safe-area-inset-top, 0px))`
  - landscape block: `padding-top: calc(12px + env(safe-area-inset-top, 0px))`
  - companion (added in review): the landscape preview-column width formula
    subtracts the top inset too —
    `calc((100dvh - 151px - env(safe-area-inset-top, 0px) - env(safe-area-inset-bottom, 0px)) * <aspect>)`
    — because the available column height shrinks by the top inset once the
    status bar overlays the page.

## Server: two new present-only routes beside /remote

(`crates/peitho/src/server.rs`; /remote itself is served on loopback too —
`--host` only adds non-loopback exposure/QR — so the manifest and icon follow
the same rule. Amended in review: the routes are gated by an explicit
`serve_remote_assets` construction flag — present servers pass `true`,
preview servers `false` — because preview also binds `PresentServer` but
never serves /remote, and an installable manifest pointing at a 404 is a
confusing dead surface. Pinned by
`present_server_without_remote_assets_404s_remote_routes`.)

- `GET /remote.webmanifest` → `application/manifest+json`, embedded:
  `{"name":"Peitho Remote","short_name":"Remote","start_url":"/remote","display":"standalone","background_color":"#101216","theme_color":"#101216","icons":[{"src":"remote-icon.png","sizes":"180x180","type":"image/png"}]}`
- `GET /remote-icon.png` → `image/png`, embedded 180×180 icon
  (`include_bytes!` asset committed under `crates/peitho/assets/`).
  v1 art: flat `#101216` background with the accent-colored play-triangle
  motif; deliberately simple — iterate via Claude Design later if the author
  wants (same path as every other visual in this project). Generation is a
  one-off (documented here, not a build step).

Publish/dist are untouched: both routes are present-server-only, so the
existing publish contamination check needs no change.

## Docs

- README present section + `site/content/guide/` present page: a short
  "Add to Home Screen" paragraph — `--host` now yields a stable
  `http://<ip>:6173/remote`, scan the QR once, share-sheet → Add to Home
  Screen, subsequent `peitho present --host` runs reuse the URL.

## Implementation tasks (TDD, in order)

1. **CLI port resolution**: failing unit tests for the matrix above (plus
   `--host --port 0` = random, `--port N` without host = N), then switch the
   clap arg to `Option<u16>` and thread the pure resolver through `present`
   (and only present — build/preview/publish ports are out of scope). Audit:
   integration tests keep passing because they pass `--port 0` explicitly.
   Add the bind-failure error message test (bind the port first in the test,
   then assert the error text).
2. **Server routes**: failing tests for `/remote.webmanifest` and
   `/remote-icon.png` (status, content-type, body sanity: manifest parses as
   JSON with `display == "standalone"`; icon starts with the PNG magic),
   then implement.
3. **Template metadata**: extend
   `remote_index_mounts_remote_bundle_with_feature_detection_and_canvas_tokens`
   with string assertions for the five meta/link tags and both padding-top
   lines, then add them.
4. **Docs** (README + guide page), then all gates from CLAUDE.md
   (3× `cargo test --workspace`, clippy, fmt, bindings drift, npm build/test/
   typecheck, shell/preview/remote dist drift — no TS changes expected, so
   dist must be byte-identical).
5. **E2E (manual, author's phone)**: `peitho present --host` on a real deck →
   confirm the printed URL/QR shows port 6173; open in Safari → share →
   Add to Home Screen → launch: no browser chrome, full-bleed dark with sane
   safe-area padding, sync/nav/timer work; kill and rerun present → the
   home-screen app reconnects without edits. Desktop E2E can only verify the
   routes and meta tags (standalone launch is a phone-only behavior).

## Non-goals

- Any /remote layout tuning (deferred until standalone exists — the point of
  this issue).
- Android install prompts / service worker / offline support.
- Port changes for build/preview/publish servers.
