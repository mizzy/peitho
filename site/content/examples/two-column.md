+++
title = "Two Column"
weight = 50
template = "example-page.html"
description = "A two-column deck that resolves matching block slots with explicit left and right slot fences."

[extra]
deck = "two-column"
demo_path = "/demo/two-column/"
source_path = "static/deck-sources/two-column/deck.md"
github_path = "examples/two-column"
+++

## What it demonstrates

Two Column exists for the case convention mapping cannot solve on its own. The
layout has `left` and `right` slots, and both accept block content, so the deck
uses `::: {slot=left}` and `::: {slot=right}` fences to route each column.

That explicit routing is still checked against the layout schema. A misspelled
slot name or unsupported fence shape is a build error, as described in
[Writing Decks](@/guide/writing-decks.md).

## What to look at

The source alternates between prose, subheadings, lists, and inline code inside
the two explicit slots. The CSS labels the columns as `slot=left` and
`slot=right`, making the slot routing visible in the rendered deck.
