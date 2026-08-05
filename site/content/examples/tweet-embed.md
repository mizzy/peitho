+++
title = "Tweet Embed"
weight = 72
template = "example-page.html"
description = "An X post rendered at build time as a cached screenshot or an opt-in static card."

[extra]
deck = "tweet-embed"
demo_path = "/demo/tweet-embed/"
source_path = "static/deck-sources/tweet-embed/deck.md"
github_path = "examples/tweet-embed"
+++

## What it demonstrates

Tweet Embed shows both build-time representations of one X status URL: the
default PNG screenshot of the official widget and the opt-in static card. The
published deck has no external script or live social embed.

Committed PNG and oEmbed JSON caches let CI and the demo site build both slides
without Chrome or network access to X.

## What to look at

Slide 1 sends the cached screenshot through a required `accepts="image"` slot,
preserving the official widget's pixels in HTML and PDF.

Slide 2 opts into card mode:

````markdown
```embed mode=card
https://x.com/gosukenator/status/2074821309259973046
```
````

The card is selectable HTML regenerated from cached raw JSON, routes to an
`accepts="blocks"` slot, and follows deck CSS variables such as
`--peitho-embed-card-link-color`. It never invokes Chrome.
