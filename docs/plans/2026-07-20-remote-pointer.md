# Remote pointer (minimal)

Date: 2026-07-20
Status: Draft — awaiting author approval before implementation

## Goal

Let the presenter, holding the phone remote (`/remote`), point at things on the slides window in real time. A tap-and-hold on the remote's slide-preview area shows a red dot on the slides display; releasing removes it. Nothing is persisted; nothing enters `dist/`.

Non-goals for v1:
- Drawing / stroke retention (deferred — clean up first, add later if desired)
- Color / size pickers (fixed red, fixed radius)
- Overlay on presenter window (slides only)
- Pointer-as-navigation gesture (Next/Prev buttons stay authoritative)

## Why in-scope

The remote is already the physical device in the presenter's hand (Issue #306/#309), and Home Screen install / landscape polish (PR #311) means the ergonomics for touch input are already solid. Pointer is the smallest addition that turns the phone from "clicker" into "clicker + laser pointer" without touching the deck contract or the three pillars.

## UX

Add a single toggle at the bottom of `/remote`, next to existing controls:

```
[ Off | Pointer ]
```

- Default Off. Selection is session-only (no localStorage; a reload resets to Off).
- With Pointer on, the remote's slide-preview area (the black 16:9 region already present in the landscape layout) becomes a touch surface:
  - `touchstart` / `pointerdown` inside the region → normalized `{x,y}` in `[0,1]²` is broadcast; slides window paints a red dot at the corresponding position.
  - Movement while held → normalized coordinates continue to broadcast.
  - `touchend` / `pointerup` / `pointercancel` / touch leaves the region → broadcast `pointerup`; slides fades the dot in ~150ms.
- Next/Prev buttons remain active. Timer control area is unaffected.
- Chord-modified taps (unlikely on iOS but possible on trackpad-connected iPads) are ignored, consistent with `hasChordModifier` guard elsewhere.

Slides window:
- One overlay `<canvas>` layered above the slide DOM at `z-index` above content, below the presenter chrome (there is no presenter chrome on the slides window, but keep the layering explicit).
- Dot is a filled circle, radius ~1.2% of the shorter viewport dimension, color `#ff2a2a`, `mix-blend-mode: multiply` for legibility on both light and dark backgrounds. No trail.
- `requestAnimationFrame` interpolates between received coordinates so the visual motion is smooth even if the transport batches.
- On `peitho:navigate` (existing shell event) the overlay is cleared regardless of pointer state — pointer never survives a slide change.
- On sync `session` change (server restart) the overlay is cleared.
- Presenter window renders nothing pointer-related in v1.

## Transport: `/pointer` endpoint (new)

The existing channels don't fit:
- `/sync` is absolute-state + long poll with coalescing — designed for one integer `index`, not a coordinate stream.
- `/rehearsal` is one-shot POST for durable records.

Add a third endpoint that reuses the long-poll shape but carries a small event stream:

- `POST /pointer` — body `{"move":{"x":0.42,"y":0.71}}` or `{"up":true}`. Returns `{"seq":N}`.
- `GET /pointer` — handshake, returns `{"seq":N,"session":"..."}` (same session id as `/sync`).
- `GET /pointer?seq=N` — long poll (up to ~5s, shorter than `/sync`'s 30s to bound worst-case dot latency after transport hiccups; empty 204 on timeout).

Server retains only the **latest** event per session, plus a monotonically increasing `seq`. Missed intermediate `move`s are acceptable — the client only needs "where is the pointer now" and "is it up". `up` is sticky until the next `move` (so a client that joins mid-idle sees "up", not a stale coordinate).

Coordinate sending is throttled to ~30Hz on the remote side (`requestAnimationFrame`, coalesce to one POST per frame). Reduces battery drain and matches what the slides can visibly render. Requests use `fetch` with `keepalive: true` so a fast release still delivers the final `up`.

Rationale for not using WebSocket in v1:
- tiny_http doesn't support upgrades; adding tungstenite or a second listener is more surface than a POST endpoint.
- iOS PWA WebSocket is workable but has more edge cases (cold connect, backgrounding) than short POSTs.
- The `POST + long-poll` shape mirrors `/sync`, so operators reason about one transport, not two.

If real-world latency measurement shows POST is inadequate, a WebSocket variant can be added later behind the same `peitho:pointer*` event contract with no UI change.

## §16 event contract

Remote (only emits requests):
- `peitho:pointermodechange { mode: "off" | "pointer" }`
- `peitho:pointermove { x: number, y: number }` (normalized)
- `peitho:pointerup`

Remote shell (bridge to transport):
- On `pointermode: pointer`, install touch/pointer listeners on the preview region.
- On `pointermode: off`, remove listeners and POST a final `up`.
- Debounce/throttle to ~30Hz.

Slides shell (bridge from transport to overlay):
- Polls `/pointer` in parallel with `/sync` (independent long poll loop).
- On `move`, updates a single `PointerState { x, y, visible: true }`.
- On `up`, sets `visible: false` and fades out.
- On `peitho:navigate` or `session` change, clears state.

Presenter shell: no pointer subscription in v1.

## Types (ts-rs)

Domain in `peitho-core` remains untouched (pointer is runtime-only, no deck contract change). The pointer message type lives in the server crate and is not exported to `bindings/` — it's a transport concern, not a domain contract.

## Files touched

- `crates/peitho/src/server.rs` — add `/pointer` GET/POST routes, `PointerHub` (mirrors `SyncHub` but simpler: single latest event + seq).
- `packages/peitho-present/src/remote.ts` — mode toggle, touch listeners, POST loop, region hit-testing.
- `packages/peitho-present/src/shell.ts` — pointer overlay canvas, long-poll loop, fade animation, clear-on-navigate.
- `packages/peitho-present/src/sync.ts` — no change (pointer is a separate transport).
- Tests in Rust: `PointerHub` seq monotonicity, "up sticky until next move", session-scoped clear on new session.
- Tests in vitest: mode toggle emits `peitho:pointermodechange`; slides overlay clears on `peitho:navigate`; fade timer.

## Gates additions

Existing gates cover this (Rust tests × 3, clippy, fmt, bindings drift not applicable since no ts-rs change, vitest, embedded shell/remote drift after `npm run build`).

E2E verification before merging (real display required):
1. `peitho present --host deck.md` on macOS, phone joins `/remote`.
2. Toggle Pointer on, tap-drag inside the preview area, confirm red dot tracks on the slides display.
3. Release, confirm dot fades.
4. Advance slide via Next button, confirm any residual dot clears immediately.
5. Toggle back to Off, confirm dot goes away and further taps do nothing.
6. Kill server, restart, rejoin from phone, confirm no stale dot.

## Deferred (explicitly not in v1)

- Draw mode with per-slide stroke retention
- Color / size selectors
- Presenter-window pointer preview
- Pointer analytics in rehearsal records (would violate rehearsal's terminal-only scope)
- Multi-presenter pointer (multiple remotes at once) — server keeps only one latest event, so a second remote would race; v1 assumes one presenter

## Undecided — awaiting author

- Whether to ship v1 without draw at all (this plan assumes yes; if draw is wanted from day one, the transport doesn't change but the overlay/state model grows).
- Whether Off should be the persisted default per-device (plan: no persistence in v1).
- Dot color/size (plan: fixed `#ff2a2a`, 1.2% of min viewport dimension).
