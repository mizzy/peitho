+++
title = "Peitho Tour"
weight = 70
template = "example-page.html"
description = "A twenty-minute pitch deck for Peitho itself, walking through concept, three pillars, and the write/preview/present loop across four custom layouts."

[extra]
deck = "peitho-tour"
demo_path = "/demo/peitho-tour/"
source_path = "static/deck-sources/peitho-tour/deck.md"
github_path = "examples/peitho-tour"
+++

## What it demonstrates

Peitho Tour is the broadest example deck. It is a real product pitch for Peitho
that runs for twenty minutes, divides the talk into six agenda sections
(`Intro`, `Install`, `Write`, `Design`, `Run`, `Close`), and moves through four
custom layouts: cover, topic, code, and shot.

The deck exercises most of the type-driven contract in one place: every slide
is routed by type-driven dispatch (no explicit `layout` pin), a screenshot
slide picks up the `shot` layout because it contains a lone image, and the
code-heavy slides fall onto the `code` layout because they include a fenced
code block. See [Writing Decks](@/guide/writing-decks.md) and
[Layouts](@/guide/layouts.md) for the underlying rules.

## What to look at

Look at the page settings comments around `cover`, `install`, `layout-schema`,
`preview`, and `present`. They carry `key`, `section`, and `time`, and together
they define the agenda that the presenter view lines up against actual times.

The adjacent CSS also matters: `overrides.css` targets stable slide keys like
`preview` and `overview` to tweak individual slides, while `base.css` styles
the shared cover, topic, code, and shot layouts without changing the Markdown
content.
