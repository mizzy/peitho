+++
title = "Custom Syntax"
weight = 66
template = "example-page.html"
description = "A deck-adjacent .sublime-syntax grammar that turns an unknown-language build error into build-time highlighting."

[extra]
deck = "custom-syntax"
demo_path = "/demo/custom-syntax/"
source_path = "static/deck-sources/custom-syntax/deck.md"
github_path = "examples/custom-syntax"
+++

## What it demonstrates

Peitho highlights code at build time with syntect, and a language tag it cannot
resolve is a line-numbered parse error — never silently plain text. TOML is
genuinely absent from syntect's built-in set, so the cover slide's `toml` fence
would fail the build on its own. The fix is the deck's only asset: a compact
`syntaxes/toml.sublime-syntax` grammar, picked up by deck-adjacent auto-detect
and merged into the built-in syntax set.

The deck building at all is the proof that the grammar loads; the second slide
shows the exact error you would get without it.

## What to look at

The grammar is about a page of YAML. Its scopes are chosen to hit the `hl-*`
classes the built-in theme colors — highlighting output is spans with classes
like `hl-comment` and `hl-constant`, so the colors stay in theme CSS and a
custom grammar needs no styling of its own. Layout and theme are the built-in
defaults: the one thing this deck adds is a grammar.
