# Present: fullscreen placement over CDP (Issue #683)

## Problem

Chrome 155 (auto-updated 2026-09-29) ignores `--start-fullscreen`, and
ignores or clamps a `--window-position` that points off the primary display.
`peitho present` therefore opens both windows as normal windows on the
primary display. Measured table: Issue #683. `--window-position` plus
`--window-size` on the primary display is still honored.

## Decision

Every fullscreen placement is applied over CDP after launch; no launch flag
asks for fullscreen any more (one mechanism, not a flag path plus a CDP
fallback, on macOS and Linux alike).

- `cdp` moves from the binary into the `peitho` library so `browser` can use
  it (PDF export keeps using it from `main.rs`).
- A fullscreen launch is a `BrowserCommand` whose `fullscreen` field is
  `Some(CdpFullscreen { profile, position })`. The only constructor pushes
  `--remote-debugging-port=0` into the same argument list, so the port flag
  and the post-launch action cannot drift apart.
  - Two displays: slides `position = Some(external origin)`, presenter
    `position = Some(centered on primary)` (the planned `Fullscreen { x, y }`).
  - One window (single display or `--no-presenter`): `position = None`,
    fullscreen wherever Chrome opens it.
- `open_browser_plan` removes a stale `DevToolsActivePort` from the profile
  before spawning (a leftover from a previous session would point at a dead
  port), then applies the fullscreen actions on one background thread so the
  server starts without waiting: wait for `DevToolsActivePort`, connect to the
  page target, `Browser.getWindowForTarget`, `Browser.setWindowBounds
  {left, top, windowState: "normal"}` when a position is given, then
  `{windowState: "fullscreen"}`. Failure is a `warning:` naming the role and
  the reason — never silent.
- Measured on Chrome 155: a fullscreen request answered successfully is not
  evidence. Right after launch, and whenever another window's fullscreen
  transition is running, Chrome reports `fullscreen` and falls back to
  `normal` 100–200 ms later. Therefore the windows are fullscreened one at a
  time (slides, then presenter), and each is done only when
  `Browser.getWindowBounds` has reported `fullscreen` continuously for 1 s; a
  fallback to `normal` is re-requested at most once a second until a 20 s
  deadline.
- E2E caveat: while the macOS session is locked every fullscreen request
  falls back, even in a fresh Chrome — check `CGSSessionScreenIsLocked`
  before trusting a failed run.
- Windowed placements (`--presenter-windowed`, single-display debug) keep
  their flags.

## Accepted tradeoff (author approved 2026-09-29)

While a present session runs, each peitho Chrome profile listens on a random
loopback DevTools port; local processes can drive those windows. The
profiles are peitho-only (`~/.peitho/chrome-profile-*`).

## Verification

- Unit tests: argument planning (port flag present with a fullscreen action,
  absent otherwise; no `--start-fullscreen` anywhere).
- E2E on the real two-display Mac: `peitho present` puts slides fullscreen on
  the external display and the presenter fullscreen on the primary display
  (CGWindowList bounds), and `--no-presenter` fullscreens the one window.
