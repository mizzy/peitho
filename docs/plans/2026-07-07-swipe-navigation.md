# Swipe navigation for touch devices (Issue #161)

## Goal

Let iPhone / iPad users advance and rewind slides by swiping left and
right on the slide canvas. The keyboard path (already Mac-only) and
the canvas-click path (fine for mouse but poor for fingers) both stay
unchanged.

Concretely: a horizontal swipe on the slide canvas emits
`peitho:navigate` with `to: 'next'` (left swipe) or `to: 'prev'` (right
swipe), matching §16 of the kickoff spec.

## The three pillars

- **Long-term view** — every existing "how do I advance a slide" input
  path (keyboard, click, presenter buttons, sync) already dispatches
  `peitho:navigate` through the shell. Adding swipe as a new sibling
  installer next to `installCanvasClickNavigation` in
  `packages/peitho-present/src/controls.ts` keeps the pattern uniform,
  so a future touch-input variant (pinch-to-zoom, keyboard-on-iPad,
  etc.) plugs in the same way.
- **Type safety** — the swipe installer dispatches the existing
  `NavigateDetail` union (`{ to: "prev" | "next" }`); no new detail
  shape, no new event type. `installSwipeNavigation` returns the same
  `() => void` cleanup type as its siblings — a caller cannot forget
  to detach listeners without leaking, because the type signature
  forces the pattern.
- **Root cause** — this isn't a bug fix; the root is that peitho has
  no touch input path. Adding one at the same seam that already owns
  input-to-navigate translation is the correct depth. Rejected
  alternatives: (a) a `touchend` handler jammed into
  `installCanvasClickNavigation` — mixes two independent input models
  in one installer, breaks single-responsibility; (b) a document-level
  swipe listener bypassing the shell — violates §16.

All three lenses uniquely select the design below.

## Design

### `packages/peitho-present/src/controls.ts` — new installer

```ts
export type SwipeNavigationOptions = {
  root: HTMLElement;
  window?: Window;
  bus?: EventTarget;
  minHorizontalPx?: number;   // default 50
  maxDurationMs?: number;     // default 800
  minRatio?: number;          // default 1.5 (|dx| / |dy|)
};

export function installSwipeNavigation(options: SwipeNavigationOptions): () => void {
  // touchstart → record (x, y, t) of primary touch
  // touchend → compute dx, dy, dt; if within thresholds, dispatch navigate
  // ignore multi-touch (event.touches.length > 1 at start → skip)
  // ignore inside control bar (same guard as installCanvasClickNavigation)
}
```

Decisions:

- **Left swipe (`dx < 0`) → `next`**, right swipe (`dx > 0`) → `prev`.
  Matches natural "flick the current page off to the left to reveal
  the next" reading behavior; also matches the click-navigation
  convention (right side of the canvas → next).
- **Threshold defaults**: 50 CSS px horizontal, |dx|/|dy| > 1.5,
  duration < 800 ms. The threshold is in CSS pixels (dp), not device
  pixels, so a swipe feels the same on every DPR.
- **Primary-touch only**: if `event.touches.length !== 1` at
  `touchstart`, drop the gesture. Multi-touch scroll / pinch is not
  our concern.
- **Passive `touchstart`, non-passive `touchend`** — `touchend` needs
  `preventDefault()` when we actually consume the gesture, so it
  cannot be passive. `touchstart` never prevents default (we don't
  block scrolling) so it can stay passive for perf.
- **`{ passive: true }` on touchstart** is the compatibility default
  browsers want; violating it triggers Chrome console warnings.
- **Control-bar exclusion**: mirror
  `installCanvasClickNavigation`'s `closest('[data-peitho-control-bar="true"]')`
  guard on the `touchstart` target — a swipe that starts on the
  control bar (e.g. thumbing along the timer) should not paginate.
- **Click-suppress preventDefault (added during review)**: when
  `|dx| >= minHorizontalPx / 2` (25 px by default) `touchend` calls
  `event.preventDefault()` even when the swipe fails the other
  thresholds. Without this, iOS synthesizes a `click` after a "clearly
  intended but malformed" horizontal drag, and
  `installCanvasClickNavigation` (or the inline distribution click
  handler) then navigates based on `clientX` — so a canceled
  diagonal drag would still advance the slide.
- **Mid-swipe multi-touch guard (added during review)**: if a
  second `touchstart` arrives while `active === true`, ignore it
  instead of resetting. The in-flight primary-finger gesture keeps
  going; the resting thumb doesn't cancel navigation. The primary
  finger's own `touchend`/`touchcancel` still resolves the gesture.
- **`touch-action: pan-y` on `html`, `body`,
  `#peitho-slides` / `#peitho-present-root` (added during review)**:
  reserves horizontal touch gestures for peitho and prevents iOS
  Safari's edge-swipe back-navigation from firing on letterbox black
  bars (portrait phone + 16:9 deck) while our document-level
  listener also processes the same gesture, which otherwise causes
  double navigation (browser back + peitho next).

### `packages/peitho-present/src/index.ts` — export

Add `installSwipeNavigation` and its `SwipeNavigationOptions` type.

### `crates/peitho-core/src/render.rs` — two callers

- **`render_present_index`** (`present.html`): after
  `peitho.installCanvasClickNavigation({ root, window });` add
  `peitho.installSwipeNavigation({ root, window });`
- **`render_distribution_index`** (`index.html`): this template has
  no access to the shell bundle — it inlines its keyboard and click
  handlers. Add an inline touchstart/touchend handler that mirrors
  the shell installer's threshold logic and calls the local
  `navigate('next' | 'prev')` function. Duplication is unavoidable
  because the file is a self-contained fallback bundle; the same is
  already true for the keyboard and click handlers on lines 356–374.

Do NOT wire this into `render_presenter_index` — the issue explicitly
excludes the presenter.

### Test scope

Vitest, DOM-based, mocking `TouchEvent` / `Touch` construction. Cases:

1. Left swipe (`dx < -50`, |dx|/|dy| big enough, dt < 800) → dispatches
   `{ to: "next" }`.
2. Right swipe → `{ to: "prev" }`.
3. Too-short swipe (`|dx| < 50`) → no dispatch.
4. Too-slow swipe (`dt > 800`) → no dispatch.
5. Diagonal-mostly-vertical swipe (`|dx|/|dy| < 1.5`) → no dispatch
   (so vertical scrolling on a partial page still works).
6. Multi-touch (`touches.length > 1` at start) → no dispatch.
7. Swipe that starts on the control bar → no dispatch.
8. `cleanup()` removes listeners.

Additionally: a `render.rs` test asserting that both
`render_present_index` and `render_distribution_index` contain the
new touch-handler markers (mirrors existing `installCanvasClickNavigation`
substring assertion).

## What does NOT change

- The keyboard path (`installKeyboardNavigation`).
- The mouse-click path (`installCanvasClickNavigation` for present,
  inline click for distribution index) — those still fire on tap,
  because a tap under threshold has `dx < 50` and is filtered by our
  swipe logic, so the two input models don't collide.
- The presenter view (`presenter.html`) — out of scope.
- Vertical scroll semantics — we do NOT `preventDefault` on
  `touchmove`; the user can still scroll a page whose swipe fails the
  ratio check.

## Gates

Standard peitho gates. No new Rust logic, but `render.rs` templates
change so:

- `cargo test --workspace` (test the new `render_present_index` /
  `render_distribution_index` substring assertions)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/`
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js`
  (rebuild embeds the new installer)

## Manual smoke (post-merge)

Serve a built deck locally, open on a real iPhone via
`http://<mac-ip>:port/`, swipe left and right, confirm slides advance
and rewind. Verify vertical scroll still works when the swipe fails
the ratio.
