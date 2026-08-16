# Strikethrough support (Issue #426)

Date: 2026-08-16
Issue: #426 — `~~x~~` passes through as literal text instead of rendering or erroring

## Problem

`ENABLE_STRIKETHROUGH` is absent from both the parse grammar
(`parser_options()`, `crates/peitho-core/src/parser.rs`) and the renderer's
`BODY_MARKDOWN_OPTIONS` (`crates/peitho-core/src/render.rs`), so
`This is ~~deleted~~ text.` builds without error and renders the tildes
literally. Unlike blockquotes/tables (loud `unsupported construct` errors),
this degrades silently — the pillar ③ failure mode.

## Decision

Enable the GFM strikethrough extension and render `<del>` (option 1 in the
issue). All three project lenses select it: it is the long-term behavior
users expect from GFM input, the change happens at the two grammar seams
rather than per-consumer, and pulldown-cmark's exhaustive event matches force
every walk site to make an explicit decision (no silent path can survive the
compile).

## Changes

1. `parser_options()`: add `Options::ENABLE_STRIKETHROUGH`. Leave
   `slide_split_options()` unchanged — strikethrough is inline and cannot
   affect `Event::Rule` detection; the split grammar stays minimal on
   purpose (two-grammar pitfall).
2. `BODY_MARKDOWN_OPTIONS` (render.rs): add `Options::ENABLE_STRIKETHROUGH`
   so fragment re-rendering emits `<del>`.
3. Chase the compile/behavior consequences at every exhaustive event match
   that can now legally see `Start(Tag::Strikethrough)` /
   `End(TagEnd::Strikethrough)`:
   - the main parse walk treats it like Emphasis/Strong (legal inline
     content, rides the fragment's markdown source),
   - the footnote-definition walk allows it wherever Emphasis is allowed,
   - plain-text extraction walks (key derivation, heading text, notes)
     treat it as transparent formatting (keep the inner text).
   No arm may become `_ => {}`.
4. Theme: no CSS required (`<del>` gets the UA line-through); do not add
   unsolicited styles.

## Tests (TDD order)

1. Parse: `~~deleted~~` inside a paragraph parses (no error) and the
   fragment markdown retains the source text.
2. Render: rendered slide HTML contains `<del>deleted</del>` (and not the
   literal tildes).
3. Strikethrough inside a heading: renders `<del>` in the heading, and the
   derived slide key from that heading text ignores the markup (matches how
   emphasis-in-heading keys behave today — mirror the existing test).
4. Strikethrough inside a footnote definition renders.
5. Regression: a paragraph with literal `~~` that is not closed (e.g.
   `a ~~ b`) still renders literally (GFM semantics, no error).

## Non-goals

- No `del`-specific theme styling.
- No change to the slide-split grammar.
