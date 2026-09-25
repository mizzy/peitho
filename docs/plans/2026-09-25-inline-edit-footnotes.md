# Inline edit: footnote definitions are editable

## Problem

Preview inline editing (`docs/specs/2026-09-20-preview-inline-edit-design.md`)
listed footnote definitions as a non-goal. The footnote body at the bottom of a
slide was the one piece of rendered prose a click could not edit. Since
`docs/plans/2026-09-25-inline-edit-link-click.md`, a plain click on a link
edits its block, so a link in a footnote body was also the one link that still
opened on a plain click.

## Author decision (2026-09-25)

Footnote definition bodies are editable like any other paragraph ("footnotes
not being editable is counter-intuitive"). This removes "footnote definitions"
from the spec's non-goals. Only the body is editable. The `[^label]: ` marker
stays outside the span, so a click edits the text, not the label.

## Design

The parser remains the only authority on what is editable, and nothing
downstream gains a footnote-specific path:

1. `FootnoteCapture` records its paragraph with `source_slice_with_span` and
   attaches parser-authorized spans through the same helper
   `with_source_provenance` uses (`authorized_editable_spans`: the combined
   slice must be byte-identical to the stored Markdown, then
   `editable_spans_for_markdown`). The body is one paragraph, so the kind is
   `Paragraph`.
2. `FootnoteEntry` carries `source_span` and `editable_spans` (set only by the
   parser through a `pub(crate)` builder).
3. `ParsedSlide::editable_spans()` also collects footnote entries' spans. That
   is the single list `POST /slide-edit` authorizes against and `rewrite_block`
   compares, so saves need no change.
4. `render_footnotes_block` passes the entry's span and editable spans into
   `BodyMarkdownFragment` instead of `plain`, so the existing range-only
   annotation marks the body `<p>` under `EditAnnotations::On`. `Off` output is
   unchanged.

Safety comes from the existing reparse comparison. Adding a second paragraph,
a block construct, or a new or removed reference changes the parser's footnote
validation, the fragment kinds, or the editable block count and is refused.
