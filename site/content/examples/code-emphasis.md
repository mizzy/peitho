+++
title = "Code Line Emphasis"
weight = 71
template = "example-page.html"
description = "A deck that emphasizes specific code lines, steps through a function, and labels paired source and output blocks."

[extra]
deck = "code-emphasis"
demo_path = "/demo/code-emphasis/"
source_path = "static/deck-sources/code-emphasis/deck.md"
github_path = "examples/code-emphasis"
demo_label = "Open published deck (steps)"
+++

## What it demonstrates

Code Line Emphasis shows how a brace group in a fence info string marks the
lines you are talking about. This is a separate layer from syntax highlighting:
highlighting colors code by what it *is*, emphasis marks where you are in the
talk.

The `|` separator is the only difference between the two modes. Without it,
`{3}` is static emphasis: always applied, consuming no steps, and baked into
the published deck. With it, `{2|4-6|9}` becomes a walkthrough — one step per
group, with the emphasis moving rather than accumulating as you advance.

The third slide uses `{1,4-5}` on a block with no language tag, showing that
`,` lists individual entries, `-` makes a range, and emphasis does not require
highlighting. The fourth slide mixes emphasis with `::: {reveal}`, since
emphasis steps *are* reveal steps and both advance in source order. The fifth
slide is the contrast case: a block with no spec renders exactly as it always
has. The final slide stacks Terraform source above its plan output.
Each fence has its own `title="..."` caption and static emphasis; placing the
title before `{2}` on one fence and after it on the other shows that the two
attributes compose in either order.

The deck sets `public_reveal: steps`, so the published deck above walks the
stepped emphasis with the arrow keys. To step through it in presenter mode, run
`peitho present examples/code-emphasis/deck.md`.

## What to look at

Stepped emphasis is a pointer that follows narration, so it appears while
walking steps in `peitho present` and on the `peitho preview` stage regardless
of `public_reveal`. With `public_reveal: steps`, it also appears in the
published viewer and PDF export; the PDF contains every exact-match emphasis
position in the shared step sequence. A direct preview single-mode landing
starts at step 0, so stepped emphasis activates only after you advance to its
matching step. Preview filmstrip thumbnails and overview tiles, lint, the
gallery screenshot, and default/final published and PDF output leave that
pointer inactive. Static emphasis says "these lines are the important ones",
which is a property of the content, so it ships with the deck and is visible in
the published version above.

The deck's only frontmatter is `public_reveal: steps`. Its `layouts/title-body-code.html` is a copy of
the built-in layout with one intentional change: the `code` slot arity is
`0..*` instead of `0..1`, allowing the final slide to carry two typed code
blocks while the existing one-block slides render the same HTML. There is no
custom CSS: the built-in theme styles the captions.

When a deck uses static emphasis, its default styling travels with the deck,
including when the deck has its own `css/`. Restyle it with the
`--peitho-emphasis-background`, `--peitho-emphasis-marker`, and
`--peitho-emphasis-dim` custom properties, or override
`.code-line-emphasis` directly. Stepped emphasis is styled by the present
shell and the published viewer, through the same custom properties.
