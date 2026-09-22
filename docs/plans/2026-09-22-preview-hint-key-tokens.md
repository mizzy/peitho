# Distinguish hint keys from their descriptions (#615)

<!-- derived-from ./2026-09-22-preview-editor-key-hints.md -->

Date: 2026-09-22. This follows the key naming from #612, the editor
affordances from #610, and the source-editor hint from #605. Those changes
made the slot say the right words; this one makes the words readable at a
glance.

## The problem

Each hint mixes two kinds of token in one flat string, in one color:

    Click text to edit · e for Markdown · Enter for notes
                         ^                ^^^^^
    Cmd/Ctrl+Enter or click away saves · Enter inserts a newline · Esc cancels
    ^^^^^^^^^^^^^^                       ^^^^^                     ^^^

The carets mark things the reader is meant to press. Everything else is
description. On screen they were identical: `#9ca3af`, separated only by `·`.
The guide's Markdown already draws this distinction with backticks; the UI did
not, so a reader skimming the row for "what do I press" had to parse each
segment to find out.

## What is not changing

The hint's own color stays `#9ca3af` (7.00:1 against the `#15181e` panel).
The hint is peripheral information: it must stay below the note body
(`#e5e7eb`, 14.36:1) and must not compete with the save-failure color
(`#f87171`, 6.43:1). The slot is blanked while a failure shows, and that
ordering only reads correctly if the hint is the quieter of the two. So this
is about distinguishing *within* the hint, not about making the hint louder.

Every hint's rendered text is byte-identical to before. The existing tests
that pin each hint as an exact string are the regression proof, and they were
neither weakened nor rewritten: `textContent` concatenates the new child
spans, so they pass unchanged only if the split reproduces each string exactly.

## Key styling

Key tokens render at `#cbd5e1` (11.97:1); descriptions inherit the slot's
`#9ca3af`. Color alone carries the distinction — no weight change.

A heavier weight was the considered alternative. It was rejected because
600-vs-700 is a weak signal at this size and color, and a font without that
weight axis falls back to an identical glyph, producing a distinction that
silently does nothing. Applying both color and weight was rejected as two
mechanisms for one job. Color is verifiable and sufficient.

Prose is not given an explicit per-token color; it inherits from the parent
span, so the neutral color keeps exactly one definition point.

## Shape: typed tokens, not markup

The root problem is that hint content carried no structure: the key and the
prose were indistinguishable to the renderer because they were indistinguishable
in the source.

Inline markup in the strings (backticks, as the guide uses) was rejected: a
missing backtick lives inside a string literal, where it looks like ordinary
prose and is nearly invisible in review.

So the hint constants became token lists:

    type HintToken =
      | { readonly kind: "key"; readonly text: string }
      | { readonly kind: "text"; readonly text: string };
    type HintTokens = readonly HintToken[];

`hintKey` / `hintText` keep that cheap enough to use consistently. The
separators and their surrounding spaces live inside the text tokens, so
concatenation reproduces the old strings exactly.

**What this does and does not buy.** Every token must name its kind, so the
classification is explicit at each one and a misclassification shows up as a
wrong constructor in the diff rather than as a missing character inside a
string. That is a real improvement in reviewability, but it is not a closed
class: `hintText("Press Ctrl+K to search")` type-checks, so an author adding a
future hint must still remember to *split* the string at its key boundaries.
The type prevents an unclassified token, not an unsplit one.

Closing it properly would mean rejecting key-looking substrings inside prose
tokens — a parser for English, clearly worse than the problem — so this stops
where it is deliberately. The current constants are pinned from both sides:
the pre-existing exact-string tests prove the rendered text is unchanged, and
the new per-token tests prove each key is classified.

`"Click text to edit"` holds no key token: clicking is not a key press.
`"Saving…"` is one text token — a hint with no keys must not be made to
invent one.

## Where the split happens

Per the issue, `panelHint()` keeps returning exactly one value; only its type
changes, from `string` to `HintTokens`, with the empty case from `""` to `[]`.
The total priority chain from #612 is untouched, and `activeEditHint`'s switch
stays exhaustive with no default arm. `editAffordanceHint` and `activeEditHint`
return `HintTokens` for the same reason.

The token-to-DOM step is confined to rendering. `renderPanelHint` clears the
slot with `replaceChildren()`, appends one `<span>` per token, and sets the
key color on key tokens only; `hidden` becomes `hint.length === 0`, which is
equivalent to the old `hint === ""` on every path because no constant is empty
and no token is an empty string.

`replaceChildren()` before each render is load-bearing. Assignment to
`textContent` was idempotent; appending is not, so without the clear a stale
key span would survive into the next hint — including into the hidden state,
where the slot would be invisible but non-empty. That is the case the new
`clears_hint_token_spans_when_the_single_slot_becomes_hidden` test pins.
The teardown path reaches the same clear: `destroy()` sets `destroyed` before
its unconditional `replaceActiveEdit(null)`, which re-renders the panel, and
`panelHint()`'s first rung then returns an empty token list.

## Scope

Preview only. The hint slot is preview chrome and never reaches `dist/`, so
the publish contamination check is unaffected, and no Rust code, contract
binding, or guide text changes. `dist/preview.js` is regenerated because it is
a committed, drift-checked build artifact.

## Verification

Beyond the workspace gates, the rendered result was measured in Chrome against
a running `peitho preview`, since jsdom cannot show computed color or
whitespace collapsing. Key tokens compute to `rgb(203, 213, 225)` and prose to
`rgb(156, 163, 175)`; the concatenated text matches the pre-change string; and
the separator spacing renders with no collapsed or doubled gaps, confirming
that moving the spaces to the edges of sibling inline boxes is safe.
