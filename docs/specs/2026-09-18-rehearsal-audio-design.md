# Rehearsal audio and per-slide timeline — design

Date: 2026-09-18
Extends: `docs/plans/2026-07-19-rehearsal-mode.md` (Issue #288)

## Goal

Listening back to a whole rehearsal recording is painful enough that nobody
does it. Make a rehearsal reviewable per slide:

1. `peitho present --rehearsal` records a **per-slide timeline** (which slide
   was entered at which timer position). Useful on its own: it shows which
   slide eats the time, which is the existing purpose of rehearsal data
   (adjusting time allocations).
2. `peitho present --rehearsal --audio` additionally records the speaker's
   microphone with a recorded timer offset, so a timeline entry at or after
   the audio start maps directly to `atMs - audio.startMs`. No manual sync
   between a recorder and the timer — this is the one thing only peitho can
   provide.

Out of scope (author-accepted, 2026-09-18): transcription, filler/pace
analysis, any playback UI. The record + `.webm` are enough input for external
tools (whisper, an LLM). Review stays terminal-only.

## Chosen approach

Presenter-side `MediaRecorder` slaved to the timer; chunks POSTed to a
dedicated endpoint and appended to a file next to the session record.

Rejected: spawning `ffmpeg`/`sox` from the server. It adds an external
dependency, and a server-side recorder cannot follow pause/resume precisely,
which breaks the stable timer-to-audio offset — the property the whole feature
rests on.

Rejected: "record with QuickTime, peitho only emits timestamps". Works, but the
author confirmed that starting a recorder in sync with the timer is the actual
pain.

## Key design decisions

### Timeline (always on under `--rehearsal`)

- `RehearsalSnapshot` / `RehearsalRecord` go to **version 2** and gain
  `timeline: Vec<RehearsalSlideEntry>` where an entry is
  `{ key: SlideKey, index: u32, atMs: u64 }` — "slide entered at timer
  position `atMs`". `index` is the 0-based manifest index at record time (keys
  are identity, index is for display).
- The first entry is the slide showing when the timer starts (`atMs: 0`).
  Re-entering a slide appends another entry; per-slide totals are derived at
  print time (entry N lasts until entry N+1's `atMs`, the last until
  `elapsedMs`). Reveal steps are not timeline entries.
- The accumulator lives next to `sectionActuals.ts` and follows the same
  events. Snapshots stay **absolute**: every snapshot carries the full
  timeline (a 60-minute talk is a few hundred small entries).
- Reset clears the timeline, matching the existing rule "reset means this run
  starts over; the zeroed snapshot overwrites the session file".
- The server validates: `atMs` non-decreasing, `atMs <= elapsedMs`, `index`
  within the deck, `key` matches the deck's key at that index. Violations are
  422, like the section mismatch today.
- `peitho rehearsal` keeps reading **version 1** records (they are user data
  and never pruned): v1 prints exactly as today. For v2 it prints the section
  table, then a per-slide table (`#`, first-entered position, total time), then
  the audio path when the record has one. Unknown versions stay a hard error.

### Audio (`--audio`, requires `--rehearsal`)

- `--audio` without `--rehearsal` is a clap-level error (`requires`).
- **The presenter must know audio is requested**, because opening the
  microphone is a user-visible privacy effect and cannot follow the existing
  "always report, the sink decides" rule. `present.json` gains
  `rehearsalAudio: bool`. Persistence is still decided in exactly one place:
  the server's sink. Without the flag the audio endpoint answers 404.
- The presenter calls `getUserMedia({audio:true})` at load when
  `rehearsalAudio` is true (the timer auto-starts on the first slide advance,
  so acquiring lazily would lose the opening words behind a permission
  prompt). `http://localhost` is a secure context; the presenter profile is
  persistent, so the permission prompt appears once.
- Recorder state is a pure function of the shell's actual timer state after
  each event, handled in one module (`rehearsalAudio.ts`) listening after the
  shell on the same bus as the reporter. Invalid requests that the shell
  ignores therefore cannot move the recorder:
  - start → `recorder.start(5000)` (5 s timeslice)
  - pause → `recorder.pause()`; resume → `recorder.resume()`
  - reset (local or adopted) → stop and discard; the next start begins a new
    take whose first chunk truncates the file
  - close request → `recorder.stop()`, await final `dataavailable` and drain
    without `keepalive`; `pagehide` does the same as a best-effort fallback
    with `keepalive`
- `POST /rehearsal/audio?take=<id>&seq=N&startMs=M`, body = raw chunk bytes
  (`Content-Type: audio/webm`). All three query values are required;
  `seq` and `startMs` are strict u64 values. `take` is a random id the client
  mints per recorder start. Chunks of one take concatenated in order are a
  valid WebM stream (only chunk 0 carries the header), so **order is an
  invariant, not a convention**: the client posts through one serialized queue, and the server
  tracks `(take, next_seq)` under the sink mutex — a new `take` with `seq == 0`
  truncates/creates and becomes current; current `take` with `seq == next_seq`
  appends; anything else is 409 whose JSON body carries the current
  `{take, nextSeq}`. That body makes retries idempotent without guessing: when
  a response was lost after a successful append, the retry's 409 shows
  `nextSeq == seq + 1` for the same take, which proves the head chunk is
  persisted, so the queue drops it and continues. Any other 409 keeps the head
  and stays visibly failed. Chunks are never skipped (a gap would corrupt
  everything after it). A second presenter window recording concurrently
  simply loses the take and shows the error — last take wins, never an
  interleaved file.
- **Reset is a sink-side event, not only a client-side one.** Every zeroed
  snapshot (`elapsedMs == 0`, empty timeline) clears the current take and any
  latched audio write error, attempts to delete the same-stem `.webm`, and
  clears `audio` in the record in the same mutex hold as the JSON rewrite.
  Cleanup is attempted even without a current take, and a failed removal is a
  terminal warning naming the path rather than a silent surviving file.
- File: `.peitho/rehearsals/rehearsal-<stamp>[-N].webm`, same stem as the
  session's `.json`. The session (stamp reservation) is created by the first
  snapshot as today; the reporter additionally reports on timer start so the
  session exists before the first chunk (≥ 5 s later). A chunk arriving with no
  session is 409 and is retried by the queue — no second reservation path.
- The record gains `audio: Option<{ file, startMs }>`, set once the first chunk
  is accepted. `file` is the same-stem `.webm` name; `startMs` is the timer
  position at which the take began. Audio position equals the timer only while
  the recorder is slaved to it, and a take does not always begin at 0: the
  presenter may adopt an already-running timer (reload, late open), or the
  timer may start while the first-run microphone permission prompt is still
  open. The seek position of a timeline entry is therefore
  `atMs - audio.startMs` (entries before `startMs` have no audio). The client
  sends `startMs` with every chunk request (`?take=&seq=&startMs=`); the sink
  reads it at `seq == 0`. When the offset rounds to at least one second (a
  normal start measures `startMs` of about 1 ms), `peitho rehearsal` prints it
  and adds a `seek` column to the slide table (`-` for a slide whose first
  visit ended before the audio began); it also notes when the named file is
  missing on disk. Because microphone permission is keyed by origin, `--audio`
  pins the same fixed default port as `--host` so the prompt appears once.
  `parse_rehearsal_filename` only matches `.json`, so `.webm` files never enter
  record selection.
- A take can only begin while the session describes a started run: `seq == 0`
  is accepted only when the latest snapshot has a non-empty timeline. After a
  reset the latest snapshot is zeroed, so a stale old-take chunk that was in
  flight across the reset cannot resurrect a recording (409, and the client
  never retries old-take chunks).
- Measured 2026-09-18 (real Chrome, fake media device): `pause()`/`resume()`
  is gapless (no pts gaps across a 10 s wall-clock pause), audio length matched
  `elapsedMs - audio.startMs` within 30 ms, ffmpeg decodes the concatenation,
  and every tested `atMs - audio.startMs` seek works. Chrome's WebM carries no duration;
  `ffmpeg -i in.webm -c copy out.webm` adds one for players that need it.
- **Closing is a two-step transition.** The final chunk only exists after
  `recorder.stop()` fires `dataavailable` asynchronously, so it cannot be
  produced from `pagehide` (measured: every close lost the last chunk and the
  final snapshot). The sync bridge dispatches `peitho:beforeclose` with
  `waitUntil(promise)`, waits at most 2 s, then closes; the server's exit grace
  is 3 s when a rehearsal sink exists (500 ms otherwise). These awaited final
  POSTs do not use `keepalive`, avoiding its 64 KiB in-flight body cap.
  `pagehide` stays as the keepalive fallback for the window close button and
  display swap.
- Chunk size cap on the endpoint (5 s of Opus is tens of KB; cap at a few MB)
  so a bad client cannot fill the disk through one request.

### Visibility (author-approved 2026-09-18)

Issue #288 decided "the presenter renders nothing rehearsal-specific". Audio
strains that: a microphone that is silently recording, or silently **not**
recording because permission was denied / no input device exists, is the
silent path this project forbids. Decision (narrowing the #288 rule, approved
by the author): a single small presenter
indicator, shown only when `rehearsalAudio` is true — `● REC` while recording,
`❙❙ REC` while paused, and a visible error (`mic unavailable: <reason>`) when
`getUserMedia`/`MediaRecorder` fails or chunk posts keep failing. The terminal
line becomes `recording rehearsal (with audio) to .peitho/rehearsals/`.

## File structure

```
crates/peitho-core/src/rehearsal.rs     v2 types: RehearsalSlideEntry, timeline, audio; v1 read compat
bindings/                               regenerated (RehearsalAudio.ts, RehearsalSlideEntry.ts, updated Snapshot/Record)
crates/peitho/src/server.rs             RehearsalSink: timeline validation, audio append (seq), POST /rehearsal/audio
crates/peitho/src/main.rs               --audio flag, present.json rehearsalAudio, `peitho rehearsal` per-slide table + audio path
packages/peitho-present/src/slideTimeline.ts     timeline accumulator (new)
packages/peitho-present/src/rehearsalReporter.ts timeline in snapshot, version 2, report on start
packages/peitho-present/src/rehearsalAudio.ts    recorder state machine + serialized chunk queue (new)
packages/peitho-present/src/presenter.ts         wiring + indicator
```

## Edge cases and constraints

| Case | Behaviour |
| --- | --- |
| Timer never starts | No json, no webm, no directory (unchanged) |
| Reset mid-rehearsal | Timeline cleared; the zeroed snapshot overwrites json, deletes the webm and clears `audio` in one sink step; the next start is a new take |
| Display swap mid-rehearsal | Presenter navigates, recorder dies with the page, timer resets (known tradeoff); the new page starts a new take that truncates — same information-loss class as the timer itself |
| Mic permission denied / no device | Visible presenter error; timeline and section recording continue; record has `audio: null` |
| Chunk POST fails | Retried in order; indicator shows the error while failing; never skipped |
| Presenter closed by Esc | The echoed close message triggers `peitho:beforeclose`; `stop()` produces the final chunk, both final POSTs drain without `keepalive`, then the window closes (2 s client cap, 3 s rehearsal-server grace) |
| Server killed mid-talk | webm valid up to the last appended chunk; json as of last snapshot (unchanged) |
| Slide keys change between rehearsal and review | Timeline stores key + index at record time; `peitho rehearsal` prints them as recorded and never re-reads the deck |
| `dist/` / caches | `.peitho/rehearsals/` stays outside every build output (unchanged) |

To verify by measurement during implementation (real Chrome, not jsdom):

- `MediaRecorder.pause()`/`resume()` produces a gapless timeline, i.e. audio
  position tracks `elapsedMs - audio.startMs` across pauses. If it does not,
  stopping and starting a new take per resume is **not** an acceptable fallback
  (breaks single-file append) — instead record pause spans in the record and
  document the offset.
- Chrome's MediaRecorder WebM has no duration/cues; confirm `ffmpeg`/whisper
  consume it and that seeking works well enough in a player, or document
  `ffmpeg -i in.webm -c copy out.webm` as the remux step.
