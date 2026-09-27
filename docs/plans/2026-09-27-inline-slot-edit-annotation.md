# Inline-slot edit annotations without a wrapper element (Issue #678)

## Problem

With `EditAnnotations::On`, `render_heading_inline_fragment` wrapped every
annotated `accepts="inline"` fragment in a fresh
`<span data-peitho-src data-peitho-md>`. `Off` has no such element, so preview
and build had different DOM trees. Theme CSS that addresses the slot's
children (`.slot-title > em`, or `.slot-title { display: contents }` feeding a
flex parent) matched in `dist/` but not in preview, which defeats preview as
a visual check.

Every other annotation already rides an element that exists in both modes
(`p`, `h1`–`h6`, `li`, `td`/`th`, footnote bodies). The inline slot was the
one path that created an element.

## Invariant

Edit annotations are attributes on elements the `Off` render already emits.
They never add, remove, or reorder elements. The existing property test
"On minus the attributes equals Off" states exactly this; it simply had no
inline-slot case, which is how the wrapper got through.

## Design

- `render_heading_inline_fragment` returns the rendered inline HTML plus an
  optional `EditAnnotation`, and never builds markup around it.
- `render_slot` (`Accepts::Inline`) puts the annotation on the slot's own
  `<span class="slot-…">` opening tag, next to `data-reveal-step` when the
  slot is revealed.
- A slot holding more than one inline fragment gets no annotation: there is
  no element per fragment to carry one, and the joined contents are not one
  editable span. Every inline slot in the built-in layouts and examples is
  `arity="1"`, so no shipped deck loses editing. A multi-fragment inline slot
  still edits through `e` (whole-slide source editing).
- Preview shell: when the edit target generates no box
  (`getComputedStyle(target).display === "contents"`, the `crn-post` case in
  the issue), the editor is an inner `<span>` holding the target's children,
  the same shape the `LI` path already uses, so the frame overlay measures a
  real box and the caret has somewhere to live. Measured E2E in Chrome.

## Tests

- Extend `edit_annotations_on_minus_attributes_equals_off` with inline-slot
  cases: single fragment, single fragment with inline markup (`*em*` first,
  the issue's eyebrow shape), and an `arity="1..*"` slot holding two
  fragments (the existing revealed-heading case stays).
- Replace `edit_annotations_on_wraps_each_accepts_inline_heading_fragment`
  with: single fragment annotates `<span class="slot-title" …>` directly;
  a revealed single fragment carries `data-reveal-step` and the annotation on
  that same tag; two fragments produce no annotation.
- Preview shell (vitest): editing a `display: contents` target uses an inner
  editor span and restores the original children on cancel/commit.
