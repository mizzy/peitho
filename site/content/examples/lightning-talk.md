+++
title = "Lightning Talk"
weight = 10
template = "example-page.html"
description = "A five-minute poster-style talk that uses sections, notes, and a title/body layout with no code slot."

[extra]
demo_path = "/demo/lightning-talk/"
source_path = "static/deck-sources/lightning-talk/deck.md"
+++

## What it demonstrates

Lightning Talk is a timed five-minute deck with section markers for setup,
problem, approach, and wrap-up. It also includes speaker notes on the opening
slide, so it is useful for checking the presenter workflow.

The custom poster layout accepts only title and body content. There is no code
slot, which keeps the example focused on talk timing, section metadata, and
large-type Markdown slides.

## What to look at

Compare the `time: 5m` frontmatter with the per-section page settings comments.
Those values drive the agenda and timer behavior described in
[Writing Decks](@/guide/writing-decks.md) and [Frontmatter](@/guide/frontmatter.md).
