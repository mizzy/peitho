# Preserve non-ASCII whitespace paragraphs during source edits (Issue #587)

Date: 2026-09-22

## Problem

Markdown blank lines contain only ASCII space, tab, and the line terminator.
The edit helpers instead used Unicode `str::trim()` in several blank-line
decisions. In the measured note-save reproduction,
`# I1\n\ntext\n\n\u{a0}\n\n---…` became
`# I1\n\ntext\n\n<!-- n -->\n---…`: the NBSP paragraph was silently
deleted, and the reparse postcondition did not compare fragments. A U+3000
paragraph could likewise disappear from a whole-slide identity save or from
the body shown in the editor.

Form feed and vertical tab lines are also content, not Markdown blank lines.
In the measured pulldown case `a\n\x0c\nb`, the three lines form one paragraph
with soft breaks: the form-feed-only line does not end the paragraph. Main
likewise deleted a form-feed-only line during a note save.

## Decision

There is one `pub(crate)` predicate for Markdown blank lines in
`crates/peitho-core/src/notes_edit.rs`. It accepts only space, tab, CR, and LF.
Both `notes_edit.rs` and `slide_source.rs` use it for whole-line suffixes,
non-blank neighbours, trailing and leading blank runs, settings gaps, and body
edge normalization. The CLI origin-tail helper remains private and performs an
equivalent byte-level trim over exactly the same four bytes.

Unicode trimming remains intentional for submitted speaker-note text,
diagnostic positioning, and validating or formatting comment text: those
operations concern human-authored comment content rather than Markdown blank
lines.

## Authorized legacy test updates

Two Issue #591 tests encoded the superseded Unicode-blank interpretation:

- The defensive `edge_blank_runs` refusal now uses a fabricated span over
  `"  \n\t\n"`. Parser-produced slides always contain a Markdown-nonblank body,
  settings comment, or note, so the helper branch remains defensive and its
  refusal message stays pinned directly.
- The settings-gap regression now asserts that U+3000 and NBSP lines are body
  content: extraction starts with the line, an identity rewrite is
  byte-identical, and replacing the body with `---` retains the measured
  round-trip refusal. The old canonical-gap assertion classified body content
  as spacing and therefore no longer described Markdown semantics.

## Parser measurement (not fixed here)

The one-line decks `"\u{3000}\n"` and `"\u{a0}\n"` each produce zero ranges
from `split_slide_ranges`, so deck parsing reports zero slides (`deck has no
slides`). With the same slide-splitting pulldown options, each source emits
`Start(Paragraph)`, `Text("…")`, and `End(Paragraph)`. The discrepancy comes
from the range filter's Unicode `trim()` and is deliberately not fixed here.
Choosing whether such input becomes a real slide or a line-numbered build error
is a product decision; either answer changes what existing decks build to. That
decision is tracked as Issue #595 for the author. Edit helpers never receive a
slide that `split_slide_ranges` dropped, so the parser and editor definitions
cannot disagree for any slide that exists.

## Verification

Regression tests cover note append and replacement paths, note removal next to
Unicode whitespace paragraphs, slide-body extraction, byte-identical
whole-slide identity rewrites, corpus identity at slide edges, and clipped
include-origin writes. The focused tests failed before the implementation and
pass afterward; `cargo test -p peitho-core` passes 1107 tests with one existing
manual measurement ignored.

Differential fuzzing against `origin/main` found zero divergences for decks
without non-ASCII-whitespace-only lines and no lost content for decks with
them; main lost or hid those lines in about 18,000 generated cases.

Repeated `cargo test --workspace` runs cannot bind `127.0.0.1:0` in this
sandbox. A `--no-fail-fast` run completed every target: all non-socket tests
passed, while 63 server tests across the peitho library, binary, and `present`
integration target failed at socket setup only. No ignored or Chrome tests were
run. `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all --check`, and `git diff --exit-code bindings/` pass.

## Residual inside comment spans (accepted)

Whether a span is "just a comment" is still decided with Unicode trimming, in
the parser (`page_settings_comment_body`, `raw.trim()`) and in
`spans_match_source::comment_span` (`raw.trim_end()`), identically on `main`.
Unicode whitespace that pulldown puts *inside* a note's HTML-block span is
therefore replaced together with the comment: `<!-- note -->\u{3000}` loses
the trailing U+3000 on a note save, exactly as ASCII trailing spaces do. Nothing
visible is lost on LF/CRLF decks. The guard and the parser must agree, so
changing one without the other would make the guard refuse decks that parse;
it belongs with Issue #595's decision about the parser.

A body consisting only of U+3000 on a slide with no settings comment or note is
now refused for slide count (the parser drops such a slide, Issue #595) instead
of being normalized to an empty body; the refusal is loud and names the cause.

