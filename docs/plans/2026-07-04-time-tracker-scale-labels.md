# Time tracker scale labels — fix last-label alignment (Issue #97)

## Current state

The presenter time tracker's scale (0:00 / 1:15 / 2:30 / 3:45 / 5:00) is placed at the left edges of the 5 cells of `display: grid; grid-template-columns: repeat(5, 1fr)`. As a result, the last label "5:00" lands at the start of the last cell = 4/5 (80%) of the bar's full width and does not line up with the right edge of the bar (= the planned-time position).

## Design approach (author-approved)

Each label points at a **moment** on the bar at 0% / 25% / 50% / 75% / 100%.

- **Placement**: position each label with `position: absolute` + `left: N%`
- **Alignment**:
  - First (0%): left-aligned (`translateX(0%)`)
  - Middle (25%, 50%, 75%): center-aligned (`translateX(-50%)`)
  - Last (100%): right-aligned (`translateX(-100%)`)

This follows the same convention as the existing rabbit/turtle markers (inline style setting `left` + `transform: translateX(...)`), so CSS only carries a generic `.tracker-scale span { position: absolute; }`, and alignment is applied by inline styles set from timeTracker.ts.

### Why inline style

The presenter CSS test in `crates/peitho-core/src/render.rs` asserts `assert!(!html.contains("transform: translateX(-50%)"))`, so translateX cannot be declared in CSS. Applying it inline from TS — the same way the existing rabbit/turtle markers do it — is consistent and satisfies this constraint.

## Files changed

### `packages/peitho-present/src/timeTracker.ts`

1. Extend `timeScaleLabels` to return placement info alongside text (or derive from index at the call site)
2. In the presenter-variant `<div class="tracker-scale mono">…</div>` construction, add `style="left: X%; transform: translateX(Y%)"` on each of the 5 spans
   - index 0: `left: 0%; transform: translateX(0%)`
   - index 1: `left: 25%; transform: translateX(-50%)`
   - index 2: `left: 50%; transform: translateX(-50%)`
   - index 3: `left: 75%; transform: translateX(-50%)`
   - index 4: `left: 100%; transform: translateX(-100%)`

### `crates/peitho-core/src/render.rs`

Rewrite the CSS as:

```css
.tracker-scale { position: relative; height: 12px; margin-top: 6px; color: var(--fg-dim); font-size: 10px; letter-spacing: 0.08em; }
.tracker-scale span { position: absolute; top: 0; white-space: nowrap; }
```

Remove:
- `display: grid; grid-template-columns: repeat(5, 1fr)`
- The cell borders `border-left: 1px solid var(--line-soft); padding-left: 6px;`
- `:first-child { border-left: none; padding-left: 0; }`

Reason: the cell concept itself is gone in the new layout, so cell borders are unnecessary. The Issue calls for "first left-aligned, middle center-aligned, last right-aligned" and does not include the cell-border look as a preservation requirement.

### `crates/peitho-core/src/render.rs` (test side)

Add new assertions:

```rust
assert!(html.contains(".tracker-scale { position: relative;"));
assert!(html.contains(".tracker-scale span { position: absolute;"));
assert!(!html.contains(".tracker-scale { display: grid;"));
```

Keep `assert!(!html.contains("transform: translateX(-50%)"))` as is (it never appears in CSS; it is only emitted as an inline style).

## Tests

### `packages/peitho-present/test/timeTracker.test.ts`

Append to the existing "renders presenter variant with legend fill track and five-point time scale" test:

```ts
const scaleSpans = Array.from(tracker.querySelectorAll<HTMLElement>(".tracker-scale span"));
expect(scaleSpans.map((s) => s.style.left)).toEqual(["0%", "25%", "50%", "75%", "100%"]);
expect(scaleSpans.map((s) => s.style.transform)).toEqual([
  "translateX(0%)",
  "translateX(-50%)",
  "translateX(-50%)",
  "translateX(-50%)",
  "translateX(-100%)"
]);
```

This pins "last right-aligned," "first left-aligned," and "middle center-aligned" at the DOM level.

The existing "keeps the present variant DOM unchanged" test is untouched (the present variant is not in scope; its DOM stays as is).

## Verify

- `cargo test --workspace` x3
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` (no change to the ts-rs contract)
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js` (after rebuild)

## E2E (author verifies on device)

Open the presenter with `peitho present examples/deck-with-time.md --port 8080` and visually confirm "5:00" on the scale is flush with the right edge of the bar.
