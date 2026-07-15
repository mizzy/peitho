+++
title = "Code Images"
weight = 65
template = "example-page.html"
description = "Diagrams-as-code: built-in Mermaid and user-declared Graphviz transformed to SVG images at build time."

[extra]
deck = "code-images"
demo_path = "/demo/code-images/"
source_path = "static/deck-sources/code-images/deck.md"
github_path = "examples/code-images"
+++

## What it demonstrates

Code Images shows fenced diagram source turning into SVG images during the
build. Mermaid fences use Peitho's built-in renderer; the deck declares a
Graphviz command for `dot` fences with `code_images:` frontmatter.

The resulting SVGs are cached and then treated like normal local images. That
means the layouts use ordinary `accepts="image"` slots, and the rest of the
pipeline does not need a separate diagram type.

## What to look at

The Mermaid slide demonstrates the built-in path from fenced source to rendered
image. The Graphviz slide exercises a user-declared command whose SVG output
includes XML/comment/DOCTYPE preamble before `<svg>`. The before/after slide
keeps the same Mermaid source visible as highlighted Markdown beside the
transformed SVG.

The final slide shows the frontmatter for the non-built-in `dot` renderer. The
same configuration shape is documented in
[Frontmatter](@/guide/frontmatter.md#code-images).
