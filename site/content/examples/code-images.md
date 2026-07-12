+++
title = "Code Images"
weight = 65
template = "example-page.html"
description = "Diagrams-as-code: fenced mermaid / dot blocks transformed to SVG images at build time via user-declared commands."

[extra]
deck = "code-images"
demo_path = "/demo/code-images/"
source_path = "static/deck-sources/code-images/deck.md"
github_path = "examples/code-images"
+++

## What it demonstrates

Code Images shows `code_images:` frontmatter turning fenced diagram source into
SVG images during the build. The deck declares two commands: Mermaid CLI for
`mermaid` fences and Graphviz for `dot` fences.

The resulting SVGs are cached and then treated like normal local images. That
means the layouts use ordinary `accepts="image"` slots, and the rest of the
pipeline does not need a separate diagram type.

## What to look at

The Mermaid slide demonstrates the standard path from fenced source to rendered
image. The Graphviz slide exercises SVG output with XML/comment/DOCTYPE
preamble before `<svg>`. The before/after slide keeps the same Mermaid source
visible as highlighted Markdown beside the transformed SVG.

The final slide shows the exact frontmatter that powers the deck. The same
configuration shape is documented in [Frontmatter](@/guide/frontmatter.md#code-images).
