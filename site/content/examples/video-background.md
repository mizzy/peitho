+++
title = "Video Background"
weight = 73
template = "example-page.html"
description = "A cover slide whose layout references a looping video and its poster directly, with no Markdown image syntax."

[extra]
deck = "video-background"
demo_path = "/demo/video-background/"
source_path = "static/deck-sources/video-background/deck.md"
github_path = "examples/video-background"
+++

## What it demonstrates

Not every asset belongs in the Markdown. A full-bleed video background is a
design decision, so it lives in the layout, where the deck's prose never
mentions it:

```html
<video class="backdrop" src="media/loop.mp4" poster="media/poster.png"
       autoplay muted loop playsinline></video>
```

peitho reads that reference out of the layout at parse time, resolves it against
the deck directory, copies the file under a content-hashed name, and rewrites the
attribute to point at the copy. The same applies to `<img src>`, `<script src>`,
`<link href>`, and the other attributes that load a subresource.

## What to look at

The deck source is two ordinary slides. Nothing in it names the video — that is
the point of [separating content from design](@/guide/_index.md), and the
asset pipeline treats a layout's own references exactly like a Markdown image.

A path that does not exist is a build error naming the layout and the attribute,
rather than a missing file discovered mid-talk. While `peitho preview` runs,
replacing the video on disk rebuilds the deck, and the development server
answers HTTP Range requests, which WebKit requires before it will play a video
at all.
