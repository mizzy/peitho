# Rehearsal mode: persist per-section actuals and compare against the plan (Issue #288)

## Goal

`peitho present --rehearsal deck.md` records the per-section actual times measured
by the presenter agenda into `.peitho/rehearsals/<timestamp>.json`. On every
subsequent `peitho present` (with or without the flag), the agenda shows the last
rehearsal's actual next to each section's planned time (`0:45 / 1:00 (last 0:52)`),
so the author can see how the previous run went — including for sections not yet
reached in the live run.

## Author decisions (2026-07-19)

1. **Recording is opt-in via `--rehearsal`.** The timer auto-starts on the first
   forward slide advance (`timeTracker.ts` dispatches `timercontrol start`), so
   "the timer ran" cannot distinguish a rehearsal from a display-check run-through.
   The only reliable signal of rehearsal intent is an explicit flag. Recording
   every timed session would let routine check runs overwrite the last real
   rehearsal (v1 compares against the most recent record).
2. **Persistence is incremental absolute snapshots.** The presenter periodically
   POSTs the full actuals state; the server rewrites the session's record file
   each time. A killed window or crash mid-rehearsal loses at most a few seconds,
   and absolute state matches the sync design (no deltas, replay-idempotent).
3. **Comparison display is unconditional.** If a last rehearsal record exists and
   matches the deck's sections, the agenda shows it in every present session —
   the real talk is where plan-vs-actual guidance matters most.
4. Schema is versioned from day one (notes.json precedent).

## Scope

Per-section actuals only (what the agenda already measures). Per-slide actuals
are out of scope for v1; the versioned schema leaves room for them. Decks without
section markers have nothing to record or display, so `--rehearsal` on a
sectionless deck is an explicit error (no silent no-op).

## Data flow

```
presenter window (rehearsal reporter, always active when sections exist)
  measures per-section actuals  (shared accumulator with the agenda display)
  ↓ dispatches peitho:rehearsalreport {elapsedMs, sections:[{name, plannedDurationMs, actualMs}]}
    (throttled: every 5s while running, plus immediately on slide change /
    pause / reset — including a reset adopted from another window via
    `peitho:timeradopt` — and on `peitho:closerequest` so the final section's
    tail is flushed before the windows close)
rehearsal bridge (installRehearsalBridge — DOM event → transport, §16 layering)
  ↓ POST /rehearsal  (absolute snapshot, fire-and-forget, console.error on failure)
present server
  --rehearsal: validate → write .peitho/rehearsals/rehearsal-YYYYMMDD-HHMMSS.json
               (atomic: temp file + rename; file created lazily on first snapshot)
               → responds {"recorded":true}
  otherwise:   discards (documented behavior) → responds {"recorded":false}
```

The client always reports; whether anything is persisted is decided in exactly
one place (the server's rehearsal sink, present only when `--rehearsal` was
passed). No `present.json` change, no client-side mode flag.

Reading side, at `peitho present` startup (before the server starts):

```
scan .peitho/rehearsals/ for names matching the rehearsal-*.json scheme
  → latest by the parsed (stamp, suffix) key (documented, deterministic;
    non-matching files are not peitho records and are ignored)
  → parse RehearsalRecord (unsupported version / unparsable → hard error with
    "delete or move <path>" help; these files are peitho-generated, so breakage
    means corruption, and silent skipping is forbidden)
  → emit present-cache rehearsal.json = RehearsalBaseline { version: 1, lastRun }
    (lastRun: null when no records exist — the file always exists, so the
    presenter never has to distinguish 404-normal from 404-broken)
presenter bootstrap (render_presenter_index): fetch rehearsal.json alongside
notes.json, pass into mountPresenterView → installAgenda
```

## Contract types (peitho-core, ts-rs exported to bindings/, committed)

New module `crates/peitho-core/src/rehearsal.rs` (shape follows `notes.rs`):

- `RehearsalSection { name: String, plannedDurationMs: u64, actualMs: u64 }`
- `RehearsalSnapshot { version: u8 (=1), elapsedMs: u64, sections: Vec<RehearsalSection> }`
  — the POST body. `deny_unknown_fields`. Serverside validation: version == 1,
  sections non-empty.
- `RehearsalRecord { version: u8 (=1), recordedAtMs: u64, elapsedMs: u64, sections: Vec<RehearsalSection> }`
  — the on-disk record. `recordedAtMs` is epoch ms stamped by the server when the
  session's file is first created (stable across rewrites of the same session).
- `RehearsalBaseline { version: u8 (=1), lastRun: Option<RehearsalRecord> }`
  — the present-cache `rehearsal.json`.

JSON emission via the existing `pretty_json` helper. Golden serialization tests
plus ts-rs export tests, mirroring `notes.rs`.

## Server (crates/peitho/src/server.rs)

- New `RehearsalSink { dir: PathBuf, expected: Vec<(String, u64)>, session: Mutex<Option<RehearsalSession>> }`
  held by `PresentServer` as `Option<Arc<RehearsalSink>>` (None for preview and
  non-rehearsal present — those callers genuinely have nothing to record).
  `RehearsalSession { path, recorded_at_ms }` lives inside the mutex so the
  stamped `recordedAtMs` stays stable across rewrites; the session is stored
  only after the first write succeeds.
- Route `POST /rehearsal` in `respond()` before the static fallback:
  - body parses as `RehearsalSnapshot` (`deny_unknown_fields`) → else 400
  - snapshot sections must match `expected` exactly (same order, name and
    plannedDurationMs) → else 422; guards against a stale window from an earlier
    build writing garbage into a new session's record
  - sink present: stamp/reuse `recordedAtMs` + session file name, write the
    `RehearsalRecord` atomically (write `.tmp`, rename), respond 200
    `{"recorded":true}`
  - sink absent: respond 200 `{"recorded":false}` (documented discard, not a
    silent one)
  - `GET /rehearsal` is not a route (falls through to static → 404); only POST.
- File name `rehearsal-YYYYMMDD-HHMMSS.json` in local time via `chrono` (already
  in the dependency tree through merman; add as a direct dep of `crates/peitho`
  with minimal features). If the name already exists (two sessions in the same
  second), suffix `-2`, `-3`, … — never overwrite a previous session's record.
  The session file is reserved atomically (`create_new`, so two concurrent
  rehearsal processes cannot claim the same name) and the first snapshot's
  bytes are written through the reserving handle, so outside the microsecond
  reserve-to-write window a `rehearsal-*.json` is never visible empty (a
  SIGKILL inside that window is the accepted residual, and the resulting error
  names the file to delete); later snapshots rewrite via temp + rename. The filename
  scheme has a single parser (`parse_rehearsal_filename`, returning the
  `(stamp, suffix)` ordering key); baseline selection considers only files
  matching the scheme — anything else in the directory is not a peitho record
  and is ignored.
- `/sync` is untouched. Rehearsal snapshots must not ride the sync channel: the
  channel coalesces (only the latest message survives), and rehearsal reports
  would race slide/timer messages out of the replay window.

## CLI (crates/peitho/src/main.rs)

- `Present` gains `#[arg(long)] rehearsal: bool`.
- Validation: `--rehearsal` + `--no-serve` is an error (recording needs the
  server). `--rehearsal` on a deck without sections is an error with help
  ("declare {\"section\":...} page comments to define the agenda").
- `present()`:
  - before emitting the cache: build the `RehearsalBaseline` from
    `.peitho/rehearsals/` (helper in main.rs or core; pure selection logic unit
    tested) and write it as `rehearsal.json` in `emit_present_cache` (always,
    like notes.json).
  - when `--rehearsal`: hand the sink (dir + expected sections from the built
    artifacts) to the server. The `.peitho/rehearsals/` directory is created
    lazily by the sink on the first snapshot — a rehearsal session where the
    timer never starts leaves no file *and no directory* behind.
- `PRESENTATION_ONLY_DIST_FILES` gains `"rehearsal.json"` — the publish
  contamination check must reject it in `dist/`, same rule as notes.json.

## Shell (packages/peitho-present)

- **Extract the actuals accumulator from `agenda.ts`** into a shared module
  (e.g. `sectionActuals.ts`): the slidechange flush-to-previous-section logic,
  timercontrol reset handling, timeradopt merge, and the 250ms tick accumulation
  move there behind `installSectionActuals({shell, sections, bus, window, log}) →
  { actualMs(): readonly number[], flush(): void, destroy(): void }` (`flush()`
  pushes the pending delta into the current slide's section; the reporter calls
  it before every snapshot so pause/close reports are exact regardless of
  listener order). The section-validation function has exactly one
  implementation (shared from `sections.ts`); the presenter gates measurement
  and reporting on it — invalid manifest sections mean no measurement, no
  display, and no reports — while `installAgenda`, being a public export, also
  runs the same shared validator for its own API robustness. The agenda becomes a pure
  display consumer. Exactly one accumulator instance exists per presenter view
  (created in `presenter.ts`, passed to both consumers) — one measurement source,
  two consumers, no duplicated semantics and no double-counting.
- New `rehearsalReporter.ts`: given the accumulator, shell, and sections,
  dispatches `peitho:rehearsalreport` with the full absolute snapshot on a 5s
  cadence while the timer runs, plus immediately on slide change, pause, reset
  (local or adopted from another window), and close request. Values are rounded
  to integers at the snapshot seam (serde `u64` rejects fractional JSON).
  Emits nothing before the timer has ever started. No-op when sections
  are empty.
- New `rehearsalBridge.ts`: `installRehearsalBridge(win, bus, fetcher)` listens
  for `peitho:rehearsalreport` and POSTs to `/rehearsal`. Fire-and-forget with
  `console.error` on failure. Installed by the presenter entry only (the
  audience and remote windows measure nothing).
- `agenda.ts` display: each row appends a dimmed `(last m:ss)` span
  (`data-peitho-agenda-last`) when a baseline `lastRun` exists **and** its
  sections match the manifest sections per index by name and plannedDurationMs
  (slide ranges are ignored — adding a slide inside a section must not invalidate
  the comparison). On mismatch the comparison is omitted and a `console.warn`
  explains why (deck edited since the rehearsal — expected lifecycle, not an
  error; but never silently absent). Extend the injected `log` pick with `warn`.
- `presenter.ts` options gain `rehearsal: RehearsalBaseline`; the presenter
  bootstrap in `render_presenter_index` fetches `rehearsal.json` alongside
  `notes.json`.
- CSS for the `(last …)` span in `render_presenter_index` (dimmed, does not
  disturb the fixed agenda row grid; agenda stays `overflow: hidden`).
- Rebuild and commit `dist/shell.js` (and any other bundle that changes).

## Edge cases

| Case | Behavior |
|---|---|
| `--rehearsal`, deck without sections | Error at startup with help |
| `--rehearsal --no-serve` | Error (recording needs the server) |
| Rehearsal session where the timer never starts | No snapshots → no file |
| Timer reset mid-rehearsal | Zeroed absolute snapshot overwrites the session file (reset means "this run starts over") |
| Non-rehearsal session | Reports discarded server-side, `{"recorded":false}` |
| Stale window POSTs mismatched sections | 422, nothing written |
| Corrupt / future-version **latest** file in `.peitho/rehearsals/` | Hard error at present startup with the path and delete/move help (only the latest scheme-matching file is parsed; corrupt older records are never read) |
| No rehearsal records | `rehearsal.json` has `lastRun: null`; agenda shows no `(last …)` |
| Deck sections edited since last rehearsal | Comparison omitted + `console.warn` |
| Display swap mid-rehearsal | Timer resets on swap (known tradeoff); the zeroed snapshot overwrites the session record — same information loss class as the timer itself, documented, not defended against in v1 |
| Publish | `rehearsal.json` in `dist/` fails the contamination check |
| Crash between temp write and rename | An orphaned `*.json.tmp` stays in `.peitho/rehearsals/`; it does not match the record filename scheme, so baseline selection ignores it. Not swept automatically (a sweep could race a concurrent session's in-flight rename) |

## Tests (TDD scope)

Rust:
- `rehearsal.rs`: golden JSON serialization (snapshot/record/baseline), ts-rs
  export tests, snapshot validation (version, empty sections, unknown fields)
- filename formatting + collision suffixing (pure, unit tested)
- baseline selection: empty dir / missing dir → `lastRun: null`; latest-by-name
  wins; corrupt or future-version file → error carrying the path
- server: POST /rehearsal in rehearsal mode writes the record (and rewrites on a
  second snapshot with the same `recordedAtMs`), non-rehearsal responds
  `recorded:false` and writes nothing, 400 on garbage, 422 on section mismatch,
  405 unaffected paths, `/sync` behavior unchanged
- main: `emit_present_cache` always writes `rehearsal.json`; contamination check
  rejects it in dist; present options validation errors
- render.rs: presenter template fetches `rehearsal.json` (existing test style)

TS (vitest):
- `sectionActuals`: existing agenda measurement tests migrate here; agenda
  display tests keep passing against the extracted accumulator (no behavior
  change in display semantics, including the Issue #258 revisit rule)
- `rehearsalReporter`: cadence, flush-on-slidechange/pause/reset, absolute
  payload shape, silent before first start, no-op without sections
- `rehearsalBridge`: POSTs on event, error logging, teardown (listener hygiene)
- `agenda`: `(last …)` rendering, mismatch omission + warn, null baseline
- `presenter`: integration — one accumulator, reporter + agenda both alive,
  teardown clean

E2E (real Chrome, mandatory before claiming done — server+client change):
1. `peitho present --rehearsal examples/lightning-talk/deck.md --port <p>`,
   advance through sections, confirm `.peitho/rehearsals/rehearsal-*.json`
   appears and grows; `cat` it.
2. Quit; re-run plain `peitho present` and confirm the agenda rows show
   `(last …)` (screencapture).
3. Re-run with `--rehearsal`, confirm a second file; delete it, confirm the
   first becomes the baseline again.
4. Confirm `peitho publish` contamination check still passes (no rehearsal.json
   in dist) and preview is unaffected.

## Out of scope

- Per-slide actuals (schema v2 candidate)
- Trend view across multiple records (v1 shows the most recent only)
- Pruning/retention of `.peitho/rehearsals/` (user-managed, timestamped files)
- Preserving actuals across display swap
- Docs-site guide page (separate PR, matching the examples/docs PR convention)
