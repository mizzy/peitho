+++
title = "Aspect Ratio 4:3"
weight = 20
template = "example-page.html"
description = "A short deck that switches Peitho from widescreen defaults to a 4:3 logical canvas."

[extra]
demo_path = "/demo/aspect-ratio-4-3/"
source_path = "static/deck-sources/aspect-ratio-4-3/deck.md"
+++

## What it demonstrates

Aspect Ratio 4:3 changes one deck-level setting: `aspect_ratio: 4:3`. The deck
then renders on a 960 by 720 logical canvas instead of the default widescreen
shape.

Use this example when you need older projector geometry, embedded documentation
captures, or a square-ish canvas without changing the authoring model.

## What to look at

The source has no custom layout or CSS. The frontmatter alone changes the
canvas behavior, which is the same `aspect_ratio` key documented in
[Frontmatter](@/guide/frontmatter.md).
