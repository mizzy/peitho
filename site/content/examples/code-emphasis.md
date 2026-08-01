+++
title = "Code Line Emphasis"
weight = 71
template = "example-page.html"
description = "A zero-config deck that emphasizes specific code lines, stepping the emphasis through a function during a talk."

[extra]
deck = "code-emphasis"
demo_path = "/demo/code-emphasis/"
source_path = "static/deck-sources/code-emphasis/deck.md"
github_path = "examples/code-emphasis"
demo_label = "Open published deck (static emphasis only)"
+++

## What it demonstrates

Code Line Emphasis shows how a brace group in a fence info string marks the
lines you are talking about. This is a separate layer from syntax highlighting:
highlighting colors code by what it *is*, emphasis marks where you are in the
talk.

The `|` separator is the only difference between the two modes. Without it,
`{3}` is static emphasis: always applied, consuming no steps, and baked into
the published deck. With it, `{2|5-7|10}` becomes a walkthrough — one step per
group, with the emphasis moving rather than accumulating as you advance.

The third slide uses `{1,4-5}` on a block with no language tag, showing that
`,` lists individual entries, `-` makes a range, and emphasis does not require
highlighting. The fourth slide mixes emphasis with `::: {reveal}`, since
emphasis steps *are* reveal steps and both advance in source order. The last
slide is the contrast case: a block with no spec renders exactly as it always
has.

To step through the emphasis locally, run
`peitho present examples/code-emphasis/deck.md` and use the arrow keys.

## What to look at

Stepped emphasis is a pointer that follows narration, so it appears only in
`peitho present`. Published output, previews, PDF export, and the gallery
screenshot show those blocks unemphasized — freezing an arbitrary moment of a
moving pointer into a distributed artifact would assert something the author
never did. Static emphasis says "these lines are the important ones", which is
a property of the content, so it ships with the deck and is visible in the
published version above.

The deck uses no frontmatter, custom layouts, or custom CSS. Emphasis is styled
by the theme, and decks that ship their own CSS can restyle it through
`--peitho-emphasis-background`, `--peitho-emphasis-marker`, and
`--peitho-emphasis-dim`.
