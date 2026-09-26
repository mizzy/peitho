# Inline edit overlay outline

## Goal

Keep the rendered editor's layout box unchanged when preview inline editing starts, while preserving one visible edit frame for wrapped content.

## Design

- Append one pointer-inert, absolutely positioned edit-frame element to the active stage tile, outside the slide shadow root.
- Measure the editor's union bounding rectangle, expand it by a 6px screen-space gap, and translate it into tile-relative coordinates.
- Position synchronously when editing starts, then use one animation-frame loop to track every source of editor movement; write only changed frame geometry.
- Why a frame loop, not the issue's `ResizeObserver` proposal (measured in Chrome): `ResizeObserver` reports a non-replaced inline element (the tight-list-item span, an inline-slot heading span) as 0×0, so typing never fired it. The editor also moves without resizing, for example through overflow scrolling, web-font loads, layout scripts and stage scaling. Enumerating triggers kept missing cases. A per-frame measurement covers every cause through one path.
- Known limits: the frame lives outside the slide's clipping. It outlines the part of an editor that is scrolled out of an overflow container. It can also be clipped by the stage tile when an editor sits flush against the stage edge. An emptied inline editor measures 0×0, so the frame shrinks to a gap-sized box.
- Cancel the pending animation frame and remove the overlay through the inline edit's `dispose` closure; failed-save unlock leaves both active.
- Apply only `outline: none` to the editor to suppress Chrome's native focus ring without affecting layout. Restore the original style on close, and reapply the suppression after a failed-save unlock.

## Tasks

1. Cover outline suppression, unchanged layout styles, animation-frame tracking, overlay geometry, and lifecycle behavior with failing tests.
2. Add frame creation, synchronous positioning, continuous geometry tracking, conditional style writes, and centralized disposal to the preview shell.
3. Regenerate the committed preview bundle.

## Verification

- Run the focused Vitest coverage red, then green.
- In `packages/peitho-present`, run `npm run build && npm test && npm run typecheck`.
- Confirm only the intended source, tests, plan, and generated `dist/preview.js` changed.
