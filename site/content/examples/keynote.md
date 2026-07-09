+++
title = "Keynote"
weight = 40
template = "example-page.html"
description = "A centered serif keynote deck where slide shape dispatches title-only covers and body statement slides."

[extra]
deck = "keynote"
demo_path = "/demo/keynote/"
source_path = "static/deck-sources/keynote/deck.md"
github_path = "examples/keynote"
+++

## What it demonstrates

The Keynote deck uses two layouts with no explicit layout comments. A title-only
slide matches the cover layout, while slides with a heading and body copy match
the statement layout.

That makes it a compact example of hybrid dispatch by content shape. The
Markdown stays close to the talk outline, and the layout files decide how title
and body slots render; the mechanism is covered in [Layouts](@/guide/layouts.md).

## What to look at

The custom CSS gives the deck a centered, serif keynote style with larger cover
type and measured statement copy. Line-breaking rules in the CSS keep the text
from breaking awkwardly at narrow widths, while the Markdown itself stays
plain.
