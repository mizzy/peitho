# Represent empty slots on the rendered slide (Issue #423)

Date: 2026-08-16
Issue: #423 — empty slots leave layout whitespace in wrapper elements, so `:empty` styling silently fails

## Problem

When a slot receives no fragments, `render_slot` returns an empty string and
the `<slot>` element is removed (`crates/peitho-core/src/render.rs`). The
layout author's wrapper element survives with the indentation whitespace that
surrounded the slot tag, so `.wrapper:empty { display: none }` silently does
nothing — whether `:empty` works depends on invisible layout formatting
(base.css's `.footnotes:empty` only works because that slot happens to be
written inline).

## Decision

Option 2 from the issue: make the state representable instead of inferable.
The rendered slide root gains `data-empty-slots="<name> <name>"` listing every
layout slot that received zero fragments, in sorted slot-name order (the
`~=` selector is order-insensitive, so declaration order would be
unobservable — see Changes). CSS targets the state deliberately:

```css
.peitho-slide[data-empty-slots~="quote"] .quote { display: none; }
```

All three lenses select this option: trimming adjacent whitespace (option 1)
guesses at which parent is "the wrapper" and can eat non-slot content;
documenting `:not(:has(.slot-*))` (option 3) leaves the silent trap in place.
The attribute is emitted **only when at least one slot is empty**, so decks
whose slides fill every slot build byte-identical.

## Changes

(Revised 2026-08-16 after adversarial review.)

1. `render.rs`: before the lol_html rewrite, compute the empty-slot list from
   the checked slots (a slot missing from the checked map counts as empty —
   the slot element handler still hard-errors on that unreachable state, so
   the two consumers of the invariant agree). Order is the sorted slot-map
   order: the `~=` attribute selector is order-insensitive, so declaration
   order is unobservable and not worth a parallel `Vec<SlotName>` shadowing
   the map (rejected: parallel state with an unguarded sync invariant). In
   the existing slide-root element handler, set `data-empty-slots` when the
   list is non-empty.
2. Theme validation: the documented consumption form
   `[data-empty-slots~="name"]` joins the validated selector vocabulary —
   `build_theme_css` checks slot names inside such attribute selectors the
   same way it checks `.slot-*` classes (typos and renamed slots are
   line-numbered build errors, not silent no-ops; anything less would
   recreate the silent-CSS class this issue exists to kill).
3. `themes/base.css` (and the example copy `examples/footnotes/css/base.css`):
   `.footnotes:empty { display: none }` is a first-party sibling of the same
   whitespace-sensitive `:empty` bug class — it only works because the
   built-in layout happens to write that one slot inline. Replace it with
   `.peitho-slide[data-empty-slots~="footnotes"] .footnotes { display: none }`
   so the built-in theme consumes the new mechanism and stops depending on
   invisible layout formatting.
4. Docs: README's layout/slot section + the guide's slot styling discussion
   gain the CSS recipe, with a clarifying sentence that the wrapper class in
   the recipe is the layout author's own class (the attribute token is the
   slot name; the system does not tie the two together).

## Tests (TDD order)

1. A slide leaving one optional slot empty renders
   `data-empty-slots="quote"` on the root; the filled slots are not listed.
2. Two empty slots → both names, sorted slot-name order.
3. All slots filled → attribute absent entirely (byte-identical guarantee).
4. Regression: `data-slide-key` and `peitho-slide` class stamping unchanged.

## Non-goals

- No whitespace trimming around removed slots.
- No per-slot marker elements.
