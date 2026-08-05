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

Tweet Embed turns one X status URL in an `embed` fence into a PNG screenshot of
the official widget during the build. Peitho caches that snapshot and sends it
through the ordinary image pipeline, so the published deck has no external
script or live social embed. The example deck uses this default screenshot mode.

The committed cache snapshot lets CI and the demo site build without Chrome or
network access to X. HTML and PDF use the same PNG, so the post looks identical
in both outputs and remains available offline.

For selectable text and deck-theme styling, add `mode: card` after the URL:

````markdown
```embed
https://x.com/gosukenator/status/2074821309259973046
mode: card
```
````

Card mode caches raw oEmbed JSON, rebuilds escaped card HTML on every build,
and uses system `curl` only on a cache miss. It never invokes Chrome. Cards
route to `accepts="blocks"` slots; screenshots route to `accepts="image"`
slots. `mode: screenshot` states the default explicitly.

## What to look at

The slide combines a short body line with the generated image in a
required `accepts="image"` slot. If the cache is absent and the post has been
deleted or cannot render, Peitho stops with a line-numbered build error instead
of silently dropping the embed.
