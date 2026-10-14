# Lint measures the live slide set (Issue #711)

## Problem

`peitho lint` built and measured the public deck (`BuildTarget::Public`), so
`present_only` slides were never linted and every slide after one was numbered
one lower than in `peitho preview`, whose badges count the live deck.

## Author decisions (2026-10-14)

- Lint slide numbers match preview's slide numbers.
- Lint also checks `present_only` slides.

## Design

Lint does two builds through the existing `build_cli_artifacts` seam:

1. `BuildTarget::Public`, result discarded: a deck `peitho build` refuses (for
   example "every published slide in the deck is marked present_only") still
   fails lint with the same error. No public-only rule is restated on the lint
   side, so the two can never drift.
2. `BuildTarget::Present` (Live audience, no edit annotations): the measured
   deck. Its slide order is the live order preview numbers with `index + 1`,
   so lint's `index + 1` in `lint_measure.js` equals preview's number by
   construction. Draft slides are absent from both, as before.

Rejected: build Live only and re-check public-only rules in lint (a second copy
of the rules that can drift from the parser).

The second build reuses the code-images and embeds caches the first one fills,
so it costs one more parse/render; only `code_images` and embed blocks on
present-only slides (which the public build never sees) can still run external
commands or fetches on a cold cache.

## Tests

- Lint artifacts for a deck with a `present_only` slide count it (slide count
  includes it, and its position is the live position).
- A deck where every non-draft slide is `present_only` fails lint with the
  public build's error.
- A draft slide is still not counted.

## Docs

`site/content/guide/writing-decks.md`, `site/content/guide/cli.md`,
`site/content/examples/draft-skip.md`, the `BuildTarget` doc comments, and the
present-only bullet in `CLAUDE.md`.
