+++
title = "Live Showcase"
weight = 75
template = "example-page.html"
description = "An animated cover, a chart drawn from JSON in the Markdown, and a code slide you can edit and run on stage, all from layout scripts."

[extra]
deck = "live-showcase"
demo_path = "/demo/live-showcase/"
source_path = "static/deck-sources/live-showcase/deck.md"
github_path = "examples/live-showcase"
+++

## What it demonstrates

Layout scripts are enough for the effects people reach for a web framework to
get: a moving background, an animated chart, a live code editor. The Markdown
stays plain content: a heading, a sentence, a JSON block, a code block. Every
moving part lives in the layout that owns it.

- **Cover:** the `cover` layout draws a drifting particle field on a
  `<canvas>` behind the title.
- **Chart:** the `chart` layout reads the JSON code block from its `code` slot,
  hides it, and draws an SVG bar chart. An `IntersectionObserver` replays the
  grow-in each time the slide is shown, because peitho hides slides that are
  not current.
- **Playground:** the `playground` layout makes its syntax-highlighted code
  block editable in place and runs it with a captured `console.log` when you
  press Run.
  Typing in the editor never advances the deck.

## What to look at

The data and the code a slide shows are still Markdown, so they diff, review,
and render like any other content, and the same deck works without the
scripts: PDF export prints the chart's final frame and the editor's starting
code. Each script follows the pattern from the guide's
[Scripts in a layout](@/guide/layouts.md#scripts-in-a-layout) section: install
once, drain `window.__peithoShadowRoots`, listen for `peitho:shadow-mounted`,
and mount each root at most once.
