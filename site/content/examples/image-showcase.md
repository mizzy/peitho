+++
title = "Image Showcase"
weight = 30
template = "example-page.html"
description = "A minimal visual deck that maps one local Markdown image into a required image slot."

[extra]
deck = "image-showcase"
demo_path = "/demo/image-showcase/"
source_path = "static/deck-sources/image-showcase/deck.md"
+++

## What it demonstrates

Image Showcase is intentionally small: one heading and one local Markdown image.
The `visual` layout provides a required `accepts="image"` slot, so the image is
typed content rather than an arbitrary HTML embed.

The image path is deck-relative, and Peitho checks that the referenced local
asset exists. The convention mapping for image-only paragraphs is covered in
[Writing Decks](@/guide/writing-decks.md).

## What to look at

The CSS frames the diagram in a fixed visual area and uses `object-fit: contain`
so the asset remains inspectable. The Markdown stays just a heading plus
`![Architecture diagram](img/arch.png)`.
