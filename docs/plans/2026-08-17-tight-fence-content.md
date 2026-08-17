# Tight content around `::: {slot=…}` fences (Issue #428)

Date: 2026-08-17
Issue: #428 — allow content on the line directly after a fence without a blank line

## Problem

The `:::` fenced-div grammar requires blank lines between the markers and
the content. Without one, pulldown-cmark fuses the marker line and the
content into a single paragraph block, and the parser rejects it
(`fused_div_marker_error`, three shapes: content after the opening marker,
before the closing marker, around a marker). #360 made that a loud error
instead of a silent drop; this issue removes the speed bump for the common
case, since Pandoc/Djot fenced divs accept tight content and real-world
authoring hits this repeatedly.

## Design

Split fused *paragraph* blocks at marker lines instead of rejecting them.
The line-first marker scan (`scan_div_markers`) already knows which lines
are markers; when a finalized pulldown block that spans a marker line is a
**Paragraph**, the block's line range is partitioned into marker lines and
content runs:

- marker on the first line(s): the marker feeds the div state machine as
  today; the remaining lines become a paragraph fragment whose line number
  is the first content line;
- marker on the last line(s): the leading lines become a paragraph
  fragment; the closing marker then closes the group after it;
- markers in the middle: each maximal content run becomes its own
  paragraph fragment, with the markers processed between them in order.

Splitting is sound for paragraphs because a paragraph's continuation lines
carry no block-level structure — each content run re-parses as exactly one
paragraph. Every other fused block shape (lists, headings via lazy
continuation, anything not a Paragraph) keeps the existing three
line-numbered errors verbatim: splitting a fused list at an interior line
would need real block surgery, and the loud error remains the honest
answer there (recorded as the deliberate boundary).

Fragment routing is unchanged: group membership is driven by the marker
positions, so the split fragments land inside/outside the slot group
exactly as if the author had written the blank lines.

## Tests (TDD order)

1. `::: {slot=left}` + content on the next line → builds; content lands in
   the left slot (this is the #428 reproduction; errors today).
2. Content on the line directly before the closing `:::` → builds.
3. Tight on both sides, single paragraph → builds.
4. Marker sandwiched mid-paragraph (`text / ::: / text` with no blanks) →
   both content runs land on the correct sides of the boundary.
5. Line numbers: an error inside tight content (e.g. unknown code language
   on a later fragment) still reports the correct source line.
6. Fused NON-paragraph shapes keep the existing errors: a list line
   directly after `:::` still errors with the current message.
7. `::: {reveal}` tight content works identically (same grammar).
8. Existing fence tests stay green (blank-line style keeps working).

## Non-goals

- No support for splitting fused non-paragraph blocks.
- No change to the marker grammar itself (attributes, nesting rules, 4+
  colon handling).
