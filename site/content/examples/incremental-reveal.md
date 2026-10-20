+++
title = "Incremental Reveal"
weight = 69
template = "example-page.html"
description = "A deck whose reveal steps walk in presenter mode and, with public_reveal: steps, in the published deck too."

[extra]
deck = "incremental-reveal"
demo_path = "/demo/incremental-reveal/"
source_path = "static/deck-sources/incremental-reveal/deck.md"
github_path = "examples/incremental-reveal"
demo_label = "Open published deck (steps)"
+++

## What it demonstrates

Incremental Reveal shows how `::: {reveal}` fences split normal Markdown content
into presenter-mode steps. The first slide reveals a bullet list item by item,
with a nested item traveling with its parent. The second slide uses two reveal
groups on one slide: a paragraph plus code block, then a checklist whose step
numbers continue from the first group.

Content outside reveal groups stays visible in every presenter state. The deck
sets `public_reveal: steps`, so the published deck above steps through the same
reveal groups with the arrow keys, and its URL keeps the step (`?slide=1&step=2`).
Without that key, published output renders the final state. Previews, PDF
export, and gallery screenshots render the final state either way.

To step through the reveal in presenter mode, run `peitho present examples/incremental-reveal/deck.md` and use the arrow keys.

## What to look at

The deck's only frontmatter is `public_reveal: steps`; it uses no custom layouts
or custom CSS. Built-in convention
mapping sends the heading to the title slot, prose and lists to body, and the
Rust block to code. The final slide has no reveal fences, making it a contrast
case for ordinary static slides.
