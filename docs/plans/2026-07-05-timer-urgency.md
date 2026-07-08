# Presenter view color signals (Issue #107 +)

## Intent

Make the meaning of "color signal = remaining time" consistent across the presenter view's timer surfaces. The original scope was just Issue #107 (timer color change), but during implementation review a pause/urgency color collision and the fact that the agenda section's current-time overrun was not reflected in color surfaced, so this PR cleans up both in the same pass.

### Timer remaining time

- Remaining 3min–1min: warning (amber = reuse `--pause`)
- Remaining 1min–0min: urgent (red = `--warn`)
- Elapsed ≥ planned time: overrun (red = `--warn`, regardless of state)
- `plannedDurationMs` unset: neutral color (current behavior)
- If planned time is short, skip thresholds and start from the strongest applicable state (e.g. a 2min deck starts in warning, a 40sec deck starts in urgent)

### Timer color while paused

Timer color while paused becomes `--fg-dim` (same as stopped). Previously it was amber (`--pause`), but that collided with warning urgency: "when amber lights up, is it paused or 3min remaining?" was ambiguous. Unify the color channel on "remaining time" and express pause as "progress has stopped" via dim. The distinction between pause and stopped is carried by the state pill's dot color (pause=amber, stopped=dim).

### Agenda section timer

- current and actual > planned → mark the row with `outcome="over"` and render time/delta in red (`--warn`)
- done keeps its existing under/over outcomes
- upcoming has no outcome
- CSS's over rule uses the compound selector `[data-peitho-agenda-state][data-peitho-agenda-outcome="over"]` so its specificity (0,3,0) explicitly beats the current-row blue rule (0,2,0)

## Design

### One derived function and one DOM attribute

Derive the timer's display state from one pure function. Toggle one `data-peitho-urgency` attribute.

```ts
export type TimerUrgency = "normal" | "warning" | "urgent" | "overrun";

export function urgencyFor(
  elapsedMs: number,
  plannedDurationMs: number | null
): TimerUrgency;
```

Thresholds:
- `plannedDurationMs == null` → `"normal"`
- `elapsedMs > plannedDurationMs` → `"overrun"`
- For `remaining = plannedDurationMs - elapsedMs`:
  - `remaining ≤ 60_000` → `"urgent"`
  - `remaining ≤ 180_000` → `"warning"`
  - otherwise → `"normal"`

Boundary is closed with `≤` (switches to urgent at exactly 1min remaining).

### Why one attribute (three lenses)

- **long-term**: adding future thresholds only requires changing one function, `urgencyFor()`. CSS is one attribute-value match away.
- **type-safety**: the `type TimerUrgency` union guarantees exhaustiveness. CSS covers the 4 values of `[data-peitho-urgency="..."]`.
- **root-cause**: "the derived value from elapsed and planned" — one derivation, one pure function. Upstream.

### Treatment of the existing `[data-peitho-overrun]` attribute

- `[data-peitho-presenter="timer"][data-peitho-overrun]` already exists and switches color to `var(--warn)`. It reads like it also gates visibility of the `.overrun` span (the `+MM:SS` display), but actually the `.overrun` span is a child of `.timer` with its own direct `color: var(--warn)`, so the attribute toggle is color-only.
- When `urgencyFor` returns `"overrun"`, `data-peitho-urgency="overrun"` achieves the same color, so **`data-peitho-overrun` toggling is removed** (unified into one attribute). The existing `[data-peitho-presenter="timer"][data-peitho-overrun]` selector is also removed, replaced by `.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer`.
- `.peitho-time-tracker[data-peitho-overrun]` is on the tracker side and switches its own color, so **leave it as-is** (separate component).

### CSS changes

Add/change in the `<style>` block inside `render_presenter_index()` in `crates/peitho-core/src/render.rs`:

```css
.clock[data-peitho-urgency="warning"] .timer,
.clock[data-peitho-urgency="warning"] .timer .planned { color: var(--pause); }
.clock[data-peitho-urgency="urgent"] .timer,
.clock[data-peitho-urgency="urgent"] .timer .planned { color: var(--warn); }
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer,
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer .planned { color: var(--warn); }
```

Reference the existing `--pause` (amber) and `--warn` (red) directly. The values line up: "urgency-warning = pause", "urgency-urgent = overrun = warn", so no separate variable is created and the references are unified (the overrun rule already used `--warn` directly, so introducing a separate variable would create in-PR inconsistency).

Keep the existing `.clock[data-peitho-state="paused"] .timer` and `.clock[data-peitho-state="stopped"] .timer`. It is natural that timer state color (paused=amber, stopped=dim) wins over urgency (making it red while stopped conveys nothing).

**Priority implementation:** CSS is last-wins, so place state selectors after urgency selectors under the `:root` block. Alternatively, apply urgency only while running, e.g. `.clock[data-peitho-state="running"][data-peitho-urgency="warning"]` — more explicit and safer → adopt this form.

```css
.clock[data-peitho-state="running"][data-peitho-urgency="warning"] .timer,
.clock[data-peitho-state="running"][data-peitho-urgency="warning"] .timer .planned { color: var(--pause); }
.clock[data-peitho-state="running"][data-peitho-urgency="urgent"] .timer,
.clock[data-peitho-state="running"][data-peitho-urgency="urgent"] .timer .planned { color: var(--warn); }
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer,
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer .planned { color: var(--warn); }
```

Note: only overrun should stay red while stopped/paused (once over, stop → still signal over). → For overrun only, add a compound selector that doesn't restrict the state value:

```css
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer,
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer .planned { color: var(--warn); }
```

Including `[data-peitho-state]` gives higher specificity than state rules, so overrun stays red even while stopped/paused, and state color (stopped=dim, paused=amber) wins otherwise.

### TypeScript-side integration

`packages/peitho-present/src/presenter.ts`:

- Export `urgencyFor()` from a new module `timerUrgency.ts` and import from presenter
- Set `data-peitho-urgency` on `clockRoot` in `tick()`:
  ```ts
  clockRoot.dataset.peithoUrgency = urgencyFor(elapsedMs, plannedDurationMs);
  ```
- Remove the existing `timerRoot.toggleAttribute("data-peitho-overrun", ...)`

### Tests (TDD)

`packages/peitho-present/test/timerUrgency.test.ts` (new):
- `plannedDurationMs == null` → `"normal"`
- remaining > 3min → `"normal"`
- remaining = exactly 3min → `"warning"`
- remaining = 1min 1sec → `"warning"`
- remaining = exactly 1min → `"urgent"`
- remaining = 1sec → `"urgent"`
- remaining = 0 → `"urgent"` (elapsed == planned is not yet overrun, aligned with the existing `isOverrun` `>` condition)
- elapsed > planned → `"overrun"`
- planned = 2min → at elapsed=0, `"warning"` (the 3min threshold is out of range, so start at warning)
- planned = 30sec → at elapsed=0, `"urgent"` (both thresholds skipped, start at urgent)

Additional tests in `packages/peitho-present/test/presenter.test.ts`:
- ticking the presenter updates `clockRoot.dataset.peithoUrgency` to the expected value
- planned=10min, elapsed=8min 1sec → urgency="warning"
- planned=10min, elapsed=9min 1sec → urgency="urgent"
- planned=10min, elapsed=10min 1sec → urgency="overrun"
- planned=null → urgency="normal"

### Impact surface

- `packages/peitho-present/src/timerUrgency.ts` (new)
- `packages/peitho-present/src/presenter.ts` (tick update)
- `packages/peitho-present/test/timerUrgency.test.ts` (new)
- `packages/peitho-present/test/presenter.test.ts` (additional tests)
- `crates/peitho-core/src/render.rs` (added CSS, added color variables)
- `crates/peitho/tests/present.rs` (add assertion that the new CSS selector appears in presenter HTML, if needed)
- `packages/peitho-present/dist/shell.js` (regenerated by `npm run build`, committed)

### Verification

- Follow the E2E procedure in `docs/plans/2026-07-04-presenter-redesign.md`
- With a 10min deck (`plannedDurationMs = 300_000`), visually confirm: start → 3min elapsed (2min remaining) yellow, 4min elapsed (1min remaining) orange, 6min elapsed red
