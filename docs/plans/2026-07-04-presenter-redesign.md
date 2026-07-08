# Presenter view redesign (Claude Design mock reflection)

2026-07-04. Reflect the mock built in Claude Design (project `73ba6a5b`/`Presenter.dc.html`, https://claude.ai/design/p/73ba6a5b-288b-46f7-a2c3-cb06863e3c5b) into the presenter screen. After the reflection, the CSS in `render_presenter_index()` is the source of truth.

## Design overview

- Dark base (oklch) + cyan accent, Geist/Geist Mono fonts (Google Fonts, fall back to system-ui when offline)
- Left column: status row (Now / Slide N of M / deck title) → 16:9 fixed current slide (container query) → keyboard hint row → Notes card
- Right column: Next slide card (16:9) → timer card (large tabular numerals + state pill + redesigned tracker + controls)
- Timer state (stopped/running/paused) drives the state pill, timer color, and the Play button's label/color
- Merge Start/Pause/Resume into a single Play button (action derived from state; the `peitho:timercontrol` event contract is unchanged)
- Button press feedback: sink + color invert + ripple from the click position (disabled under `prefers-reduced-motion`)

## Out of scope (conservative call, called out in the report)

- The Agenda section in the mock (per-section actual/planned time): excluded because peitho has no section concept and no section-interval time data. Revisit in a separate Issue if needed
- The "Section — ..." indicator in the status row: same as above
- Keybinding changes: the existing Space=next mapping is unchanged. Align UI labels with the actual behavior (do not attach a Space hint to the Play button)

## Files changed

1. `packages/peitho-present/src/presenter.ts`
   - Rewrite the DOM structure to the new design (keep and extend the data-peitho-presenter / data-peitho-action hooks)
   - Timer state derivation: `startedAt()===null` → stopped, `isPaused()` → paused, otherwise running
   - `data-peitho-action="playpause"` button: dispatch start/pause/resume based on state
   - Update the state pill / play label / clock card `data-peitho-state` in tick()
   - Split the timer display into spans (planned / overrun coloring). textContent remains format-compatible with the existing output
   - pointerdown handler for the ripple (`--rx`/`--ry` + `.pressed`)
2. `packages/peitho-present/src/timeTracker.ts`
   - Only when `variant: "presenter"`, emit the legend (Slide progress / Time) + `.tracker` wrapper + `.fill` (time-progress width) + scale (planned-time 5-minute ticks)
   - The DOM for the present variant is unchanged. Marker-move logic (left% + translateX clamp) stays shared
3. `crates/peitho-core/src/render.rs` `render_presenter_index()`
   - Replace CSS wholesale + add a Google Fonts link. Do not include anything Agenda-related
   - `.clock` is flex column + `.controls { margin-top: auto }` (prevents the grid-stretch button bloat recurrence)
4. Test updates
   - `packages/peitho-present/test/presenter.test.ts`: state transitions of the unified playpause button, state pill, new hooks
   - `packages/peitho-present/test/timeTracker.test.ts`: added fill/scale for the presenter variant (the existing present tests are unchanged)
   - Update the two presenter tests in `crates/peitho-core/src/render.rs` to assert on the new CSS
5. Rebuild + commit `packages/peitho-present/dist/shell.js`

## Gates

All CLAUDE.md gates (cargo test x3 / clippy / fmt / bindings drift / npm build+test+typecheck / shell.js drift) + real-browser E2E (build → present → screenshot the examples).
