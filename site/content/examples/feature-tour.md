+++
title = "Feature Tour"
weight = 70
template = "example-page.html"
description = "A broad tour deck that combines custom layouts, presenter timing, notes, list slots, code slots, and keyed CSS."

[extra]
deck = "feature-tour"
demo_path = "/demo/feature-tour/"
source_path = "static/deck-sources/feature-tour/deck.md"
+++

## What it demonstrates

Feature Tour is the broadest example deck. It sets `time: 8m`, divides the talk
into agenda sections, uses speaker notes, and moves through four custom layouts:
cover, topic, agenda, and code demo.

The deck also shows why Peitho treats layouts as checked contracts. One slide
explicitly asks for the `agenda` layout to resolve a list-only ambiguity, while
the code-demo layout accepts multiple fenced code blocks for the Rust and JSON
contract example. See [Writing Decks](@/guide/writing-decks.md) and
[Layouts](@/guide/layouts.md) for the underlying rules.

## What to look at

Look at the page settings comments around `tour-cover`, `checks`, `cli`, and
`timing`. They carry keys, sections, times, and the explicit layout choice that
keeps dispatch predictable.

The adjacent CSS also matters: `overrides.css` targets stable slide keys, while
the base theme styles the checklist and highlighted code without changing the
Markdown content.
