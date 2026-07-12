+++
title = "Custom Fonts"
weight = 35
template = "example-page.html"
description = "Bundled webfonts through the fonts/ auto-detect convention, with @font-face CSS and licenses riding along."

[extra]
deck = "custom-fonts"
demo_path = "/demo/custom-fonts/"
source_path = "static/deck-sources/custom-fonts/deck.md"
github_path = "examples/custom-fonts"
+++

## What it demonstrates

Custom Fonts is a fully zero-config deck: it has no frontmatter at all. Both the
theme (`css/`) and the webfonts (`fonts/`) sit next to `deck.md` and are picked up
by deck-adjacent auto-detect. Everything rendered — Playfair Display for text,
JetBrains Mono for code — is a bundled `.woff2` file; nothing loads from a CDN.

The `fonts/` directory is copied verbatim into every output (build, preview,
present, and PDF export) with no extension filter. That is deliberate: the SIL
Open Font License texts live beside the `.woff2` files and ship with the fonts,
and an `@font-face` CSS file could sit there too.

## What to look at

The theme's `@font-face` rules use relative URLs like
`url("fonts/playfair-display-latin.woff2")` — the emitted `peitho.css` sits next
to the copied `fonts/` directory, so no path rewriting is involved. The Playfair
file is a variable font declared with a `font-weight: 400 700` range, so one
38 KB latin subset covers regular body text and bold headings.

The cover slide's large type treatment is a keyed override
(`.peitho-slide[data-slide-key="cover"]` in `css/overrides.css`), validated
against the layout's slots like any other keyed selector.
