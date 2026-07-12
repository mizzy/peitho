# Layout pin example (Issue #277)

Date: 2026-07-13

## Goal

Give explicit layout dispatch (`{"layout":"name"}`) a dedicated example. The pin
appears exactly once across all examples (one slide of peitho-tour, mixed with
section/time settings), so the story — same structural shape in two layouts makes an
unpinned slide a build error, and the pin is the resolution — is never shown end to
end. This is the layout-dispatch counterpart of two-column's explicit-slot example.

## Shape

```
examples/layout-pin/
  deck.md                  # four slides, every one pinned (see below)
  layouts/
    statement.html         # light, left-aligned editorial
    spotlight.html         # dark, centered display
  css/
    theme.css
```

The two layouts deliberately declare the **same slot contract** (`title` 1,
`body` 1..*, `code` 0..1), so no slide in the deck can be structurally dispatched:
every slide must carry a pin, which is the point. The deck building at all proves the
pins resolve; the error slides quote the measured texts:

- unpinned: `slide matches multiple layouts: spotlight, statement` with
  `help: pick one explicitly with <!-- {"layout":"…"} -->`
- typo: `unknown layout 'sptolight'` with `help: use one of: spotlight, statement`

## Content

Cover pinned to `spotlight` (dark tile for the gallery), the three dispatch rules on
`statement`, the ambiguity error on `statement` (code block), the unknown-name error
back on `spotlight`.

## Wiring

- `Makefile`: add `layout-pin` to `DEMO_DECKS`
- `site/content/examples/layout-pin.md`: weight 55 (after Two Column, its
  explicit-slot sibling). The landing gallery derives automatically (#274).
