+++
title = "Math"
weight = 67
template = "example-page.html"
description = "Build-time LaTeX math rendering with embedded KaTeX CSS and fonts."

[extra]
deck = "math"
demo_path = "/demo/math/"
source_path = "static/deck-sources/math/deck.md"
github_path = "examples/math"
+++

## What it demonstrates

Math shows fenced `math` blocks rendered at build time into KaTeX HTML+MathML.
The output is body content, not an image, and the deck does not need
client-side math JavaScript.

When a deck contains math, Peitho prepends the embedded KaTeX CSS to
`peitho.css` and writes the matching fonts under `katex-fonts/`.

## What to look at

The deck uses zero `code_images:` configuration. The LaTeX source remains in
the Markdown deck and manifest text, while the slide HTML contains the rendered
KaTeX fragment.
