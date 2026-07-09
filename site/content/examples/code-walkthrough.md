+++
title = "Code Walkthrough"
weight = 60
template = "example-page.html"
description = "A Rust typestate walkthrough that requires one code block per slide and styles annotation text beside it like a terminal."

[extra]
demo_path = "/demo/code-walkthrough/"
source_path = "static/deck-sources/code-walkthrough/deck.md"
+++

## What it demonstrates

Code Walkthrough explains a Rust typestate pipeline through four slides. Each
slide has one heading, exactly one fenced Rust block, and optional annotation
bullets.

The custom layout makes that contract explicit with `accepts="code"` and
`arity="1"` for the code slot, plus a blocks slot for walkthrough text. If a
slide omits the code block or adds a second one, the deck stops at build time
rather than silently reshaping the content.

## What to look at

The theme is intentionally terminal-like: code owns the left side, annotation
text owns the right side, and the `payoff` slide gets a keyed CSS override in
`overrides.css`. The slot and override behavior is the same mechanism described
in [Layouts](@/guide/layouts.md).
