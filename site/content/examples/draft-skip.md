+++
title = "Draft, Skip, and Present-only"
weight = 76
template = "example-page.html"
description = "Per-slide flags that keep a slide out of the build, out of next/prev navigation, or out of everything you share."

[extra]
deck = "draft-skip"
demo_path = "/demo/draft-skip/"
source_path = "static/deck-sources/draft-skip/deck.md"
github_path = "examples/draft-skip"
+++

## What it demonstrates

The source has five slides, and the published demo has three. The difference
is the point of this example:

- The `{"draft":true}` slide is dropped at parse end, so it appears in no output
  at all, not even in `peitho present`.
- The `{"present_only":true}` slide appears in `peitho present` and
  `peitho preview` but is dropped from `peitho build`, `peitho publish`, and
  `peitho export pdf`. That is why it is missing here. `peitho lint` still
  checks it.
- The `{"skip":true}` slide is a real slide, so it is in the demo. In
  `peitho present` and `peitho preview`, `next`/`prev` step over it; the
  published viewer on this page pages through it normally.

## What to look at

Compare the source with the demo, then run `peitho present` on the deck to see
all four non-draft slides, including the present-only one, with `next`/`prev`
skipping the skipped slide. The flags are documented in
[Writing decks](@/guide/writing-decks.md).
