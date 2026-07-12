+++
title = "Layout Pin"
weight = 55
template = "example-page.html"
description = "Two layouts with the same slot contract, so every slide must pick one with an explicit layout pin."

[extra]
deck = "layout-pin"
demo_path = "/demo/layout-pin/"
source_path = "static/deck-sources/layout-pin/deck.md"
github_path = "examples/layout-pin"
+++

## What it demonstrates

Layout dispatch is hybrid: an explicit `{"layout":"name"}` pin wins, a single
layout dispatches unconditionally, and otherwise the slide's structure must
match exactly one layout. This deck removes the third path on purpose — its two
layouts (`statement`, light and left-aligned; `spotlight`, dark and centered)
declare exactly the same slot contract, so structural dispatch can never decide
and every slide carries a pin.

Ambiguous or zero matches are never silently resolved. The last two slides
quote the measured errors: an unpinned slide fails with
`slide matches multiple layouts: spotlight, statement`, and a misspelled pin
fails with `unknown layout 'sptolight'` plus the list of candidates.

## What to look at

The cover and closing slides have the same Markdown shape as the middle two —
title plus body (plus an optional code block) — yet render dark-centered or
light-left purely by pin. This is the layout-dispatch counterpart of
[Two Column](@/examples/two-column.md)'s explicit slots: when convention cannot
decide, the author declares, and everything else is a line-numbered build
error.
