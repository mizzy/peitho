# /remote layout tuning against the chrome-free standalone viewport (Issue #309)

Follow-up to #303 (landscape), #305 (implementation), #306/#308 (standalone). All
three items the author flagged on 2026-07-17 after using the merged standalone
mode on a real device get resolved by the *same* two moves — remove the always-
reserved Status row, and use the existing `dim-on-end` state stronger — so this
PR is one small CSS change, not three per-item ones.

## Author's items, and how they collapse into one

1. **Portrait: bottom clearance reads as dead space** — `padding-bottom: 18px +
   env(safe-area-inset-bottom)` was sized for a browser toolbar; in standalone
   the home indicator needs less visual clearance.
2. **Portrait: blank band between notes and Previous** — the always-reserved
   Status row (`min-height: 1.4em`, empty except when Ended) plus its two 12px
   gaps costs ~42px that notes could use.
3. **Landscape: blank space below notes** — Status row 3 (usually empty) and
   Section row 4 sit under the notes panel unused.

The Ended state is what the Status row exists for, and Ended is fully read-only
— the whole surface is already `dim-on-end` and every button already goes
`disabled` when `this.ended` is true (`renderTimeDependentChrome` /
`renderButtons` in `remote.ts`). So the Ended-specific text row is redundant:
"the surface is dim and every control is disabled" already says "the deck has
ended". Removing the Status row entirely handles Item 2 (portrait) and, in
landscape, freeing up row 3 lets notes grow.

Item 3's second half — the Section line at row 4 — is not Ended-specific but
still costs a ~44px band that adds no information notes doesn't already carry
(the section name and offset in the section). The section context is a nice-to-
have that the presenter view surfaces properly; on the phone remote where every
pixel is being fought over, cutting it lets notes fill the full right column.

Item 1 is a one-line clearance change.

**Author confirmed 2026-07-18 via the Claude Design mock**: no "Ended" overlay,
no chip, no separate label. Bare grey-out + disabled buttons is enough.

## Final visual spec

### Portrait (approved mock — v4 "final")

- `#peitho-remote-root` `padding-bottom: calc(8px + env(safe-area-inset-bottom, 0px))`
  (was `18px + env(...)`; ~10px reclaimed to notes).
- **`.peitho-remote-status` element and its inline text state are removed
  entirely.** The `min-height: 1.4em` selector goes away, and `renderRemoteControls`
  no longer creates the DOM element; `render.setText(this.root, "status", ...)`
  is deleted from `remote.ts`. This is what makes the Ended state a "dim-only"
  state — there is no text row to reveal.
- Notes gets back the ~42px row + ~10px bottom padding.

### Landscape (`@media (orientation: landscape) and (max-height: 520px)`)

- `.peitho-remote-actions` grid-area unchanged (rows 1–4 col 3, rail).
- `.peitho-remote-notes` **grid-area: 1/2/5/3** (was `1/2/3/3`) — fills the full
  right column height.
- Status row and Section line are absent from the grid: neither the CSS grid-
  area rules for `.peitho-remote-status` and `.peitho-remote-section` nor the
  landscape `align-self: end` rule for the section survive. The DOM insertion
  path for the dynamically-created `.peitho-remote-section` line
  (`renderSection`) is deleted along with it — a rendered element with no grid
  placement rule would fall to the auto flow track and either overlap the notes
  panel or push the grid taller than the viewport.
- Landscape column-1 tracks unchanged (the second-declaration `min(calc(...), 52%)`
  formula from #303 stays).

### Ended state

- `.peitho-remote[data-peitho-ended="true"] .peitho-remote-dim-on-end` opacity
  goes **0.35 → 0.28** (the author picked heavier dim in the mock). The
  `dim-on-end` targeting rules are unchanged.
- The DOM-level `disabled` behavior in `remote.ts`'s `renderButtons` and
  `renderTimeDependentChrome` is unchanged — timer/reset/prev/next already go
  `disabled` when `this.ended` is true.
- No overlay element, no chip, no label. Ended and non-Ended layouts are
  *byte-identical structurally*; only opacity + button `disabled` change.

## What that means for the code

- **`crates/peitho-core/src/render.rs` `render_remote_index()`**:
  - `#peitho-remote-root` base padding-bottom `18px` → `8px`.
  - Landscape media block's `padding-bottom: calc(12px + ...)` stays as is (the
    landscape number wasn't part of the complaint and already accounts for the
    swipe zone).
  - Delete the `.peitho-remote-status` selector block entirely.
  - Delete the landscape `.peitho-remote-status` and
    `.peitho-remote-section` grid-area rules (both the base
    `.peitho-remote-section` and the `align-self: end` variant).
  - Landscape `.peitho-remote-notes` `grid-area: 1/2/3/3` → `1/2/5/3`.
  - `[data-peitho-ended="true"] .peitho-remote-dim-on-end` opacity `0.35`
    → `0.28`.
  - Update the `remote_index_mounts_remote_bundle_with_feature_detection_and_canvas_tokens`
    string-containment test to match: the four rewrites above, plus new
    negative assertions that `.peitho-remote-status {` and
    `.peitho-remote-section` are no longer in the emitted CSS.

- **`packages/peitho-present/src/remote.ts`**:
  - `installRemoteControls`: remove the `status` element creation, its
    `dataset.peithoRemote = "status"`, and its append to `container`.
  - `RemoteController`:
    - Delete `renderSection` (the whole method + its call site in `render`).
    - Remove `setText(this.root, "status", ...)` from `render`.
  - Existing unit tests that assert `[data-peitho-remote="status"]` receives
    "Ended" text on `setEnded()` need to be replaced with tests that assert
    the frame's `data-peitho-ended` attribute is `"true"` and Prev/Next/Timer/
    Reset buttons are `disabled` (which is already the observable behavior;
    the Status text assertion was redundant).
  - Existing section-line tests need to be removed (there is no
    `.peitho-remote-section` element in the DOM anymore).

- **`crates/peitho-core/src/render.rs` embedded bundle drift**: after
  `npm run build`, `dist/remote.js` will regenerate. Commit it. `shell.js` and
  `preview.js` should stay byte-identical (no changes touch either).

## Implementation tasks (TDD, in order)

Each: failing test first, then implementation, then the full gate list.

### 1. render.rs CSS: padding-bottom + Status/Section removal + dim strength

- Update the string-containment test with the four selector rewrites plus two
  negative assertions.
- Apply the CSS changes.

### 2. remote.ts: drop Status element + Section rendering

- Delete jsdom tests that assert the Status text or Section line DOM presence.
- Add (or update) tests asserting Ended state: `data-peitho-ended="true"`,
  all four buttons `disabled`, no Status/Section DOM.
- Apply the code deletion.

### 3. Embedded bundle + gates

- `npm run build` → commit `dist/remote.js`.
- Full gate list: 3× `cargo test --workspace`, clippy, fmt, bindings drift,
  npm test + typecheck, shell/preview/remote dist drift check.

### 4. E2E (manual, on the author's phone)

- `peitho present --host` on a deck with a `time:` frontmatter and sections;
  scan QR, open in Safari, Add to Home Screen. Launch:
  - Portrait: notes taller than before by ~50px; no gap between notes and
    Previous; no gap between Next and the home indicator — including on a
    cold start without rotating the phone first (the dvh pitfall below).
  - Landscape: notes extends down through the old status band (rows 1–3);
    the Section line stays at the bottom of the column (row 4).
  - End the deck (kill `peitho present`): whole remote surface dims to a
    heavier grey, every button is unpressable. No "Ended" text anywhere.
- Screenshot evidence in the PR.

## Non-goals

- Any tablet-specific layout.
- Any changes to how the presenter view (not /remote) handles Ended.
- Portrait `.peitho-remote-chase` height, pace row layout, or button sizes —
  none of these were in the author's item list and touching them would be
  scope creep.

## Real-device amendments (2026-07-18, author's phone)

- **Landscape Section line restored.** The first implementation deleted the
  Section line along with the Status row, reading the issue's "if the
  status/section placement is rethought" as license to drop it. The author
  corrected this: removal was never requested. The Section line is back at
  landscape row 4 (bottom-aligned, built via `createDimmableRow` so it keeps
  `dim-on-end`), and notes spans rows 1–3 (`grid-area: 1/2/4/3`) — gaining
  exactly the old status band, not the section band. Portrait is unaffected
  (the section line only renders in the landscape grid).
- **`100dvh` is stale on iOS standalone cold start** (measured on the
  author's device: portrait launch leaves a dead band at the bottom, and one
  portrait→landscape→portrait rotation clears it — matching the documented
  WebKit behavior that dynamic-viewport units are only initialized once the
  viewport is "exercised" by a geometry change). Reading
  `visualViewport.height` / `window.innerHeight` from JS at startup returns
  the same stale value, so a first-attempt JS tracker that mirrored
  `visualViewport.height` into a CSS variable did not help and was removed —
  do not retry that approach. The fix is CSS-only: standalone mode has no UA
  chrome, so `100vh` is correct from launch and always equal to the real
  viewport there. `@media (display-mode: standalone)` overrides `body` /
  `#peitho-remote-root` heights to `100vh`, plus a companion override of the
  landscape preview-column formula (`100vh` in place of `100dvh`). In-browser
  pages keep the `vh → svh → dvh` cascade — the `dvh` remains load-bearing
  for Safari's landscape tab-bar under-report
  (docs/plans/2026-07-17-remote-landscape.md).

## Design record (mock iteration)

- Claude Design mock:
  https://claude.ai/design/p/4bfae954-d0e7-41f8-8c4d-a50e231c1802?file=Remote+Tuning.dc.html
- Iteration path: three-column proposal (A/B/three-way) → Ended-state
  disambiguation with all controls `disabled` → three overlay variants
  (X/Y/Z: preview overlay, full-surface overlay, preview-panel-replace) →
  author picked "just grey out". "V4 final" frame set (Portrait Normal,
  Portrait Ended, Landscape Normal, Landscape Ended) is the approved spec.
