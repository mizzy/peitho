+++
title = "Minimal"
weight = 5
template = "example-page.html"
description = "A single-file starter deck that uses built-in defaults for layout, CSS, slide splitting, code, keys, and notes."

[extra]
demo_path = "/demo/minimal/"
source_path = "static/deck-sources/minimal/deck.md"
+++

## What it demonstrates

Minimal is the smallest complete deck in the repository. It has no frontmatter,
custom layout directory, or custom CSS; Peitho uses its built-in defaults and
splits slides on `---`.

The deck still exercises the core conventions: a heading becomes the title,
plain paragraphs and lists become body content, fenced Rust code maps to the
code slot, keyed comments can identify slides, and HTML comments become speaker
notes.

## What to inspect

Start here when learning the file shape, then move to
[Getting Started](@/guide/getting-started.md) and
[Writing Decks](@/guide/writing-decks.md) for the same ideas with commands and
authoring rules around them.
