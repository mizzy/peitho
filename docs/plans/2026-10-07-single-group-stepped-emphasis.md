# Single-group stepped emphasis (Issue #693)

## Problem

The `|` separator is the sole discriminator between static and stepped line
emphasis, so a stepped spec needs at least two groups. A slide that points at
one line in each of two code blocks, one after the other, has no notation:
`{1}` is static, `{1|}` is an empty-group error, and `{1|1}` spends a keypress
on a step that changes nothing.

## Decision

A leading `|` marks a spec as stepped: `{|1}` is one stepped group
emphasizing line 1. The grammar becomes

```text
spec := "|"? group ("|" group)*
```

- `{1}` stays static, `{1|}` stays the empty-group error, `{||1}` and `{|}`
  are empty-group errors (the leading separator is consumed once, then the
  remaining text is split as before).
- `{|1|3}` is accepted and means the same as `{1|3}` — the leading `|` is a
  redundant but unambiguous marker, not a different mode.
- The discriminator is unchanged in spirit: a spec is stepped iff it contains
  `|`.

The issue's open question — what a block shows while another block's step is
active — is already answered by the shell: `data-emphasis-active` toggles on
**equality** with the current step, and steps share one slide-wide space in
source order, so block 1's emphasis clears when block 2's step begins. No
shell, render, manifest, or bindings change.

## Tasks

1. `emphasis.rs`: accept the leading separator in `parse_emphasis_spec`; unit
   tests for `|1` (stepped, one group), `|1|3`, `|`, `||1`; move `|2` out of
   the error table.
2. `parser.rs`: a test that two blocks each written `{|1}` give the slide two
   steps with spans starting at 1 and 2.
3. Docs: guide (`site/content/guide/writing-decks.md`) and the design spec
   grammar.
