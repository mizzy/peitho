# Remote urgency haptic transitions (Issue #327)

## Scope

Add deck-level urgency transition events for timer threshold changes and let the remote vibrate on those transitions. This intentionally does not add section-level urgency, presenter-side event tracking, remote visual UI, or CSS changes.

## Design

- Keep `urgencyFor(elapsedMs, plannedDurationMs)` unchanged as the pure derivation.
- Add `installUrgencyTracker()` as a small state machine that polls the shell every 250ms, derives deck urgency, and emits `peitho:urgencychange` on forward band changes.
- Use the fixed order `normal -> warning -> urgent -> overrun`; when elapsed jumps across multiple bands, emit each intermediate transition in order rather than silently dropping middle bands.
- Treat rewinds and resets as silent state updates back toward `normal`.
- In `/remote`, install the tracker against the remote's absolute timer state and subscribe a haptic bridge that maps `warning`, `urgent`, and `overrun` to vibration patterns. The bridge buffers events dispatched within a single JS turn and flushes once via `queueMicrotask`, merging patterns with a 200 ms pause: `navigator.vibrate()` cancels any prior pending vibration, so a multi-band jump like `normal→overrun` would otherwise only feel the terminal band. `HAPTIC_PATTERNS` is a total `Record<TimerUrgency, readonly number[] | null>` (with `normal: null`), so adding a new urgency variant is a compile error at both the rank map and the pattern table.

## Tests

- Extend `timerUrgency.test.ts` with fake-timer tests for null plans, boundaries, multi-band jumps, rewinds, repeated ticks, reset snapping, and `destroy()`.
- Extend `remote.test.ts` with direct bridge tests for vibration patterns, missing `navigator.vibrate`, and listener cleanup.
- Run `npm test`, `npm run typecheck`, and `npm run build` in `packages/peitho-present`; commit regenerated `dist/` bundles.
