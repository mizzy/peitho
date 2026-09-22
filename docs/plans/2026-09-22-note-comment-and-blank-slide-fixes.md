# Three deferred parser/notes defects (#582, #584, #595)

Date: 2026-09-22. These were found by review and fuzzing during the preview
source-edit milestone (#566–#580) and deferred with options for the author.
Each option below was selected because the long-term, type-safety, and
root-cause lenses agree on it; where they did not agree the narrower fix is
recorded as the reason.

All three share one theme: **the parser and the edit helpers disagree about
what a "blank line" or a "note comment" is**, and each disagreement currently
resolves by dropping or mangling content silently — which pillar ③ forbids.

## Task 1 — #595: a slide holding only U+3000/NBSP is silently dropped

`split_slide_ranges` (`parser.rs`) filters candidate ranges with Unicode
`str::trim()`, so a range whose only content is U+3000 (ideographic space) or
U+00A0 (NBSP) is not a slide — even though pulldown parses it as a paragraph
(`Start(Paragraph) Text("\u{3000}") End(Paragraph)`).

Measured on `main`: a three-slide deck whose middle slide is a single U+3000
line builds 2 slides with no diagnostic, and the third slide silently becomes
index 2.

**Chosen: option 1 — treat "blank" as Markdown does (ASCII only).** Use the
existing shared predicate `notes_edit::is_ascii_blank_line` in the split
filter instead of a second definition of "blank". The range then becomes a
real slide and goes through normal layout dispatch and slot checks, so an
author who writes `　` for a deliberate pause slide gets a slide (or a
line-numbered slot error), never a silent drop.

Why not option 2 (line-numbered error): it forbids a legitimate Japanese
authoring habit for no invariant gain. Why not option 3 (document it): the
silent index shift is the defect.

- Make `is_ascii_blank_line` visible to `parser.rs` (it is already
  `pub(crate)`), and use it for the range filter. Do not add a new predicate.
- The filter works on a whole range, not one line: the range is blank when
  every line in it is ASCII-blank. Keep it allocation-free.
- Red tests: a middle slide of only U+3000 becomes a real slide and later
  slide indexes do not shift; NBSP likewise; a range of ASCII spaces/tabs/CRLF
  is still dropped (unchanged); a deck of only U+3000 no longer reports
  `deck has no slides` but whatever the layout check says.
- Verify no example deck changes: `grep -rlP '\x{3000}|\x{00A0}' examples/*/deck.md`
  is empty today, and the byte-identical build snapshot must stay green.

## Task 2 — #584: two comments on one line become one note with leaked delimiters

`<!-- a --> <!-- b -->` on one line is a single pulldown HTML block, and the
note collector takes the whole block, so the note text becomes
`a --> <!-- b` — comment delimiters leak into `notes.json` and the presenter.
`notes_edit::spans_match_source` already refuses to treat such a span as one
comment, so **the parser and the edit guard disagree about what a note comment
is** — and the slide becomes uneditable from preview with a message that does
not name the cause.

**Chosen: option 2 — split the block into individual comments, each its own
note span.** This makes the two definitions agree, which is the root cause;
option 1 (parse error) would break decks that build today for a construct
whose meaning is unambiguous, and option 3 leaves the leak.

- One shared scanner decides what a note comment is; the parser's collector
  and `spans_match_source` must both go through it. Do not copy the scan.
- Only split a block that is **entirely** a sequence of complete comments
  separated by ASCII blank space. Anything else (text around a comment, an
  unterminated comment) keeps today's behaviour so no new silent path opens.
- Each recorded span must satisfy the existing "exactly one comment"
  predicate, so the preview note/source save paths accept the slide afterwards.
- Red tests: the issue's example yields notes `a` and `b` joined per the
  existing multi-comment rule (blank line), not `a --> <!-- b`; `notes.json`
  contains no `-->`; a preview note save on that slide now succeeds; a line
  mixing text and a comment is unchanged; an unterminated comment is unchanged.

## Task 3 — #582: a note between tight list items turns the list loose

`notes_edit::removal_edit` replaces a whole-line comment that sits between two
non-blank lines with a **blank line** rather than deleting the line. That is
deliberate — deleting it can join `text` to a following `---`/`===` into a
setext heading and merge slides. Side effect: between tight list items the
blank line makes the list loose, changing the rendering (`<li>item` becomes
`<li><p>item</p>`), and `rewrite_note`'s postcondition compares keys,
sections, flags and notes but not fragment shape, so it is accepted silently.

**Chosen: option 3 — decide per span by reparsing both candidates.** Keep the
existing blank-line form when it preserves the slide's fragment shape; only
delete the line when the blank form changes shape and deletion preserves it.
When neither preserves shape, keep the blank-line form and let the existing
postcondition speak. This removes the guesswork at the seam rather than
refusing the save (option 2) or documenting a silent rendering change
(option 1).

This is deliberately broader than tight lists: the same shape comparison
removes safe residue lines left by blockquote-hosted and indented notes. The
classic `> q` / `> <!-- x -->` / `> more` case still retains its `>` because
there the blank-line form is the shape-preserving candidate.

- The choice belongs to the removal seam, not to each caller: both
  `rewrite_note` and the whole-slide source path go through
  `remove_comment_spans`, so the selection must live there (or in one helper
  it calls) and must not be duplicated.
- Extend `rewrite_note`'s reparse postcondition to compare the **target
  slide's fragment shape** too, so that any future removal rule that changes
  rendering is refused rather than accepted silently. This is the type-level
  half: today the postcondition cannot see the defect at all.
- Red tests: the issue's example round-trips with the list still tight and the
  note moved to the canonical position; the setext hazard case
  (`text` / comment / `---`) still uses the blank line and still does not merge
  slides; `para1` / comment / `para2` (no blank lines) does not become one
  paragraph; a note save that would change fragment shape in a way no
  candidate fixes is refused with a reason rather than silently accepted.

## Gates

The repo gate list in CLAUDE.md, plus: every example deck's build output stays
byte-identical except where a test states otherwise, and `bindings/` shows no
drift (no contract type changes are expected).
