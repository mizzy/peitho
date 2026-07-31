+++
title = "Incremental Reveal"
weight = 69
template = "example-page.html"
description = "A zero-config deck that marks presenter-mode reveal steps while published output remains fully visible."

[extra]
deck = "incremental-reveal"
demo_path = "/demo/incremental-reveal/"
source_path = "static/deck-sources/incremental-reveal/deck.md"
github_path = "examples/incremental-reveal"
+++

## What it demonstrates

Incremental Reveal shows how `::: {reveal}` fences split normal Markdown content
into presenter-mode steps. The first slide reveals a bullet list item by item,
with a nested item traveling with its parent. The second slide uses two reveal
groups on one slide: a paragraph plus code block, then a checklist whose step
numbers continue from the first group.

Content outside reveal groups stays visible in every presenter state. Published
output, previews, PDF export, and gallery screenshots render the final state, so
all reveal steps remain visible outside `peitho present`.

## What to look at

The deck uses no frontmatter, custom layouts, or custom CSS. Built-in convention
mapping sends the heading to the title slot, prose and lists to body, and the
Rust block to code. The final slide has no reveal fences, making it a contrast
case for ordinary static slides.
