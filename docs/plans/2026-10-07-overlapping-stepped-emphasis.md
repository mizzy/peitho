# Overlapping stepped emphasis groups (Issue #696)

## Problem

`{1-5|3}` (show the whole block, then zoom into line 3) builds a slide with
two steps, but step 2 emphasizes nothing. `LineEmphasis::group_of` returns
only the *first* group containing a line, and `wrap_emphasis_lines` stamps one
`data-emphasis-step` per line, so line 3's membership in group 2 is dropped
silently — a pillar ③ violation.

## Decision (author, 2026-10-07)

Make overlap work rather than reject it. Overlap was always documented as
legal ("emphasized again"), and the reveal.js notation peitho mirrors
(`data-line-numbers="1-5|3"`) gives `{1-5|3}` exactly the zoom-in meaning.

## Design

- **Root seam, typed**: replace `group_of(line) -> Option<usize>` with
  `groups_of(line)` returning *every* group index containing the line, in
  ascending order. With no first-match accessor left, no consumer can drop a
  membership again.
- **Render**: a stepped line belonging to groups `g1 < g2 < …` emits
  `data-emphasis-step="<base+g1> <base+g2> …"` — a space-separated token list
  like `class`. Lines in a single group keep the exact current bytes
  (`data-emphasis-step="N"`), so existing decks build byte-identical. Static
  emphasis has one group, so it is unaffected (it only asks "any group?").
- **Shell**: `applyEmphasisState` splits the attribute on whitespace and
  toggles `data-emphasis-active` when the current step is one of the tokens
  (still equality per token: emphasis moves, never accumulates).
- No parser, manifest, step-count, or bindings change: step count is still the
  number of groups.

## Tasks

1. `emphasis.rs`: `groups_of`, tests for overlap (`1-5|3` → line 3 in
   `[0, 1]`, line 1 in `[0]`, line 6 none) replacing the first-match test.
2. `render.rs`: multi-token stamp test for `{1-5|3}`; existing single-group
   assertions unchanged.
3. `shell.ts`: vitest that a `"1 2"` marker is active at steps 1 and 2 and not
   at 0 or 3.
4. Docs: design spec render/shell sections, guide sentence on overlap.
