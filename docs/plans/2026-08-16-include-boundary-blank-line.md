# Include boundary: make the splice seam construct-proof (Issue #421)

Date: 2026-08-16
Issue: #421 — Slide following an include is silently merged into the include's last slide

## Problem

Include expansion is a textual splice performed before pulldown-cmark runs
(`expand_includes_for_source`, `crates/peitho-core/src/include.rs`). The
boundary guards (`append_region_leading_newline_if_needed`,
`append_separator_boundary_newline_if_needed`) only guarantee the output ends
with a single `\n` before the deck's following separator line is copied back
in. When the included file ends in a paragraph, the spliced text is:

```
Included slide B
---
# Slide After Include
```

In CommonMark a `---` directly under a paragraph is a **setext H2 underline**,
not a thematic break, so `split_slide_ranges` never sees an `Event::Rule` and
the two slides silently fuse — the reported bug.

Every existing include test uses an included file whose last line is an ATX
heading (where the following `---` *is* a thematic break), which is why the
seam was never exercised.

## Root cause framing

The invariant that broke: **the include splice must not let the included
file's trailing parse state change the meaning of the deck text that follows
the include region.** Two constructs can leak across the seam:

1. **Trailing paragraph → setext underline.** The deck's own separator line is
   reinterpreted as part of the included content (the reported bug).
2. **Unclosed code fence in the included file.** After the splice, the open
   fence swallows the deck's separator *and everything after it* into a code
   block — same class, worse blast radius.

Both are fixed at the single splice seam; no downstream phase changes.

## Fix

### 1. Blank-line normalization before a following separator

Extend `append_separator_boundary_newline_if_needed` so that when the text at
`region.end` starts with a slide separator line, the output is guaranteed to
end with a **blank line** (`\n\n`), not merely a newline. Each synthetic `\n`
pushes a `synthetic_boundary_origin` (original_line == 0) exactly like the
existing single-newline guard, so `LineMap::translate` needs no changes.

This is unconditional normalization — no classification of "risky" trailing
constructs. A blank line before a thematic break is semantically inert for
every construct that can legally end an included file (paragraph, heading,
list, closed fence, blockquote-free peitho subset), so heading-ending includes
keep building the same slides; only the intermediate expanded source gains one
synthetic line.

### 2. Included files that end inside an unclosed block are build errors

(Revised 2026-08-16 after adversarial review; the first cut was a hand-rolled
`opening_code_fence` / `is_closing_code_fence` scan, which false-positived on
4-space-indented ``` lines and on ``` inside closed HTML comments, and missed
the sibling leak entirely: an included file ending with an unterminated
`<!--` swallows the following slides through CommonMark HTML block type 2 —
measured, silent. `<?`, `<!`, and `<![CDATA[` leak the same way.)

The shipped gate is `validate_included_source_boundary`: append a sentinel
(`\n\n---\n` + a probe paragraph) to the included file's raw source and parse
the whole thing with pulldown-cmark under `slide_split_options()` (made
`pub(crate)` so the probe and the real splitter share one grammar). If an
`Event::Rule` starts at or after the original source end, the file ends at a
clean block boundary. Otherwise the first event spanning the boundary names
the construct (code fence vs HTML block, exhaustively matched — no silent
arm) and its opening line, and the build fails with
`with_origin_file(&include_path)` plus help to close it. Because the real
grammar does the classification there are no false positives by
construction, and every construct blank lines cannot terminate (fences, HTML
blocks types 2–5) is covered by one total check. The gate runs per included
file at every recursion depth, on the raw on-disk source.

A standalone deck with an unclosed fence at EOF stays legal (pulldown-cmark
closes it at EOF; nothing follows) — only the include path creates the leak,
so only the include path gets the check.

## Tests (TDD order)

1. `include_ending_in_paragraph_keeps_following_slide` — deck with
   `include → separator → # After` where the included file ends in a
   paragraph; assert the parsed deck has all slides and `# After` is its own
   slide (this is the reproduction from #421; fails before the fix).
2. `include_ending_in_list_keeps_following_slide` — same shape, included file
   ends in a list item.
3. `include_ending_in_closed_fence_keeps_following_slide` — same shape,
   included file ends with a properly closed code fence.
4. Line-map regression: an error in the slide *after* the include still
   reports the deck file + correct source line (the synthetic blank line must
   not shift attribution).
5. `unclosed_fence_in_included_file_is_error` — included file with an
   unterminated ``` fence; assert error names the included file, the opening
   fence line, and carries help.
6. Existing include tests stay green (they assert parsed slides, not raw
   expanded bytes; update any that assert exact expanded source).

## Non-goals

- No change to slide splitting (`split_slide_ranges`) or the two-grammar
  frontmatter/split setup.
- No attempt to support included files that *intentionally* end mid-construct;
  the included file must be a self-contained sequence of slides (matches the
  #330 design).
