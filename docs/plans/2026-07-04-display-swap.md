# Display swap: slides ⇄ presenter (Issue #108)

Date: 2026-07-04
Issue: #108 — Swap slides and presenter view between the two displays

## Goal

A single action, available while `peitho present` is running, that puts the
slides and the presenter view on the opposite displays from where they are
now. Fast, keyboard-reachable, no restart. This is an **escape hatch for
display misidentification**, not an attempt to improve identification itself.

## Approach decision

The issue offers two approaches and explicitly delegates the choice
("pick whichever is easier to implement — author's call, 2026-07-04"),
while noting that **Approach A (swap the displayed content) is the natural
fit** for the escape-hatch framing. All three design lenses select A:

- **Root cause / fit**: the recovery action is "wrong content on this
  display" — swapping content fixes exactly that, with no OS window
  manipulation and none of the known Chrome placement/fullscreen pitfalls
  that make Approach B fragile (placement flags only honored at launch,
  separate profiles, fullscreen Space re-entry).
- **Long-term**: A adds one synced state field to an existing channel;
  B adds an OS-window-management subsystem that every future platform
  (Linux, #22) would have to reimplement.
- **Type safety**: A's state is a single `bool` validated at the existing
  `/sync` seam; B has no typed seam at all (AppleScript/CGWindow calls).

**Chosen: Approach A**, realized as *URL role-switching*: each window
toggles its content by navigating (`location.replace`) to the other role's
route. Role = URL is already the shell's structure (present.html vs
/presenter); the swap reuses it instead of inventing an in-page remount
(which would require moving the presenter page's entire `<style>`/font
setup into the shared bundle).

## Design

### 1. Sync protocol: absolute state, replayed at join

The `/sync` long-polling channel coalesces messages (a poller that misses
intermediate seqs receives only the latest message). A "toggle" command
over a coalescing channel is broken: two rapid swaps can reach one window
as two messages and another as one, leaving both windows on the same role.
Therefore the swap message carries **absolute state**:

```
{"swapped": true|false}
```

Last-wins under coalescing, so all windows converge.

The server (`SyncHub`/`SyncState` in `crates/peitho/src/server.rs`) now
tracks current presentation state, updated on every valid `POST /sync`:

- `index: Option<usize>` — last `{index}` message (None until first nav)
- `swapped: bool` — last `{swapped}` message (false initially)

**Every** `/sync` GET response — the join handshake (no/empty seq) *and*
each long-poll response — carries the current state alongside the usual
fields (handshake has `message: null`):

```
{"seq": N, "message": M|null, "index": I|null, "swapped": B}
```

The client channel (`serverSyncChannelFactory` in
`packages/peitho-present/src/sync.ts`) synthesizes onmessage deliveries
for the state after a successful handshake and after each delivered poll
message (`{index}` if non-null, then `{swapped}`), so `installSyncBridge`
handles live and replayed state uniformly and idempotently.

**Why state rides every response, not just the handshake**: a swap
navigates both pages; without index replay at join every swap would
reset the deck to slide 0. But handshake-only replay is not enough for
convergence: the channel coalesces (a poller only ever receives the
*latest* message), so a live window that misses `{swapped:true}` — e.g.
it sat in its 1s error-retry delay while a later `{index}` post bumped
seq past the swap — would stay on the wrong role forever, because a
window that never reloads never re-handshakes. The per-poll state replay
is what makes "last-wins under coalescing, so all windows converge"
actually true: any single successful poll brings a window to full
convergence. It also closes the pre-existing gap where a mid-talk reload
of a window desynced it until the next navigation (the issue's "same as
the current slide index" expectation). Old shell bundles ignore the
extra fields (they only read `seq` and `message`), so a stale `--shell`
degrades gracefully.

### 2. Routes: window identity survives the swap

A window must know its *intrinsic identity* (which OS window it is) apart
from its *current content role*, or a second swap couldn't route it back.
Identity is encoded in the path — four routes, two files:

| route                | serves         | window            | content   | swapped |
|----------------------|----------------|-------------------|-----------|---------|
| `/present.html`, `/` | present.html   | slides window     | slides    | false   |
| `/presenter-swapped` | presenter.html | slides window     | presenter | true    |
| `/presenter`, `/presenter.html` | presenter.html | presenter window | presenter | false |
| `/present-swapped`   | present.html   | presenter window  | slides    | true    |

Counterpart pairs (same window, other role):
`/present.html` ⇄ `/presenter-swapped`, `/presenter` ⇄ `/present-swapped`.

Both new routes are extensionless and dot-free. Distinct paths keep
Chrome's `browser.app_window_placement` keys distinct (`localhost_/presenter`
vs `localhost_/present-swapped`), so a swapped session can never clobber
the unswapped presenter placement that the next launch restores.

`resolve_request_path` in server.rs gains the two aliases. Nothing enters
`dist/` — these are present-cache routes only; the publish contamination
check is unaffected.

### 3. Client: request event + route helpers (§16 layering)

New module `packages/peitho-present/src/swap.ts`:

- One frozen route table with a single lookup,
  `swapRoute(pathname): { swapped, counterpart } | null` (`null` for
  unknown paths — the bridge logs a console.error per attempt; no silent
  fallback). Keeping both fields in one record makes "a counterpart
  exists iff the route is known" structural.
- `installSwapShortcut(win, bus)` — `s`/`S` keydown → dispatch
  `peitho:swaprequest`. (`f` is taken by fullscreen in the slides window;
  Space by playpause in the presenter.) Chord-modified presses
  (meta/ctrl/alt) are ignored via a shared `hasChordModifier` predicate
  in keyboard.ts so Cmd+S keeps its browser meaning; the same guard is
  applied to the pre-existing fullscreen and navigation shortcuts, which
  had the identical unguarded-chord bug class (Cmd+F, Cmd+Arrow).

Per §16 the UI only emits the request; `installSyncBridge` (the transport
bridge, which already executes `close`) does the execution:

- on `peitho:swaprequest` → `postMessage({swapped: !routeSwapped(path)})`
- on `{swapped}` message → if it differs from the current route's swap
  state, `location.replace(swapCounterpart(path))`. Equal state = no-op,
  so echoes and replays are idempotent and the system converges.

`location.replace` keeps the single history entry, preserving the
`window.close()` close flow. Pathname source and navigation are injectable
for tests (jsdom cannot navigate).

### 4. Entry pages (`crates/peitho-core/src/render.rs`)

**present.html**
- Move `installSyncBridge` to *after* `mountPresentShell` — the handshake
  replay dispatches `peitho:navigate`, which needs a mounted shell.
  (Also stops the page's initial `show(0)` from broadcasting `{index:0}`
  on every load — previously a reloading slides window yanked the whole
  presentation back to slide 0.)
- Load `present.json` unconditionally (today it loads only when a planned
  time exists) and gate `installSwapShortcut` on `config.presenterOpen` —
  a solo slides window (`--no-presenter`) must not be able to swap itself
  into a presenter with no slides anywhere. The presenter page still
  installs the shortcut unconditionally, so a manually-opened presenter
  popup can drive the swap even under `--no-open`.

**presenter view** (`presenter.ts` template + presenter page CSS)
- A `Swap` button in the controls row (`data-peitho-action="swap"`,
  emits `peitho:swaprequest`; grid gains one column) and an `S swap`
  entry in the kbd hint bar.

### 5. Known consequences (documented tradeoffs)

- **Timer resets on swap.** The presenter's timer lives in the page;
  navigation remounts it. The escape hatch targets the start of a talk
  (before the timer runs); carrying timer state through the swap would
  drag timer sync into the protocol and is out of scope.
- **Post-swap slides are windowed.** Fullscreen-ness stays with the OS
  window, so after a swap the slides sit in the windowed `--app` window on
  the (now correct) audience display. `f` (existing fullscreen shortcut,
  installed by the slides content) fullscreens it; requestFullscreen needs
  user activation, so it cannot be automated from a broadcast.

## Implementation order (TDD)

1. **server.rs**: `SyncMessage::Swap` variant (`deny_unknown_fields`),
   state tracking on POST, `index`/`swapped` on every `/sync` GET body
   (handshake and poll, shared builder), new route aliases. Tests: route
   resolution (incl. query strings), swap POST accepted / invalid
   rejected (400), handshake and poll bodies reflect broadcast state,
   close doesn't clobber state.
2. **swap.ts**: pure route helpers + shortcut installer. Tests: full
   route table, unknown path → null, `s` dispatches request, cleanup.
3. **sync.ts**: `SyncMessage` union + state replay on handshake and
   after each poll message + swaprequest posting + swapped-message
   navigation. Tests: replay order/idempotence, convergence from a poll
   whose message is unrelated, navigation only on mismatch, unknown-path
   errors with console.error, post payload correctness.
4. **presenter.ts / render.rs**: Swap button, kbd hint, entry-page
   rewiring (bridge-after-mount, unconditional present.json, gated
   shortcut). Tests: render.rs string assertions + presenter view button
   emits request.
5. **index.ts** exports; rebuild committed `shell.js`.
6. Gates + E2E (fixed `--port`, `curl` handshake/POST `{"swapped":true}`,
   `screencapture` both windows).

## Out of scope

- Making initial display identification smarter (explicitly excluded by
  the issue; #22 for Linux enumeration).
- Timer state carry-over across swap (above).
- Markdown notes rendering, etc. — untouched.
