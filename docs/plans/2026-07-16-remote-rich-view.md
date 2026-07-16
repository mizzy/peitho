# Richer /remote page: preview, title/progress, sections, notes, pace, timer controls (Issue #299)

Design finalized in Claude Design ("Sky Tonal Pill", 2026-07-16, author-approved).
Scope decided by the author: all four candidate elements from the issue, plus a
hare-and-tortoise pace indicator and start/pause/resume timer control. Notes get
no gating beyond the existing `--host` opt-in. Layout is notes-first. The full
agenda list is intentionally excluded (phone real estate); only a one-line
current-section indicator is shown.

## Final visual spec (from the approved mock)

Phone-portrait column, top to bottom (page `gap: 12px`, padding `14px 14px 18px`,
background `#101216`, text `#f5f7fb`):

1. **Slide preview** — 16:9 box, `border-radius: 10px`, `border: 1px solid #2a3240`,
   white background; renders the *current* slide via the same canvas approach as
   the presenter's preview pane (scaled real slide, not a screenshot).
2. **Title bar** — flex row: slide title (`16px/700`, nowrap + ellipsis) left,
   counter `8 / 24` (`16px/700`, `#aab3c2`, tabular-nums) right.
3. **Progress bar** — track `height: 5px`, `border-radius: 3px`, `#232935`;
   fill = accent `#38bdf8` (no glow — tonal style), `border-radius: 3px`;
   **plan tick**: absolute 2px×12px bar (`#5a6473`, opacity 0.7, `top: -4px`)
   marking where the plan says you should be.
4. **Pace row** — flex row, `gap: 12px`, `font-size: 14px`, tabular-nums:
   - **Timer button** 44px circle. Running: background `rgba(56,189,248,0.18)`,
     pause icon (two 3×13px bars) in accent. Stopped/paused: background `#232935`,
     white play triangle (CSS border triangle, 11px wide, `margin-left: 3px`).
   - **Elapsed / planned** — elapsed white `600`, `/` separator `#6b7484` with
     4px side margins, planned `#c3ccd9 500`. Fills remaining width.
   - **Pace chip** — pill (`border-radius: 999px`, `padding: 3px 9px`, `12px/600`,
     no border): behind = 🐢 + `3:10 behind` on `rgba(232,192,122,0.12)` /
     `#e8c07a`; ahead = 🐇 + `0:50 ahead` on `rgba(143,217,160,0.12)` / `#8fd9a0`;
     paused = text `Paused` (no emoji) on `rgba(170,179,194,0.12)` / `#aab3c2`.
     Emoji spans get `font-size: 13px` and `filter: saturate(0.85)`.
5. **Section line** — `12.5px`, `#aab3c2`: `<b>Architecture</b> · slide 3 / 6 in section`
   (name `#dde3ec 600`). Hidden entirely when the deck has no sections.
6. **Notes panel** — flexible remainder (`flex: 1; min-height: 0`), background
   `#181c23`, `border: 1px solid #2a3240`, `border-radius: 12px`, padding
   `14px 16px`. Caption `NOTES` (`11px/600`, uppercase, `letter-spacing: 0.08em`,
   `#6b7484`). Body `15px/1.65 #dde3ec`, `white-space: pre-wrap`, scrollable
   (`overflow-y: auto`), rendered as plaintext `textContent` (same v1 policy as
   the presenter; Markdown notes remain a separate undecided item). Empty note:
   dimmed italic placeholder `No notes for this slide` (`#5a6473`, 14px).
7. **Status line** — center, `13px #aab3c2`; shows `Ended` after close.
8. **Prev/Next buttons** — grid `1fr 1fr`, `gap: 10px`, pill
   (`border-radius: 999px`), `min-height: 60px`, `font: 600 17px`, with `‹`/`›`
   arrow spans (`20px/400`, opacity 0.85). Previous: transparent background,
   `1px solid #2f3644`, `#dde3ec`. Next: `rgba(56,189,248,0.14)` background,
   accent text. Disabled: `opacity: 0.32` + `disabled` attribute.

Button/row states (author-reviewed):

- First navigable slide → Previous disabled; last navigable slide → Next disabled.
  Disabled means "no non-skipped slide exists in that direction" (skip-aware,
  recomputed on every index change).
- Ended (close message received): everything disabled, status `Ended`, panels
  dimmed to 0.35 opacity, sync channel closed. This is the "server is gone"
  state, distinct from end-of-deck.
- Deck without `time` frontmatter (`plannedDurationMs: null`): pace chip, plan
  tick, and `/ planned` are omitted; timer button + elapsed still render.
- Deck without sections: section line omitted (no empty row).
- Timer states mirror the presenter's `TimerState` machine: stopped → play icon,
  no chip; running → pause icon + ahead/behind chip; paused → play icon +
  neutral `Paused` chip.

## Architecture

### Timer state must ride /sync (the one protocol change)

The timer currently lives inside each window's `PresentShell`
(`startedAtValue`/`pausedAtValue`/`pausedTotalMs`, transitions via
`peitho:timercontrol` request events). Nothing about it crosses the server, so
the remote can neither display elapsed time nor control the timer.

Chosen design — **absolute timer state as a third sync state**, exactly like
`index`/`swapped` (the alternative, forwarding `timercontrol` commands, is
ruled out by the documented invariant that sync state must be absolute because
the channel coalesces):

- `POST /sync` additionally accepts `{"timer":{"running":bool,"elapsedMs":u64}}`
  — the poster's elapsed at the moment of the transition. The server stamps it
  with its own clock (`atMs`, epoch ms) when storing.
- `SyncState` gains `timer: Option<TimerSyncState { running, elapsed_ms, at_ms }>`.
- Every `/sync` JSON GET response (handshake and 200 poll) carries
  `"timer":{...}|null` **and** `"nowMs":<server clock at response time>` on top
  of the existing `seq`/`message`/`index`/`swapped`/`generation`.
- Client rendering never compares clocks across devices: displayed elapsed =
  `elapsedMs + (running ? (nowMs - atMs) + millis since the response arrived : 0)`.
  Only the server's clock is compared with itself; the local advance between
  polls uses the client's own clock for at most one poll cycle, and every
  response re-anchors. Last-write-wins on concurrent transitions; replay is
  idempotent (same convergence story as `swapped`).
- Timer transitions may be executed by any client. The presenter keeps owning
  its local shell transition (Space / buttons) but its sync bridge now also
  posts the resulting absolute state; incoming timer state is applied to the
  shell. The remote computes transitions from its last-synced state and posts
  the new absolute state. The shell needs one new capability: adopt an absolute
  external timer state (elapsed + running) instead of only deriving from local
  `Date.now()` bookkeeping.

### Slide preview

Reuse the presenter's preview-pane approach: `mountPresentShell` onto a pane
with `interactive: false`-equivalent options and a viewport sized to the pane
(see `presenter.ts` `previewRoot` + `paneViewport`). The present cache already
serves `slides/*.html`, `peitho.css`, and fonts; `remote.html` needs the same
canvas CSS custom properties as `present.html`/`presenter.html`, so
`render_remote_index()` takes the aspect ratio and runs through
`fill_canvas_tokens` like the other templates. The preview shell follows
`currentIndex` via the same bus events the remote already handles (no second
sync channel; drive it locally like the presenter drives its preview pane).

### Notes / titles / sections / progress

All from already-served static assets: `manifest.json`
(`slides[].text.title`, `slideCount`, `sections`, `plannedDurationMs`,
`slides[].key`) and `notes.json` (`Notes.notes` keyed by slide key). Fetch both
at mount (`notes.json` failure degrades to empty notes with a console error —
the file always exists in the present cache, but the remote must not die on it).
Current section = the `ManifestSection` whose `startIndex..=endIndex` contains
the index (reuse `sectionIndexForSlide` from `sections.ts`).

### Pace math (hare and tortoise)

Only when `plannedDurationMs` is non-null. Expected-elapsed mapping over slide
positions, piecewise by sections when present:

- With sections: expected elapsed *at the start of* slide `i` = sum of
  `plannedDurationMs` of all sections fully before the section containing `i`,
  plus that section's duration × `(i - startIndex) / sectionSlideCount`.
- Without sections: `plannedDurationMs × i / slideCount`.
- Delta = actual elapsed − expected(currentIndex). Positive → behind (🐢),
  negative → ahead (🐇), formatted `m:ss` via the existing
  `formatMinuteSeconds`. Zero-planned decks can't happen (`PlannedTime` is
  validated nonzero at construction).
- Plan tick position = the inverse mapping: given elapsed, walk the same
  piecewise curve to find the slide-position fraction the plan expects,
  clamped to [0, 1]. Progress fill = `index / (slideCount - 1)` (100% on the
  last slide; guard slideCount == 1).

## Implementation tasks (TDD, in order)

Each task: failing test first, then implementation, then the full gate list.

### 1. peitho-core / server: timer sync state

- `crates/peitho/src/server.rs`: `SyncTimerMessage { running, elapsed_ms }`
  accepted by `POST /sync` (serde, same rejection rules as existing messages —
  unknown shapes are still 400); `SyncState.timer: Option<TimerSyncState>`
  stamped with server epoch ms on receipt; `SyncSnapshot` + `sync_response_body`
  emit `"timer":{"running":…,"elapsedMs":…,"atMs":…}|null` and `"nowMs":…` in
  **every** JSON GET response (handshake and 200 poll). Tests: hub stores/replays
  timer state; response body includes `timer`/`nowMs` on handshake and poll;
  invalid timer POST is rejected; coalescing keeps the latest absolute state.

### 2. render.rs: remote template

- `render_remote_index(aspect_ratio)` — canvas tokens + full final CSS from the
  visual spec above + the same namespace-import/feature-detection loader
  (feature-detect the new mount signature). Update `main.rs`/`server.rs` callers
  and the existing template tests.

### 3. peitho-present: sync plumbing

- `sync.ts`: `SyncMessage` gains `{ timer: { running, elapsedMs } }`;
  `isTimerSyncMessage` guard; the server channel's poll handler surfaces
  `timer`/`nowMs` absolute state the same way it replays `index`/`swapped`
  (per-poll replay is load-bearing — a window that misses a coalesced-away
  transition converges on the next poll). Tests in `sync.test.ts` mirror the
  swapped-replay ones.
- `shell.ts`: new method to adopt absolute timer state (set elapsed base +
  running flag from an external snapshot) — keeps `elapsedMs()` semantics;
  `deriveTimerState` in the presenter must see the adopted state. Tests: adopt
  while stopped/running/paused; elapsed advances only when running.
- `presenter.ts` sync bridge: post `{timer}` after every local transition;
  apply incoming timer state to the shell (idempotent replay). Tests via the
  existing presenter test harness.

### 4. peitho-present: remote view rebuild (`remote.ts` + `remote.test.ts`)

- Fetch manifest + notes; mount preview shell; render title/counter, progress
  fill + plan tick, pace row (timer button, elapsed/planned, chip), section
  line, notes panel, status, prev/next pills per the spec and state rules above.
- Timer button dispatches `peitho:timercontrol`-style request events on the
  remote bus; the remote sync bridge executes the transition (compute new
  absolute state from last-synced state + local advance) and posts `{timer}`
  (§16: components emit requests; the bridge owns transitions).
- Skip-aware disabled recomputation on every index change; ended state per
  spec. A 1s interval re-renders elapsed/pace while running (cleared on
  destroy/ended; no interval when stopped and nothing running).
- jsdom tests for every state row in "Button/row states" above, plus pace math
  unit tests (sections piecewise, no sections, clamping, ahead/behind/paused
  chip selection, null planned time).

### 5. Embedded bundles + gates

- `npm run build` → commit `dist/remote.js` (+ `shell.js`/`preview.js` if they
  drift); all gates from CLAUDE.md including the 3× `cargo test --workspace`.

### 6. E2E (manual, before PR)

- `peitho present --host --port <fixed>` on an example deck with sections +
  notes; phone-simulated browser window on `/remote`: verify preview follows
  navigation, notes/title/section update, timer start on remote reflects on
  presenter (and vice versa), pace chip flips ahead/behind, first/last slide
  button disabling, Esc → Ended. Screenshot evidence.

## Review-driven design amendments (2026-07-16, adversarial review round)

The multi-agent review of the first implementation confirmed six correctness
bugs in the timer-sync path plus a landscape-layout regression. The fixes
below are part of the design, not incidental patches:

- **Handshake gating (`{synced:true}`)**: a client must not publish absolute
  state, nor offer controls, before the handshake snapshot has been applied.
  The server channel delivers replay state in the order timer → index →
  swapped and then surfaces a one-time local `{synced:true}` data event; the
  BroadcastChannel factory surfaces it immediately (locally, never across).
  `installSyncBridge` drops outgoing `{timer}` posts until synced; the remote
  mounts its controls disabled and enables them on the synced signal. This
  kills two bugs: a reloaded/swapped window auto-start (timeTracker fires on
  the replayed index navigation) rebroadcasting `elapsedMs: 0` over a live
  timer, and a pre-handshake phone tap stomping live timer/index state.
- **POST `/sync` acks `{"seq":N}`**: the channel records the highest acked
  post seq and skips absolute-state replay from any response with a lower seq
  (transient messages — especially `close` — are never skipped). Prevents an
  in-flight poll response from reverting a just-posted pause/resume.
- **`peitho:timeradopt` event**: `adoptTimerState` announces discontinuous
  rebasing on a dedicated event (never re-posted, unlike `peitho:timerchange`);
  the agenda listens and rebases its `lastElapsedMs` accumulator without
  attributing the jump to the current section's actual time.
- **Monotonic server clock**: `atMs`/`nowMs` are milliseconds since a
  process-wide `Instant` (OnceLock), not wall clock, so NTP steps cannot shift
  reconstructed elapsed. Only server-relative differences are ever consumed.
- **Rounded wire elapsed**: both post sites round `elapsedMs` to integers
  (serde deserializes u64; a fractional value would 400 every post).
- **Bounded layout chain**: `html/body` and `#peitho-remote-root` use fixed
  `height: 100svh` (not `min-height`) so flex shrinking actually engages;
  preview (`flex: 0 1 auto; min-height: 0`) and notes compress on landscape
  phones while the pace row and Previous/Next always stay visible (preserves
  the 7d10b2a guarantee).
- **Preview shell reuses the controller's manifest** (`ShellOptions.manifest`)
  instead of a second `manifest.json` fetch, and the 1 s timer tick updates
  only time-dependent chrome in place (persistent tick/chip/planned elements;
  notes text is written only when changed) so iOS momentum scrolling of long
  notes survives.
- `installSyncBridge` takes a `hooks` object (`closeWindow`/`pathname`/
  `navigate`/`adoptTimerState`) instead of positional defaults restated at
  every caller.

## Non-goals

- Markdown rendering of notes (undecided item — plaintext only).
- Full agenda list on the phone.
- Any notes gating beyond `--host`.
- Landscape-phone layout tuning (portrait-first; nothing breaks, just untuned).
