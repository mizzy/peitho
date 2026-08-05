+++
title = "Tweet Embed"
weight = 72
template = "example-page.html"
description = "Cached X screenshot/card modes plus a generic oEmbed thumbnail card, all built offline."

[extra]
deck = "tweet-embed"
demo_path = "/demo/tweet-embed/"
source_path = "static/deck-sources/tweet-embed/deck.md"
github_path = "examples/tweet-embed"
+++

## What it demonstrates

Tweet Embed shows both build-time representations of one X status URL—the
default PNG screenshot of the official widget and the opt-in static card—plus a
generic static card discovered from a YouTube page. The published deck has no
external script, iframe, or live social embed.

Committed X PNG/JSON and generic raw JSON/JPEG caches let CI and the demo site
build all three slides without Chrome, curl, or network access. The discovery
page itself is intentionally not committed; a valid JSON cache hit skips it.

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

Slide 3 uses a bare non-X URL:

````markdown
```embed
https://www.youtube.com/watch?v=dQw4w9WgXcQ
```
````

Peitho normally discovers the page's JSON oEmbed endpoint, validates the raw
response, downloads the advertised JPEG thumbnail, and caches the JSON and
image bytes separately. This example commits exact copies of both measured
fixtures at their computed cache paths. The JPEG still travels through the
typed image resolver and is published under `assets/` with a content hash;
neither `.peitho` nor raw JSON enters the distribution.

Generic provider HTML is ignored. The generated card contains escaped title,
author, and provider metadata, links to the URL written in the deck, and uses
only the local published thumbnail. If a provider has no thumbnail, the same
path produces a text card instead. Generic embeds are always cards, so `mode=`
is reserved for X URLs.
