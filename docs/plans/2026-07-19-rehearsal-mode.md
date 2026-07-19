# Rehearsal mode: persist per-section actuals and review them against the plan (Issue #288)

## Goal

`peitho present --rehearsal deck.md` records the per-section actual times measured
by the presenter agenda into `.peitho/rehearsals/<timestamp>.json`. A new
`peitho rehearsal` subcommand prints the recorded actuals against the planned
times (per section, with deltas and a total), so the author can adjust section
time allocations between rehearsals.

## Author decisions (2026-07-19)

1. **Recording is opt-in via `--rehearsal`.** The timer auto-starts on the first
   forward slide advance (`timeTracker.ts` dispatches `timercontrol start`), so
   "the timer ran" cannot distinguish a rehearsal from a display-check run-through.
   The only reliable signal of rehearsal intent is an explicit flag. Recording
   every timed session would let routine check runs overwrite the last real
   rehearsal.
2. **Persistence is incremental absolute snapshots.** The presenter periodically
   POSTs the full actuals state; the server rewrites the session's record file
   each time. A killed window or crash mid-rehearsal loses at most a few seconds,
   and absolute state matches the sync design (no deltas, replay-idempotent).
3. **Review happens in the terminal, not in the presenter** (amended later on
   2026-07-19, superseding the earlier in-agenda `(last …)` comparison that an
   intermediate revision of this branch implemented). The rehearsal data exists
   to adjust section time allocations retrospectively; during a talk the live
   Actual/Planned + delta in the agenda is sufficient pacing guidance. The
   presenter therefore renders nothing rehearsal-specific, and `peitho rehearsal`
   is the single consumption surface.
4. Schema is versioned from day one (notes.json precedent).

## Scope

Per-section actuals only (what the agenda already measures). Per-slide actuals
are out of scope for v1; the versioned schema leaves room for them. Decks without
section markers have nothing to record, so `--rehearsal` on a sectionless deck is
an explicit error (no silent no-op).

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

Reading side — `peitho rehearsal` (no deck argument; records are read from the
CWD-relative `.peitho/rehearsals/`, the same convention as the present cache):

```
scan .peitho/rehearsals/ for names matching the rehearsal-*.json scheme
  → order by the parsed (stamp, suffix) key (documented, deterministic;
    non-matching files are not peitho records and are ignored)
  → default: parse the latest record only; --all: parse every record
    (unsupported version / unparsable → hard error with "delete or move <path>"
    help; these files are peitho-generated, so breakage means corruption, and
    silent skipping is forbidden)
  → print per-section planned / actual / delta plus a total row
```

`peitho present` no longer reads the records at all.

## Contract types (peitho-core, ts-rs exported to bindings/, committed)

Module `crates/peitho-core/src/rehearsal.rs` (shape follows `notes.rs`):

- `RehearsalSection { name: String, plannedDurationMs: u64, actualMs: u64 }`
- `RehearsalSnapshot { version: u8 (=1), elapsedMs: u64, sections: Vec<RehearsalSection> }`
  — the POST body. `deny_unknown_fields`. Serverside validation: version == 1,
  sections non-empty.
- `RehearsalRecord { version: u8 (=1), recordedAtMs: u64, elapsedMs: u64, sections: Vec<RehearsalSection> }`
  — the on-disk record. `recordedAtMs` is epoch ms stamped by the server when the
  session's file is first created (stable across rewrites of the same session).

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
    (with the validation reason in the body when `validate()` fails)
  - snapshot sections must match `expected` exactly (same order, name and
    plannedDurationMs) → else 422; guards against a stale window from an earlier
    build writing garbage into a new session's record. The check is owned by the
    sink (`RehearsalWriteError::SectionMismatch`), not duplicated at the route.
  - sink present: stamp/reuse `recordedAtMs` + session file name, write the
    `RehearsalRecord` atomically, respond 200 `{"recorded":true}`
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
  `(stamp, suffix)` ordering key); record selection considers only files
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
- `present()` with `--rehearsal`: hand the sink (dir + expected sections from
  the built artifacts) to the server and print
  `recording rehearsal to .peitho/rehearsals/` after the serving URL (the mode
  is otherwise invisible). The `.peitho/rehearsals/` directory is created lazily
  by the sink on the first snapshot — a rehearsal session where the timer never
  starts leaves no file *and no directory* behind.
- New subcommand `peitho rehearsal [--all]`:
  - default prints the latest record; `--all` prints every record oldest→newest
  - output: file stem header with `recordedAtMs` rendered as local
    `YYYY-MM-DD HH:MM`, then an aligned ASCII table
    `section / planned / actual / delta` plus a `total` row
  - formatting matches the agenda's rules: `m:ss` with rounded seconds; delta is
    `round((actual − planned) / 1000)` seconds with `+`/`-` sign
  - no records → `no rehearsal records in .peitho/rehearsals/` plus a help line
    pointing at `peitho present --rehearsal`, exit 0 (an empty query result is
    not an error)
  - corrupt/future-version record → the same hard error with delete/move help
  - output goes through a writer parameter (testable, matching existing
    subcommand patterns)

## Shell (packages/peitho-present)

- **Extract the actuals accumulator from `agenda.ts`** into a shared module
  (`sectionActuals.ts`): the slidechange flush-to-previous-section logic,
  timercontrol reset handling, timeradopt merge, and the 250ms tick accumulation
  move there behind `installSectionActuals({shell, sections, bus, window, log}) →
  { actualMs(): readonly number[], flush(): void, destroy(): void }` (`flush()`
  pushes the pending delta into the current slide's section; the reporter calls
  it before every snapshot so pause/close reports are exact regardless of
  listener order). The section-validation function has exactly one
  implementation (shared from `sections.ts`); the presenter gates measurement
  and reporting on it — invalid manifest sections mean no measurement, no
  display, and no reports — while `installAgenda`, being a public export, also
  runs the same shared validator for its own API robustness. The agenda stays a
  pure display consumer of the live actuals (nothing rehearsal-specific is
  rendered). Exactly one accumulator instance exists per presenter view
  (created in `presenter.ts`) — one measurement source, no duplicated semantics
  and no double-counting.
- New `rehearsalReporter.ts`: given the accumulator, shell, and sections,
  dispatches `peitho:rehearsalreport` with the full absolute snapshot on a 5s
  cadence while the timer runs, plus immediately on slide change, pause, reset
  (local or adopted from another window), and close request. Values are rounded
  to integers at the snapshot seam (serde `u64` rejects fractional JSON).
  Emits nothing before the timer has ever started. No-op when sections
  are empty.
- New `rehearsalBridge.ts`: `installRehearsalBridge(win, bus, fetcher)` listens
  for `peitho:rehearsalreport` and POSTs to `/rehearsal` with `keepalive: true`
  (the close-time flush must survive the window closing). Fire-and-forget with
  `console.error` on failure. Installed by the presenter entry only (the
  audience and remote windows measure nothing).
- Rebuild and commit `dist/shell.js` (and any other bundle that changes).

## Edge cases

| Case | Behavior |
|---|---|
| `--rehearsal`, deck without sections | Error at startup with help |
| `--rehearsal --no-serve` | Error (recording needs the server) |
| Rehearsal session where the timer never starts | No snapshots → no file, no directory |
| Timer reset mid-rehearsal | Zeroed absolute snapshot overwrites the session file (reset means "this run starts over") |
| Non-rehearsal session | Reports discarded server-side, `{"recorded":false}` |
| Stale window POSTs mismatched sections | 422, nothing written |
| Corrupt / future-version record read by `peitho rehearsal` | Hard error with the path and delete/move help (default mode parses only the latest scheme-matching file) |
| `peitho rehearsal` with no records | Friendly message + help, exit 0 |
| Display swap mid-rehearsal | Timer resets on swap (known tradeoff); the zeroed snapshot overwrites the session record — same information loss class as the timer itself, documented, not defended against in v1 |
| Crash between temp write and rename | An orphaned `*.json.tmp` stays in `.peitho/rehearsals/`; it does not match the record filename scheme, so record selection ignores it. Not swept automatically (a sweep could race a concurrent session's in-flight rename) |

## Tests (TDD scope)

Rust:
- `rehearsal.rs`: golden JSON serialization (snapshot/record), ts-rs export
  tests, snapshot validation (version, empty sections, unknown fields)
- filename formatting + `(stamp, suffix)` parse ordering + `create_new`
  collision suffixing (pure where possible, unit tested)
- record selection: empty/missing dir → none; latest-by-key wins (suffix `-2`
  beats the base name, `-10` beats `-2`); stray non-scheme files ignored;
  corrupt older record ignored when the latest is valid; corrupt latest →
  error carrying the path
- server: POST /rehearsal in rehearsal mode writes the record (and rewrites on a
  second snapshot with the same `recordedAtMs`), non-rehearsal responds
  `recorded:false` (unit and HTTP-level) and writes nothing, 400 on garbage
  (with reason for validate failures), 422 on section mismatch,
  405 unaffected paths, `/sync` behavior unchanged
- `peitho rehearsal`: golden table output, latest selection, `--all` ordering,
  no-records message, corrupt-latest error, delta sign/rounding edges, long
  section names widen the name column
- main: present options validation errors; `--rehearsal` startup line

TS (vitest):
- `sectionActuals`: existing agenda measurement tests migrate here; agenda
  display tests keep passing against the extracted accumulator (no behavior
  change in display semantics, including the Issue #258 revisit rule);
  `flush()` attribution
- `rehearsalReporter`: cadence, flush-before-snapshot, immediate reports
  (slidechange/pause/reset/adopted reset/closerequest), integer payloads,
  silent before first start, no-op without sections, invalid sections gate
- `rehearsalBridge`: POSTs on event with `keepalive`, error logging, teardown
- `presenter`: integration — one accumulator, previous-section attribution on
  slide change, teardown clean

E2E (real Chrome, mandatory before claiming done — server+client change):
1. `peitho present --rehearsal examples/lightning-talk/deck.md --port <p>`,
   advance through sections, confirm `.peitho/rehearsals/rehearsal-*.json`
   appears and grows; confirm the `recording rehearsal to …` terminal line.
2. Quit; run `peitho rehearsal` and confirm the table matches the recorded
   values; `peitho rehearsal --all` after a second run shows both.
3. Confirm plain `peitho present` renders the agenda with no rehearsal
   artifacts and that `peitho publish` contamination check still passes.

## Out of scope

- Per-slide actuals (schema v2 candidate)
- Trend aggregation beyond `--all`'s chronological listing
- Pruning/retention of `.peitho/rehearsals/` (user-managed, timestamped files)
- Preserving actuals across display swap
- In-presenter comparison display (superseded by author decision 3)
- Docs-site guide page (separate PR, matching the examples/docs PR convention)
