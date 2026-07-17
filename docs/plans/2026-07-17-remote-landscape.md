# Landscape layout for /remote + stacked navigation buttons (Issue #303)

Design finalized in Claude Design ("Remote landscape", frames 9b + 9c,
author-approved 2026-07-17 after three chat iterations). The author picked the
**edge-rail** landscape split (9b) over the issue-sketch bottom-pills variant
(9a), and — replacing an intermediate left-hand-mode proposal — settled on
**stacked navigation buttons everywhere**: Next is always the bottom-most
control, full width, accent; Previous sits above it as a slimmer ghost pill.
Bottom-center/bottom-corner placement is equally reachable by either thumb, so
the design is handedness-neutral by construction and needs **no left-hand
toggle, no per-device setting** (a `data-peitho-hand` mirror was explicitly
rejected: "Next on the left is not intuitive").

This deliberately amends one Issue #303 non-goal: the portrait button row
changes from a `1fr 1fr` side-by-side grid to the stacked pair, for everyone
(author decision, 2026-07-17). Everything above the buttons in portrait is
untouched; the notes panel gives up ~50px of height.

## Final visual spec (from the approved mock)

### Both orientations

- **Actions**: vertical stack, `gap: 10px`. Previous: ghost pill
  (`border: 1px solid #2f3644`, transparent, `#dde3ec`), slimmer
  (`min-height: 48px` portrait / `44px` landscape-column contexts is NOT
  needed — see rail below; portrait uses 48px), `font-size: 15px`.
  Next: accent pill (`rgba(56,189,248,0.14)` / `#38bdf8`), `min-height: 60px`,
  `font-size: 17px`. DOM order (prev, next) already matches the stack.
- Button labels move from bare text nodes into
  `<span class="peitho-remote-action-label">` so the rail can restyle/stack
  them. Text stays "Previous" / "Next" in both orientations ("Previous" at
  13px ≈ 60px fits the 96px rail; no dual short/long label spans).

### Landscape (`@media (orientation: landscape) and (max-height: 520px)`)

Breakpoint rationale: `orientation: landscape` alone would also catch tall
desktop windows; the `max-height` guard targets phones. Short-but-narrow
(portrait) viewports keep today's compression fallback unchanged — that
behavior is load-bearing (7d10b2a: pace row and buttons always visible).

Three-column grid on `.peitho-remote` (the DOM stays flat; children are
placed with `grid-area`, so the dynamically inserted section line and the
optional elements need no wrapper divs):

```
grid-template-columns: minmax(0, 1.55fr) minmax(0, 1fr) 96px;
grid-template-rows: minmax(0, 1fr) auto auto auto;
gap: 10px 14px;  max-width: none;
```

| area     | grid placement      | notes                                        |
|----------|---------------------|----------------------------------------------|
| preview  | row 1, col 1        | `max-height: 100%; min-height: 0` — grid stretch supplies the width; keeps `aspect-ratio`; on very short viewports the shell letterboxes inside |
| titlebar | row 2, col 1        |                                              |
| chase    | row 3, col 1        |                                              |
| pace     | row 4, col 1        | timer + reset stay here (author-approved); `flex-wrap: wrap` so a long pace delta wraps inside the column instead of overflowing into the notes column on narrow landscape viewports |
| notes    | rows 1–2, col 2     | the flexible area                            |
| status   | row 3, col 2        | usually empty; shows `Ended`                 |
| section  | row 4, col 2        | bottom-aligned beside the pace row; absent ⇒ empty auto row, no hole |
| actions  | rows 1–4, col 3     | becomes the rail                             |

Rail styling: `.peitho-remote-actions` switches to a column flex; buttons get
`border-radius: 20px`, `flex: 1` (Previous) / `flex: 1.35` (Next),
`min-height: 0`, column content (arrow above label via `order: -1` on the
arrow), arrow `26px`, label `13px/600`. Next occupies the bottom-right
corner — the strongest blind-tap thumb zone in a two-handed landscape grip
(this was the deciding argument for 9b).

Root padding in landscape: `12px 14px` plus `env(safe-area-inset-left/right)`
(the page already sets `viewport-fit=cover`, so notch-side content would be
clipped without it) **and** `padding-bottom: calc(12px +
env(safe-area-inset-bottom, 0px))` — the `padding: 12px 14px` shorthand resets
the portrait bottom-inset longhand inside the media query, and the home
indicator still occupies the bottom edge in landscape, so the bottom inset
must be re-added or the rail's Next button overlaps the swipe-gesture zone.

Known risk, deliberately deferred to E2E: a palm resting on the right screen
edge could ghost-tap the rail. If real-device use shows accidental taps, the
fallbacks are (in order) a few px of dead margin on the rail, then the 9a
bottom-pills variant.

## Implementation tasks (TDD, in order)

Each task: failing test first, then implementation, then the full gate list
from CLAUDE.md.

### 1. remote.ts: action label spans

- `remoteButton()` wraps the label in `span.peitho-remote-action-label`
  (arrow span stays). jsdom tests: both buttons carry the label span with the
  right text; arrow/label order per direction unchanged in DOM (prev: arrow
  then label; next: label then arrow); click dispatch and disabled logic
  untouched (existing tests keep passing).

### 2. render.rs: portrait stacked actions + landscape grid

- `.peitho-remote-actions` portrait: `display: flex; flex-direction: column;
  gap: 10px;`; per-direction min-heights/fonts per spec above.
- New landscape media query block per the table above, all
  `peitho-remote-*` selectors.
- Extend `remote_index_mounts_remote_bundle_with_feature_detection_and_canvas_tokens`
  (string-containment style, as the file already does): stacked actions rules,
  the exact media-query prelude
  `@media (orientation: landscape) and (max-height: 520px)`, the
  grid-template lines, one `grid-area` per placed element, rail flex rules,
  safe-area padding line.

### 3. Embedded bundle + gates

- `npm run build` → commit `dist/remote.js` (+ others only if they drift);
  full gate list including 3× `cargo test --workspace`.

### 4. E2E (manual, before PR)

- `peitho present --host --port <fixed>` on an example deck with sections +
  notes; Chrome windows at 844×390 (landscape) and 390×844 (portrait), plus
  667×375 (SE-class) for the shrink path. Verify: rail renders with Next in
  the corner, notes/section/status placement, preview aspect fit
  (letterboxing acceptable, no overflow), portrait stacked buttons, ended
  state dims and disables in both orientations. Screenshot evidence.

## Review-driven amendments (2026-07-17, applied before PR)

- **Safe-area bottom inset re-added in landscape** (see the root-padding
  paragraph above) — caught in the first review pass.
- **Dead CSS removed**: the landscape preview rule dropped
  `width: 100%; margin-inline: auto` (grid stretch already fills the track,
  which made the auto margins dead); the rail button rules were consolidated
  so `border-radius: 20px` and `flex-direction: column` live once on
  `.peitho-remote-actions button` and the per-direction rules carry only
  `flex` and `min-height: 0`.
- **`flex-wrap: wrap` on the landscape pace row**: column 1 is ~313px on an
  SE-class 667×375 viewport; with the timer running, the nowrap pace delta
  could otherwise overflow the `minmax(0, 1.55fr)` track into the notes
  column. Wrapping the delta onto a second line inside the column is the
  intended degradation.
- **`aria-hidden="true"` on the arrow spans**: the old DOM used whitespace
  text nodes between arrow and label; spacing is now CSS gap, so without the
  attribute assistive technology would announce the fused run ("‹Previous").
  Hiding the decorative glyphs makes the accessible names plain
  "Previous"/"Next" — better than the old baseline.
- **`align-self: end` on the landscape section line and chase**: default grid
  stretch top-aligned the 12.5px section text in the ~44px pace row band
  (the mock bottom-aligns it), and for decks without `time` the 6px
  slide-mode chase floated mid-row because the status min-height sets row 3's
  height. Pinning both to the row bottom matches the mock; for timed decks
  the chase rule is a no-op (34px chase already fills the row).
- **Preview aspect behavior, measured (2026-07-17)**: in real Chrome the
  landscape preview keeps its aspect ratio under default grid stretch
  (315×175 ≈ 16:9 measured at 667×375) — `max-height: 100%` only engages on
  wide-short viewports, where the shell letterboxes inside. On
  smaller-than-target landscape viewports the ratio-derived pane is shorter
  than the 1fr row, leaving slack between preview and titlebar (~55px at
  667×375, ~7px at the 844×390 design target) — accepted; nothing clips or
  overlaps.

## Real-device amendments (2026-07-17, iPhone Safari screenshots from the author)

- **`100dvh` cascade** (`height: 100vh; height: 100svh; height: 100dvh;` on
  `body` and `#peitho-remote-root`): iOS Safari in landscape with the tab bar
  visible reports `svh` ~45px smaller than the actual visible area (measured
  on the author's phone — portrait reports correctly), which left a dead band
  below the grid. `dvh` tracks the current chrome state, and also expands the
  layout when the user hides Safari's toolbar. `svh` stays in the cascade as
  the no-`dvh` fallback.
- **Landscape column 1 tracks the preview's real width**: on short
  real-device viewports the aspect-fit preview used only ~154px of its fixed
  `1.55fr` (~290px) column, wasting ~130px that belongs to the notes column.
  The column is now
  `min(calc((100dvh - 151px - env(safe-area-inset-bottom, 0px)) * <aspect>), 52%)`
  (progressive second declaration; the `1.55fr` line remains as the no-`dvh`
  fallback). 151px = the column's fixed vertical overhead (padding-top 12 +
  titlebar ~19 + chase 34 + pace 44 + three 10px row gaps + padding-bottom
  base 12); `<aspect>` is the canvas token, so the calc reads
  `* 16 / 9` etc. The 52% cap equals the old 1.55fr share at the 844×390
  design size, so the approved-mock rendering is unchanged there.
- **Portrait stays untouched** (author decision): the portrait cramped-notes
  feedback is chrome tax (~130px) + preview + buttons; revisit together with
  the standalone/add-to-home-screen follow-up (Issue #306) rather than
  shrinking the preview now.

## Non-goals

- Tablet-specific layouts.
- Any handedness setting (rejected in design).
- Markdown notes, agenda list on the phone (unchanged from #299).
