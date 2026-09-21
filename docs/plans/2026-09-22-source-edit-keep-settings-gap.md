# Keep the author's spacing after the settings comment (Issue #591)

Date: 2026-09-22
Follow-up to `docs/plans/2026-09-21-preview-source-edit.md` Task 3
(`rewrite_slide_body`), found by the Task 7 end-to-end review.

## Problem (measured)

`rewrite_slide_body` joined the non-empty parts of a slide (settings comment,
body, canonical note) with exactly one blank line. Every example deck writes

```markdown
<!-- {"key":"markers"} -->
# Title
```

so the first whole-slide source save of each slide inserted a blank line after
the settings comment. An edit-and-restore pass over all 19 example decks added
2–25 blank lines per deck and changed nothing else: formatting churn in a
project where Markdown is the hand-maintained source of truth.

## Rule

The original bytes between a settings comment and the body — the settings
line's terminator plus any blank lines, possibly none — are re-emitted verbatim,
together with the settings line's own indentation, when all of these hold:

- the settings comment is a whole line (only spaces/tabs before it, a line
  terminator after it);
- no note comment precedes it;
- everything between it and the first body line is blank in the **Markdown**
  sense (bytes `' '`, `'\t'`, `'\r'`, `'\n'` only);
- the new body is not empty.

Otherwise the canonical single blank line is used: an inline settings comment
that has to move out of its block, a note that preceded the settings comment,
anything non-blank in between. A comment line ends its HTML block, so a tight
body line after it cannot change parsing. With an empty body the saved gap is
discarded, so repeated saves cannot grow the file. The separator before the
canonical note comment is unchanged.

## Two things review caught

- **Unicode blank is not Markdown blank.** The first version tested the gap
  with `str::trim()`. A line holding only U+3000 (full-width space — realistic
  in a Japanese deck) or U+00A0 counted as blank and was re-emitted, but
  pulldown parses it as a paragraph, so a body `---` became the setext underline
  of an invisible paragraph and was *accepted* where `main` refuses it (1958
  diverging decisions in a 783k-rewrite differential fuzz). The new code uses
  one ASCII predicate. The pre-existing `trim()` calls in `notes_edit` and
  `normalize_body` are Issue #587.
- **No fabricated line numbers.** Keeping the gap shifts the candidate's line
  count, and the first version shifted parser error lines back to keep the
  refusal table's expectations. A diagnostic reports the line of the candidate
  that was parsed; the table rows moved by one instead (`json-comment`,
  `unknown-language`, `bad-slot`, `bad-reveal`, `bad-emphasis`).

## Verification

- Corpus property: identity rewrite is **byte-identical** for tight, one- and
  two-blank-line, indented, CRLF, BOM, non-first and last-slide decks whose
  notes are already canonical; idempotence for the whole corpus, empty bodies
  included.
- Differential fuzz against `main` (783k rewrites): every accept/refuse
  decision and every refusal message equal; emitted bytes differ only in the
  settings→body gap; refusal lines equal the true candidate line.
- End to end: edit-and-restore over all 19 example decks (176 saves) leaves
  every file byte-identical.
