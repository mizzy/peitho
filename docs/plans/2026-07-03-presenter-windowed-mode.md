# presenter windowed mode (for debugging)

## Purpose

Add a debug mode to `peitho present` that keeps the presenter window from going fullscreen. When verifying behavior in combination with BetterDisplay's virtual display, it's hard to check things if the presenter is always fullscreen.

## Current problem (root cause)

`plan_presentation_layout` in `displays.rs` plans `WindowPlacement { fullscreen: false, width: 1200, height: 800, .. }` for the presenter, but `chrome_presenter_args` in `browser.rs` ignores this and always attaches `--start-fullscreen`. In other words, `WindowPlacement.fullscreen` and the size are dead fields — "plan" and "execution" have diverged.

Simply adding a flag branch to `chrome_presenter_args` would be a symptomatic patch. The correct approach is to **make WindowPlacement the single source of truth**:

- `plan_presentation_layout` fully encodes the intent (presenter is fullscreen by default, or 1200x800 centered in windowed mode) into `WindowPlacement`
- `browser.rs` follows `placement.fullscreen` and mechanically emits either `--start-fullscreen` or `--window-size={w},{h}`. It makes no judgment calls.

This way, future callers will also get the correct command as long as they construct a `WindowPlacement` (no need to remember the flag).

## Changes

1. `displays.rs`
   - Add an argument to `plan_presentation_layout(displays, presenter_windowed: bool)`
   - Default (false): presenter placement is `fullscreen: true` (reflecting current actual behavior in the type)
   - Windowed (true): `fullscreen: false`, 1200x800 (clamped to primary), centered
   - Thread the same argument through `layout_from_jxa_output` / `detect_presentation_layout` as well
2. `browser.rs`
   - `chrome_presenter_args`: if `placement.fullscreen` is true, emit `--start-fullscreen`; if false, emit `--window-size={width},{height}`
   - The slides side always plans `fullscreen: true`, so behavior is unchanged
3. `main.rs`
   - Add a `--presenter-windowed` flag to `present`, passed to `detect_presentation_layout` via `PresentOptions`

## Tests (TDD order)

- displays: in windowed mode, presenter placement becomes `fullscreen: false` + clamped size / default is `fullscreen: true`
- browser: when presenter placement is `fullscreen: false`, `--window-size=1200,800` is emitted and `--start-fullscreen` is not / default is unchanged (`--start-fullscreen`)
- main: `peitho present deck.md --presenter-windowed` parses correctly

## E2E

Since the presenter only opens in a two-display environment, this depends on the actual display configuration. Check the number of displays via JXA; if there are two, launch with `--port` fixed + `--presenter-windowed` → confirm windowed mode via screencapture → close all windows using `curl POST /sync`'s `{close:true}`. If there's only one display, go as far as unit tests and confirming the actual generated command output (state this explicitly in the report).

## Additional root cause discovered during E2E (fixed)

On the first E2E run, `--window-size` was ignored and the presenter opened nearly fullscreen. The cause was a Chrome process left over from the previous present session: on macOS, Chrome's process persists even after all windows are closed, and it keeps holding the peitho profile. Running `open -na` against that then becomes a handoff to the already-running instance, and every placement flag other than `--app` is nullified (the existing `--window-position`/`--start-fullscreen` were likewise ineffective from the second `present` run onward).

Fix: in `open_browser_with_request`, before launching, kill any leftover process with `pkill -f -- "--user-data-dir=<profile>"`, confirm its disappearance via pgrep, and only then spawn (`terminate_stale_profile_instances`). Note that pkill requires the `--` separator when the pattern starts with `--` (without it, exit 2 and nothing gets killed — confirmed empirically).

## Addendum: windowed mode drops explicit positioning in favor of Chrome's restore (author's instruction)

Initially, `--window-position`+`--window-size=1200,800` were specified explicitly, but per the author's request to keep reusing the position/size they manually moved things to while debugging, windowed mode was changed to pass no placement flags at all. Chrome saves the last window position in the profile's `Preferences` (`browser.window_placement`) and restores it when no flag is given (confirmed empirically).

Accordingly, `WindowPlacement` was restructured from a struct (x/y/width/height/fullscreen) into an enum `Fullscreen { x, y } | Restored`. Keeping it as a struct would make width/height dead fields again in windowed mode. The size-clamping calculation remains as a local calculation for computing the centered coordinates in fullscreen mode. On first launch (when there's no saved value in the profile), it opens at Chrome's default position (an accepted trade-off).

## Addendum 2: two root causes fixed to make restoration work

During E2E, "not restored to the position it was moved to" recurred, and investigation confirmed the following two points (details in CLAUDE.md's "gotchas" section).

1. **SIGTERM kill is treated as a crash**: the `pkill`-based leftover-process kill from the previous PR left an `exit_type: Crashed` state, and the crash recovery on the next launch would revive the old session's windows and bounds, overwriting the saved placement. As a fix, leftover processes are now terminated cleanly via `NSRunningApplication.terminate` (osascript JXA, triggered via parenthesis-less access), escalating to pkill only on timeout. The target pid is narrowed to Chrome's main process via `ps` (no `--type=`, pattern-inclusive).
2. **A dot in the app name breaks the placement pref**: the position of an `--app` window is saved under `browser.app_window_placement` keyed by an app name derived from the URL, but the dot in `127.0.0.1_/presenter.html` gets expanded as a nested path on write, causing a mismatch on read — so restoration had never actually worked. Changed the presenter's URL to `http://localhost:<port>/presenter` (added an extensionless route in server.rs), making the app name `localhost_/presenter` dot-free. Since the app name doesn't include the port, restoration is preserved even though present's port changes every time (confirmed on a real Chrome instance in the scratch environment: move → terminate cleanly → relaunch → confirmed restoration to the moved position).

Further E2E testing revealed that, with no placement flags, Chrome's first launch could sometimes place the window on the slides-side display, where it ends up completely hidden behind the fullscreen Space. Final form:

- `PresenterMode::Windowed { saved: Option<SavedWindowBounds> }`: peitho reads the saved bounds for `localhost_/presenter` from the presenter profile's Preferences
- If the center of the saved bounds is on a display other than the slides display → `WindowPlacement::Restored` (no flags, Chrome restores)
- If there's no saved value, or it's at an invisible position → `WindowPlacement::Windowed` (seeded at 1200x800 centered on the primary display via `--window-position`+`--window-size`; this position gets saved by Chrome and becomes the seed for restoration from then on)

Real-machine E2E: confirmed each of the following — initial seed display → move → close → relaunch restores accurately to the moved-to position (300,150); reseed fallback from an invisible saved bounds (external 1534,47); and Chrome's clamping of partially off-screen bounds.

## Addendum 3: cleanly terminate the Chrome instance when the presentation ends

Because macOS Chrome's process persists even after all windows are closed, two windowless Chrome instances kept lingering in the Dock after present ended (the UX problem of "it won't go away no matter how many times you close it"). When present ends (after the server shuts down, and only when `--no-open` was not specified), `quit_profile_instances` now terminates them cleanly. The stale-quit-on-launch logic remains in place as a safety net for abnormally terminated sessions. Real-machine E2E confirmed zero processes remaining after close.

## Addendum 4: made it work on a single display too (triggered by the author's bug report)

With a single-display setup (virtual display OFF), the presenter previously failed to open at all, and only the slides would open fullscreen (this turned out to be the true nature of "windowed mode but it goes fullscreen anyway"). To align with the intent of debug mode, when `--presenter-windowed` is set and there's only one display, slides now open in a 960x600 window (seeded top-left), and the presenter opens in its usual restored/seeded window. Since nothing goes fullscreen in this case, the visibility check for saved placement was relaxed to "no exclusion of the slides display." The normal-mode (no flag) behavior of "single display = slides only, fullscreen" remains unchanged.
