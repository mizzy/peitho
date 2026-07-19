+++
title = "Footnotes"
weight = 68
template = "example-page.html"
description = "Slide-scoped Markdown footnotes for compact claims, repeated citations, list items, and literal marker escape hatches."

[extra]
deck = "footnotes"
demo_path = "/demo/footnotes/"
source_path = "static/deck-sources/footnotes/deck.md"
github_path = "examples/footnotes"
+++

## What it demonstrates

Footnotes show how a slide can keep claims short while retaining the evidence,
definitions, or source details behind them. References are scoped per slide,
numbered by first reference, and repeated labels reuse the same number.

The deck also shows that references inside list items are first-class content
and that footnote bodies can use inline Markdown such as emphasis, links, and
inline code.

## What to look at

The cited-claims slide defines labels out of display order to show that
numbering follows first-reference order. The list slide shows per-slide scoping
and the build-time contract: an undefined reference is an error with line
number and help text, not a missing marker in rendered HTML.

In the built-in layout, footnotes render at the bottom of the slide; custom
layouts without a `footnotes` slot keep the body-tail fallback. The final slide
demonstrates the escape hatches. A marker in a code span or regex character
class stays literal inline, and the fenced regex block stays literal in the
code figure above the footnotes footer. The real reference on the same slide is
validated and rendered as a plain superscript marker.
