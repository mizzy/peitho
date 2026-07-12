# Derive the landing gallery from the examples section (Issue #274)

Date: 2026-07-12

## Problem

The landing page's Examples gallery was a hand-written `[[extra.sections.gallery]]`
list in `site/content/_index.md`, duplicating the examples section. Adding an example
required remembering a third wiring site beyond `DEMO_DECKS` and the example page —
and it was in fact forgotten in the first drafts of #271/#272. The template's shape
checks catch malformed tiles, but cannot catch an absent one.

## Fix

Single source of truth. The landing section declares `gallery_from = "examples"` and
the template loops `get_section(path="examples/_index.md").pages` — the same loop
`examples.html` already uses (weight order, `/deck-shots/<extra.deck>.png` tiles, a
deliberate build failure if `extra.deck` is missing). The hand-curated `gallery` list
shape is removed from the template entirely, so the drifting representation is no
longer expressible; an unknown `gallery_from` value is a build failure via the
undefined-variable idiom the template already uses.

## Consequence

The landing gallery order becomes the examples-section weight order (identical to
`/examples/`) instead of a separate curation. Ordering stays adjustable in one place:
page weights.

## Alternative considered

Dropping the gallery from the landing page — rejected while the derived version is
this cheap.
