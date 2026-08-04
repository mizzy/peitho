# Lint publish/print ordering (2026-08-04)

## Problem

`lint_accepts_trivially_small_deck` fails intermittently on Linux CI with:

```
Chrome completed before one-shot output was ready
help: expected lint measurement payload before Chrome exited
```

This is **not** the bug PR #393 fixed. That one truncated output that Chrome had
already written; this one is Chrome exiting before the payload is written at all.

### Evidence

- In the failing logs (runs 30914999594 / 30912935249 / 30911489043, all the same
  signature), `PEITHO_LINT_DONE` occurs 0 times **and so does `CONSOLE`**. The
  drain-window bug leaves a truncated tail with CONSOLE lines present; zero
  CONSOLE lines means `publish()` was never reached.
- Chrome's own timestamps bound its whole lifetime at ~370 ms
  (`134128.226` → `134128.595`), while `lint_measure.js` budgets
  `WINDOW_LOAD_TIMEOUT_MS = 2000` plus `FONT_READY_TIMEOUT_MS = 2000`. Neither
  timeout ever expired — the process ended first.
- The last stderr line is `4322 bytes written to file …/lint.pdf`: the print
  completed and Chrome tore down while the measurement chain was still pending.

### Root cause

`lint_measure.js` publishes at the end of an asynchronous chain:

```
waitForWindowLoad → waitForImages → waitForFonts → waitForFrame → publish
```

Nothing orders that chain against Chrome's `--print-to-pdf` completion. Chrome
prints when *it* considers the page ready and exits immediately after, so the
chain is a race against teardown. `--virtual-time-budget=10000` bounds virtual
time; it does not hold the print back until pending promises settle.

The three prior fixes to this seam (`7554083`, `ca87934`, `d088481`) all added
*upper bounds to waits*. A bound shortens the race; it never orders it. That is
why the flake keeps returning under new load conditions.

### Why it never reproduces locally

Measured on macOS Chrome: under `--virtual-time-budget`, both `setTimeout` and
`document.fonts.ready` resolve against virtual time, so the chain finishes
early and `PEITHO_LINT_DONE` reliably precedes the PDF write. The ordering only
inverts where real-time resource work outlives virtual time (Linux headless CI).
Local runs cannot validate this fix; the Linux `e2e` job is the check.

## Approach

Publish from a **`beforeprint` listener** instead of from the tail of the async
chain.

`beforeprint` fires synchronously, after layout and font resolution have settled,
and — measured — strictly *before* the PDF bytes are written. Publishing inside it
is ordered by construction rather than by timing, which makes "Chrome printed but
lint published nothing" unrepresentable rather than merely unlikely.

Measured orderings (macOS Chrome, same flags as `lint_chrome_args`):

| Scenario | Result |
| --- | --- |
| publish inside `beforeprint` | `BEFOREPRINT` → `DONE` → `bytes written` |
| async chain that **never settles**, plus `beforeprint` | `PUBLISH_FROM_BEFOREPRINT` → `DONE` → `bytes written` |

The second row is the load-bearing one: even with a permanently pending promise
— the exact Linux hang class called out in the existing comments — the payload
still lands before the print. It also subsumes the `image.decode()` hang pitfall
already recorded in CLAUDE.md.

### Design

Keep the existing readiness chain as the *preferred* trigger, since it publishes
as soon as fonts and images genuinely settle (the common, correctly-measured
case). Add `beforeprint` as a second trigger. Guard both with a single
`published` latch so exactly one payload is ever emitted.

This is one seam, not a guard at each consumer: `publish()` gains the latch, and
the two triggers both route through it.

Not chosen:

- **Measure synchronously at parse time.** Ordering would be guaranteed, but it
  measures before fonts and images load, silently changing lint results. Wrong
  answers on time are worse than a flake.
- **Raise the timeouts.** Same shape as the three fixes that already failed.

## Tasks

1. `crates/peitho-core/src/lint_measure.js`
   - Add a module-level `published` latch; make `publish()` a no-op after first call.
   - Register a `beforeprint` listener that measures and publishes.
   - Keep the existing chain calling the same `publish()`.
   - Comment states the invariant (publish must precede print) and why the chain
     alone cannot guarantee it — not what the next line does.
2. `crates/peitho-core/src/render.rs` tests
   - Assert `LINT_MEASURE_JS` registers `beforeprint`, alongside the existing
     assertions that it contains `console.log(` / `CHUNK_SIZE` and does not
     contain the literal signal strings.
3. Verification
   - Full gate list from CLAUDE.md.
   - Linux `e2e` job is the real check; confirm `lint_accepts_trivially_small_deck`
     passes there, and that the lint report still reports real measurements
     (a payload of zeroes would pass the test while defeating lint).

## Risk

`beforeprint` may fire on a page whose fonts have not settled, producing
measurements from fallback fonts. That only happens when the chain has not
already published — i.e. exactly the case that currently produces *no*
measurement and a hard failure. Degrading from "hard error" to "measured against
whatever font resolved" matches the tradeoff `waitForFonts` already documents.
