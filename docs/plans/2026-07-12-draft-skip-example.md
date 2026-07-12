# Draft/skip example (Issue #268)

Date: 2026-07-12

## Goal

Give the per-slide `{"draft":true}` / `{"skip":true}` flags (Issue #242, shipped in
v1.5.0) a runnable example. The behavior is invisible in a static gallery — draft
slides are dropped at build and skip only manifests in presenter navigation — so this
is a local example with a README, modeled after `examples/pdf-export/`.

## Shape

```
examples/draft-skip/
  deck.md      # four source slides: cover, skip, draft, rules
  README.md    # what to run and what to observe
```

Not added to `DEMO_DECKS` and no `site/content/examples/` page, same as pdf-export.

## Content

The deck is self-describing: the skipped slide's bullets state the skip semantics
(and its speaker note demonstrates that skipped slides keep notes), the draft slide
invites the reader to build and count slides, and a closing slide lists the invalid
combinations. The README shows the measured build output (`built 3 slide(s)` from
four source slides, `"skip": true` in `manifest.json`) and the exact
draft+skip build error text.
