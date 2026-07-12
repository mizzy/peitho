# Issue #258: keep measured actual time visible on revisited agenda sections

Date: 2026-07-12
Issue: #258
Amends: `docs/specs/2026-07-04-presenter-agenda-design.md` (display semantics of
upcoming rows that carry a measured actual)

## Problem

The presenter agenda classifies rows purely by the current slide's section
index (`agendaState`: `done` / `current` / `upcoming`). When the presenter
navigates back to a slide in an earlier section, every later section is
re-labeled `upcoming`, and `actualText` in
`packages/peitho-present/src/agenda.ts` unconditionally rendered the em dash
for `upcoming` rows. A section that had already accumulated actual time
therefore snapped back to `— / <planned>` as if it had never been visited.

The stored value was never lost — `actualMs[index]` keeps accumulating
correctly — only the render suppressed it.

## Fix

Display-layer change at the single render seam, per the direction suggested
in the issue:

```ts
function actualText(state: AgendaState, actual: number): string {
  return actual > 0 || state !== "upcoming" ? formatMinuteSeconds(actual) : EM_DASH;
}
```

A measured actual (`> 0`) renders regardless of state; the em dash remains
only for upcoming sections with no accumulated time (never visited, or
zeroed by a timer reset).

Nothing else changes:

- `agendaState` classification stays as-is.
- The outcome styling gate stays state-driven (`outcome` remains `null` for
  `upcoming`) — the issue explicitly keeps styling semantics on the state
  class.
- `deltaText` still shows the delta only for `done` — the issue explicitly
  defers whether a revisited-then-left section should show a delta.
- Accumulation (`flushElapsedToSectionOf` / `tick` / reset handling) is
  untouched.

## Tests

In `packages/peitho-present/test/agenda.test.ts`:

- Updated "accumulates elapsed deltas into the current section and resumes
  when returning": the revisited-but-not-current section now expects its
  measured `0:02 / 0:01` instead of the previously locked-in `— / 0:01`,
  and after a timer reset the same row is asserted to return to `— / 0:01`.
- Added "renders never-visited upcoming sections with a dash actual" to pin
  that the em dash still appears for sections with no measured time.

## Known edge (accepted)

A section transited for under ~500ms accumulates a positive actual that
rounds to `0:00`, so it renders `0:00 / <planned>` rather than the dash.
That is the intended "visited" semantics of the `> 0` predicate from the
issue.
