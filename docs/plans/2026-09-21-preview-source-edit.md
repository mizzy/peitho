# Preview: edit whole-slide Markdown source in single mode

Date: 2026-09-21
Branch: `preview-source-edit`

<!-- derived-from ../specs/2026-09-21-preview-source-edit-design.md -->

## File map

| Change | Exact path | One responsibility | Depends on | Task(s) |
| --- | --- | --- | --- | --- |
| Modify | `crates/peitho-core/src/phase.rs` | Store the parse-only page-settings span on `ParsedSlide`. | Existing `SourceSpan` and `KeySource`. | 1 |
| Modify | `crates/peitho-core/src/parser.rs` | Capture `settings_span` and pass it out of `process_html_chunk`. | `phase.rs::ParsedSlide`. | 1 |
| Modify | `crates/peitho-core/src/mapping.rs` | Keep direct `ParsedSlide` test fixtures compiling without propagating the parse-only span. | Task 1 field addition. | 1 |
| Modify | `crates/peitho-core/src/code_images.rs` | Keep direct `ParsedSlide` test fixtures compiling without transforming source bookkeeping. | Task 1 field addition. | 1 |
| Modify | `crates/peitho-core/src/notes_edit.rs` | Expose the existing source-text helpers and consume the shared slide comparisons. | Existing note rewrite semantics. | 2-3 |
| Modify | `crates/peitho-core/src/slide_edit.rs` | Consume the neutral shared slide comparisons and derived-key rule. | Existing inline-edit postcondition. | 3 |
| Create | `crates/peitho-core/src/slide_compare.rs` | Own the one shared set of parsed-slide comparisons and difference names. | Existing inline-edit and note-edit postconditions. | 3 |
| Create | `crates/peitho-core/src/slide_source.rs` | Own core whole-slide source semantics, the accepted rewrite identity, and the serialized contract. | Tasks 1-3 parser and shared helpers. | 2-4, 7 |
| Modify | `crates/peitho-core/src/lib.rs` | Publish the slide-source API and serialized contract. | `slide_source.rs`. | 2, 7 |
| Modify | `crates/peitho/src/main.rs` | Orchestrate whole-slide preview I/O across the origin and cache seams. | Tasks 2-7 core/server contracts. | 5-8 |
| Modify | `crates/peitho/src/server.rs` | Add `/slide-source`, the closed write variant, and outcome-specific success JSON. | Task 5 save service. | 6 |
| Modify | `crates/peitho/tests/build.rs` | Prove ordinary builds never emit `sources.json`. | Task 7 emission. | 8 |
| Modify | `crates/peitho/tests/publish.rs` | Prove `sources.json` contaminates a publishable distribution. | Task 8 contamination list. | 8 |
| Create | `bindings/SlideSources.ts` | Commit the Rust-generated preview source-map contract. | Task 7 `SlideSources`. | 7 |
| Create | `packages/peitho-present/src/previewSourceEdit.ts` | Encapsulate one complete source-editor session. | Tasks 6-7 HTTP/JSON contracts. | 10-13 |
| Modify | `packages/peitho-present/src/keyboard.ts` | Share the existing IME-key predicate with both preview keyboard layers. | Existing keyboard primitives. | 9-10 |
| Modify | `packages/peitho-present/src/preview.ts` | Coordinate the closed edit union, navigation, reloads, notes, and source-map re-keying. | Tasks 7 and 10. | 9, 11-13 |
| Create | `packages/peitho-present/test/previewSourceEdit.test.ts` | Unit-test the source editor independently of controller coordination. | `previewSourceEdit.ts`. | 10, 13 |
| Modify | `packages/peitho-present/test/preview.test.ts` | Regress keyboard requests and the cross-editor lifecycle. | Tasks 9-13 shell changes. | 9, 11-13 |
| Modify | `packages/peitho-present/test/generated.test.ts` | Type-check the generated `SlideSources` shape. | Task 7 binding. | 7 |
| Modify after rebuild when bytes differ | `packages/peitho-present/dist/shell.js` | Commit the rebuilt audience-shell entry if the shared keyboard extraction changes it. | Task 9 shared keyboard module. | 13 |
| Modify | `packages/peitho-present/dist/preview.js` | Commit the rebuilt preview entry bundle. | Tasks 9-13 TypeScript. | 13 |
| Modify after rebuild when bytes differ | `packages/peitho-present/dist/remote.js` | Commit the rebuilt remote entry if the shared keyboard extraction changes it. | Task 9 shared keyboard module. | 13 |
| Modify | `CLAUDE.md` | Record the repository-wide whole-slide edit invariants. | Tasks 1-13 behavior. | 14 |
| Modify | `site/content/guide/cli.md` | Document the author workflow and refusal/recovery model. | Tasks 1-13 behavior. | 14 |
| Modify | `README.md` | Add the whole-slide editor to the preview feature overview. | Tasks 1-13 behavior. | 14 |
| Create (ignored) | `target/preview-source-edit-e2e/deck.md` | Drive the required real-Chrome acceptance session. | Task 15 checklist. | 15 |
| Create (ignored) | `target/preview-source-edit-e2e/included.md` | Hold the browser-edited slides and include-boundary cases. | Task 15 deck. | 15 |
| Create (ignored) | `target/preview-source-edit-e2e/css/base.css` | Supply the production base theme to the browser fixture. | `themes/base.css`. | 15 |
| Create (ignored) | `target/preview-source-edit-e2e/css/overrides.css` | Trigger the derived-key/check-phase recovery case. | Task 15 included slide key. | 15 |

## Goal

In `peitho preview` single mode, make unshifted `e` replace the current rendered
slide with a shell-owned textarea containing only that slide's body Markdown.
Cmd/Ctrl+Enter, blur, a transition, or page exit saves through
`POST /slide-source`; Escape cancels. Page settings and speaker-note comments
remain outside the editor, Markdown remains the only content model, and the
watcher remains the only rebuild trigger. This plan has no design deviations.

## Verified existing seams

- `ParsedSlide` currently keeps `source_span` and `note_spans` at
  `crates/peitho-core/src/phase.rs:492-505`. `process_html_chunk` already receives
  the exact HTML `SourceSpan` at `parser.rs:3256-3273`, but its settings branch
  records only `page_settings_line` at `parser.rs:3291-3305`.
- `notes_edit.rs` owns the shared `spans_match_source` and
  `remove_comment_spans` seams, while `parser.rs` owns
  `page_settings_comment_body`. Those implementations, together with the
  existing BOM and canonical-comment helpers, are the extraction points;
  `slide_source.rs` must not reproduce them.
- `slide_edit::compare_non_target_slide` is currently private at
  `slide_edit.rs:155-193`, and the derived-key exception is inline at
  `slide_edit.rs:275-280`. They are the two comparison rules to lift and share.
- `write_preview_origin_rewrite` is isolated at
  `crates/peitho/src/main.rs:2191-2274`. Its body already handles `LineMap`
  translation, origin drift, pure CRLF, and one atomic write; this function is
  deliberately unchanged.
- `DeckWrite` and `DeckWriter` are at `server.rs:443-458`; the shared HTTP
  handler is at `server.rs:1194-1235`, and its current success bytes are
  `{"saved":true}` at `server.rs:2199-2202`.
- Preview-only generation emission is centralized in
  `main.rs:5248-5276`; present-cache emission is separate at
  `main.rs:5184-5236`. The distribution contamination list is
  `PRESENTATION_ONLY_DIST_FILES` at `main.rs:913-920`.
- The preview shell currently stores `activeSlideEdit` at `preview.ts:439` and
  tests it in reload, start-edit, transition, and destruction paths. The new
  union replaces that field rather than adding a parallel source-edit flag.

## Tasks

### Task group 1: Parser provenance

#### Task 1: Capture the page-settings span as parse-only bookkeeping

**Goal.** Give every parsed slide either the exact page-settings comment span
or `None`, without carrying that field into Mapped, Checked, Rendered, the
manifest, or generated bindings.

**Files.**

- `crates/peitho-core/src/phase.rs`
- `crates/peitho-core/src/parser.rs`
- `crates/peitho-core/src/mapping.rs`
- `crates/peitho-core/src/code_images.rs`

**Test (Red).** Extend the parser tests beside
`parsed_slide_records_html_block_and_inline_note_spans` with these assertions:

The existing test at `parser.rs:6991-7008` proves the parser's pulldown-cmark
HTML-block range includes the line terminator after `-->`. Keep that convention
for settings: the first assertion below intentionally includes the terminal
`"\n"`; it must not be shortened to the comment's closing delimiter.

```rust
#[test]
fn parsed_slide_records_page_settings_span_separately_from_notes() {
    let source = "<!-- {\"key\":\"intro\",\"layout\":\"cover\"} -->\n\n# Title\n\n<!-- note -->";
    let slide = parse_first_slide(source);
    let settings = slide.settings_span.expect("settings span");

    assert_eq!(
        &source[settings.start..settings.end],
        "<!-- {\"key\":\"intro\",\"layout\":\"cover\"} -->\n"
    );
    assert_eq!(&source[slide.note_spans[0].start..slide.note_spans[0].end], "<!-- note -->");
}

#[test]
fn parsed_slide_without_page_settings_has_no_settings_span() {
    assert_eq!(parse_first_slide("# Title").settings_span, None);
}
```

Keep the existing assertion that settings and empty comments never enter
`note_spans`.

**Implementation (Green).** Add the field next to `note_spans`:

```rust
pub struct ParsedSlide {
    // existing fields
    pub notes: Option<String>,
    pub settings_span: Option<SourceSpan>,
    pub note_spans: Vec<SourceSpan>,
}
```

Create `let mut settings_span = None` in `parse_slide`, pass
`&mut Option<SourceSpan>` into both `process_html_chunk` call sites, and assign
the received pulldown-cmark span unchanged only in the successful page-settings
branch; do not trim its terminal `\n` or `\r\n` because Task 3 owns that
canonicalization. Initialize the new field in every direct test `ParsedSlide`
literal in `phase.rs`,
`mapping.rs`, and `code_images.rs`; do not add it to any later phase. Update
`UntransformedDeck`'s `parser.rs:525-529` documentation from the obsolete
“preview notes writer” wording to all parse-only preview deck writers.

**Verification.**

```sh
cargo test -p peitho-core parsed_slide_records_page_settings_span_separately_from_notes
cargo test -p peitho-core parsed_slide_without_page_settings_has_no_settings_span
cargo test -p peitho-core page_settings_and_empty_comments_do_not_get_note_spans
cargo test -p peitho-core --lib
```

### Task group 2: One body definition and one structural postcondition

#### Task 2: Define `slide_body` by sharing the note-removal primitives

**Goal.** Establish the one and only definition of slide body, including BOM,
CRLF, inline comments, and comment placement behavior.

**Files.**

- `crates/peitho-core/src/notes_edit.rs`
- `crates/peitho-core/src/slide_source.rs`
- `crates/peitho-core/src/lib.rs`

**Test (Red).** Add
`slide_source::tests::slide_body_removes_only_settings_and_note_comments` as a
table over these named cases and exact expectations:

```rust
let cases = [
    ("no-comments", "\n# Title\n\nBody\n\n", "# Title\n\nBody"),
    ("settings-only", "<!-- {\"key\":\"fixed\"} -->\n\n# Title", "# Title"),
    ("note-before", "<!-- note -->\n# Title", "# Title"),
    ("note-between", "Before\n<!-- note -->\nAfter", "Before\n\nAfter"),
    ("note-inline", "Text <!-- note --> more", "Text  more"),
    ("note-after", "# Title\n<!-- note -->", "# Title"),
    ("after-frontmatter", "---\ntime: 1m\n---\n\n# Title\n", "# Title"),
    ("crlf", "<!-- {\"key\":\"fixed\"} -->\r\n\r\n# Title\r\n", "# Title"),
    ("bom", "\u{feff}<!-- note -->\n# Title", "# Title"),
];
```

Add a multiple-note case proving every recorded note span is removed and an
interior whitespace-only line is otherwise retained.
Add `slide_body_rejects_spans_that_do_not_match_the_source` with these refusal
rows: an equal-length source puts non-comment bytes under the settings span; a
source is shorter than the recorded slide span; the settings span points at a
real note comment; the settings span equals a note span; and an equal-length
foreign source puts following body and note bytes inside a three-line settings
span. Every row must return a parse error with reload help rather than panic.

**Implementation (Green).** Create the required public function:

```rust
pub fn slide_body(source: &str, slide: &ParsedSlide) -> Result<String>;
```

It must call `notes_edit::strip_bom`, then the shared
`notes_edit::spans_match_source` before slicing. Generalize that existing
`rewrite_note` guard to accept an optional settings span and verify it is an
in-bounds, character-boundary-safe page-settings comment disjoint from the
note spans. `rewrite_note` passes no settings span and additionally refuses a
supplied note span that classifies as a settings comment, which the parser
never produces.
Refuse a mismatch with a parse `BuildError` and reload help. Then merge
`settings_span` with every `note_span`, call the shared
`notes_edit::remove_comment_spans`, slice the adjusted `source_span`, normalize
CRLF/bare CR to LF, and remove only leading/trailing blank lines. The shared
helper accepts disjoint spans in any order and sorts its local copy by `start`.
Lift the reverse-order removal loop from `splice_note` into that `pub(crate)`
helper and make both callers use it; keep `LineContext`, `line_context`, and
`removal_edit` private. Make `canonical_comment` and `source_line_ending`
`pub(crate)`; reuse the already-`pub(crate)` `strip_bom` and `restore_bom`.
Export `slide_source` from `lib.rs`. Keep the LF/edge-blank operation in one
private `normalize_body(&str) -> String` that calls the existing
`normalized_note_text` and is used by both `slide_body` and Task 3's submitted-
body round trip. Do not create another comment-removal or line-ending
normalizer.

**Verification.**

```sh
cargo test -p peitho-core slide_source::tests::slide_body_removes_only_settings_and_note_comments
cargo test -p peitho-core slide_source::tests::slide_body_rejects_spans_that_do_not_match_the_source
test "$(rg -n '^pub fn slide_body' crates | wc -l | tr -d ' ')" -eq 1
test "$(rg -n '^pub\(crate\) fn (spans_match_source|remove_comment_spans|page_settings_comment_body|canonical_comment|strip_bom|restore_bom)' crates/peitho-core/src | wc -l | tr -d ' ')" -eq 6
```

#### Task 3: Rewrite and canonicalize one slide body after a successful reparse

**Goal.** Replace a body while preserving edge blank runs, settings bytes,
notes, line endings, BOM, and all protected parse-level identity, then return
the accepted source, key, and body from the candidate reparse itself.

**Files.**

- `crates/peitho-core/src/slide_source.rs`
- `crates/peitho-core/src/slide_compare.rs`
- `crates/peitho-core/src/slide_edit.rs`
- `crates/peitho-core/src/notes_edit.rs` (consume the shared comparisons and
  expose the existing trailing-line helper)

**Test (Red).** Add
`rewrite_slide_body_canonicalizes_settings_body_and_one_note` with this source
and exact output:

```rust
let source = "\n<!-- {\"key\":\"fixed\"} -->\n\n<!-- note one -->\n\n# Old\n\nTail <!-- note two -->\n\n";
let expected = "\n<!-- {\"key\":\"fixed\"} -->\n\n# New\n\n- one\n\n<!--\nnote one\n\nnote two\n-->\n\n";
let highlighter = Highlighter::defaults();
let deck = parse_source(source, &highlighter).unwrap();
let rewritten = rewrite_slide_body(
    source,
    &deck.parsed_slides()[0],
    "# New\n\n- one",
    &highlighter,
)
.unwrap();
assert_eq!(rewritten.source, expected);
assert_eq!(rewritten.key.as_str(), "fixed");
assert_eq!(rewritten.body, "# New\n\n- one");
```

Add a table-driven
`rewrite_slide_body_accepts_structural_changes_and_preserves_source_conventions`
test for paragraph-to-list, a new code block, changed
reveal/emphasis shape, a derived heading key change, a fixed explicit key and
section marker,
one note comment that precedes the settings comment, pure CRLF output, leading
BOM preservation, and a configured `code_images` language whose external
renderer must not run. In the derived-heading row, assert all three returned
fields: the rewritten source, the new derived key, and the post-save body.
Add rows proving that `slide_body` and `rewrite_slide_body` refuse when the
BOM-stripped deck contains a bare CR anywhere, with the exact conversion help;
include a bare CR in the target, in another slide, on the separator line just
before the target, and in a tail swallowed by a submitted unclosed fence. LF
and CRLF identity rewrites remain byte-identical, while submitted bare CR is
normalized to LF.

**Implementation (Green).** Implement the authoritative signature:

```rust
pub struct SlideBodyRewrite {
    pub source: String,
    pub key: SlideKey,
    pub body: String,
}

pub fn rewrite_slide_body(
    source: &str,
    target: &ParsedSlide,
    new_body: &str,
    highlighter: &Highlighter,
) -> Result<SlideBodyRewrite>;
```

Call `strip_bom`, perform the deck-wide bare-CR refusal, then fresh-parse with
`parse_frontmatter` plus parse-only `parse_markdown`. Find the supplied target
by exact `source_span`, compare its key, source index, settings span, and note
spans against that fresh slide, retain the original edge blank runs, and build
the interior from the non-empty
parts only: the verbatim settings comment, normalized/edge-trimmed body, and
`canonical_comment(fresh.notes)`. Join those parts with exactly one blank line.
Define those runs by byte range, once: the leading run is
`source_span.start..first_nonblank_line.start`; the trailing run is
`last_nonblank_line.content_end..source_span.end`. The line content end is
immediately before its `\r\n` or `\n`, after any spaces or tabs on that
non-blank line. Its line terminator is therefore the first bytes of the
trailing run, followed by any whitespace-only lines. When more source follows
the slide and the trailing run contains no blank line (it is empty or only the
last line's terminator), emit one extra slide line ending before the trailing
run. This makes a blank line before the following separator part of the
canonical form without growing an existing blank run. For the canonical
fixture above the leading run is `"\n"` and the trailing run is `"\n\n"`, so
the exact expected output preserves both.

The settings span follows the parser convention pinned in Task 1 and includes
its terminal line ending. Strip only that one terminal `\r\n` or `\n` before
adding canonical separators. Use the slide's existing line ending and
`restore_bom` for `SlideBodyRewrite.source`. Target validation compares key,
source index, source span, settings span, and note spans with the fresh slide
before any splice. A mismatch refuses with help to reload the preview and
retry, rather than suggesting that the stale request can succeed unchanged.

Before either public function parses spans, removes comments, or splices
source, scan the whole BOM-stripped deck and refuse when any `\r` is not
followed by `\n`. Both use the same refusal: `bare CR line endings are not
supported by preview editing`, with help to convert the deck to LF or CRLF and
reload. This check is deliberately deck-wide because separator bytes can fall
outside a target span and malformed submitted Markdown can change which tail
bytes belong to the reparsed target. Submitted body text is still normalized
to LF, including submitted bare CR characters.

Lift, rather than copy, the comparison logic from `slide_edit.rs`:

```rust
pub(crate) enum SlideDifference {
    Key,
    KeySourceKind,
    LayoutRequest,
    Skip,
    PageNumber,
    Notes,
}

pub(crate) fn compare_except_key(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> std::result::Result<(), SlideDifference>;

pub(crate) fn compare_except_notes(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> std::result::Result<(), SlideDifference>;

pub(crate) fn compare_all(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> std::result::Result<(), SlideDifference>;

pub(crate) fn target_key_change_allowed(
    before: &ParsedSlide,
    after: &ParsedSlide,
) -> bool;
```

Keep the comparison helpers, difference enum, and diagnostic noun phrases in
the neutral `slide_compare.rs` module. The layout request, skip flag, and
page-number flag are compared in one internal core. `compare_except_key`
compares notes first, preserving `rewrite_block`'s existing diagnostic order,
then layout and flags. `compare_except_notes` compares key, key-source kind,
layout, and flags. `compare_all` adds notes to `compare_except_notes`. Note
saves use `compare_except_notes` for their target and `compare_all` for every
other slide. `target_key_change_allowed`
returns `before.key == after.key` or both `key_source` values are `Derived`;
there is no second spelling of that rule. Make `rewrite_block` consume those
same helpers. The slide-source validator checks slide count,
`settings().sections()`, every non-target slide, target
layout/skip/page-number/notes, byte-identical target settings-comment text and
matching settings-comment presence, the shared key rule, and
`slide_body(candidate, target_after)? == normalized_new_body`. It must not
compare fragment shape, editable spans, or reveal step count.

Parse the candidate exactly once. The accepted `target_after` from that parse
supplies both `SlideBodyRewrite.key` and `SlideBodyRewrite.body`; the latter is
the same `slide_body(candidate, target_after)?` value used by the round-trip
comparison. No caller reparses the accepted source or re-derives post-save
identity.

**Verification.**

```sh
cargo test -p peitho-core rewrite_slide_body_canonicalizes_settings_body_and_one_note
cargo test -p peitho-core rewrite_slide_body_accepts_structural_changes_and_preserves_source_conventions
cargo test -p peitho-core rewrite_block
test "$(rg -n '^pub\(crate\) fn compare_all' crates/peitho-core/src | wc -l | tr -d ' ')" -eq 1
test "$(rg -n '^pub\(crate\) fn target_key_change_allowed' crates/peitho-core/src | wc -l | tr -d ' ')" -eq 1
```

#### Task 4: Pin every refusal to the reparse comparison and prove idempotence

**Goal.** Make all hostile input fail only because parsing or the common
postcondition detects an observable change, and prove canonical rewriting
stabilizes after one pass.

**Files.**

- `crates/peitho-core/src/slide_source.rs`
- `crates/peitho-core/src/slide_compare.rs`

**Test (Red).** Add one table-driven
`rewrite_slide_body_refusals_fall_out_of_reparse` test with these rows:

| Case | Submitted body | Expected cause |
| --- | --- | --- |
| separator | `# New\n\n---\n\n# Extra` | slide count |
| plaintext comment | `# New\n\n<!-- stolen note -->` | target notes |
| JSON comment | `<!-- {"layout":"cover"} -->\n# New` | duplicate settings parse error or changed settings |
| settings added to settings-free deck | `<!-- {"skip":false} -->\n# T` | changed settings comment |
| unclosed fence | `` # New\n\n```rust\nlet x = 1; `` | notes or slide count |
| whitespace with metadata | ` \n\t\n` | accepted and idempotent when settings or notes keep the slide parseable |
| whitespace removes one of three slides | ` \n\t\n` | slide count |
| whitespace removes the only slide | ` \n\t\n` | `deck has no slides` parser error |
| unknown language | `` # New\n\n```not-installed\nx\n``` `` | parse error |
| bad slot | `::: {slot=}\ntext\n:::` | parse error |
| bad reveal | `::: {reveal=yes}\ntext\n:::` | parse error |
| bad emphasis | `` ```rust {0}\nx\n``` `` | parse error |

Run the original rows against the same three-slide deck whose target has page
settings and a speaker note, so separators, swallowed notes, and swallowed
following slides are observable. Add measured refusal rows for the body
round-trip comparison, changed sections, another slide's notes, and an explicit
target key, using focused decks where needed to reach each comparison. The test
calls only `rewrite_slide_body`; it must not invoke a pre-validator. Every
refused row goes through one `assert_refusal_cause` helper that checks its exact
expected message and `expected_line`. Structural postcondition refusals are
unnumbered, numbered parser diagnostics carry their measured combined-source
line, and the deck-wide `deck has no slides` parser error is unnumbered. This
helper pins the observed causes; it is not a mechanical special-casing detector.

Add acceptance rows for a separator inside a code fence, a comment inside a
code fence, a comment inside inline code, and an empty body retained by settings
or notes. Keep those counterexamples on the same shared metadata fixture as the
original refusal rows. These accepted inputs defeat naive input-pattern
prechecks. The refusal causes, counterexample acceptances, and review of the
production path together guarantee that there is no input-specific
pre-validation.

Add `rewrite_slide_body_refuses_submitted_leading_bom` for a submitted single
BOM and for submitted double BOMs on both LF and BOM-prefixed decks. The
settings-comment cases are panic regressions and must return ordinary structural
refusals; the case without settings pins the round-trip refusal behavior.

Add `rewrite_slide_body_is_parse_identity_preserving_and_idempotent`, which for
a corpus containing settings, scattered/inline notes, CRLF, BOM, explicit and
derived keys asserts: rewriting with `slide_body(before).unwrap()` preserves
sections and every slide's index/source index/key/key-source
kind/layout/skip/page-number/notes/step count/body, then rewriting the reparsed
result again yields byte-identical `SlideBodyRewrite.source`, with the same
returned key and body. The first result's key and body must equal the target
identity observed in the single candidate parse before the test reparses that
source for its second idempotence call. Source coordinates and diagnostic line
numbers are not identity because canonical comment movement can shift them.

**Implementation (Green).** Put every acceptance decision in one
`validate_reparsed_slide_body` path called after candidate parsing. Have it
return the accepted `(SlideKey, String)` from `target_after` rather than `()`;
`rewrite_slide_body` combines those values with the candidate source in
`SlideBodyRewrite` without another parse. Return the parser's line/message for
parse failures and a structural `BuildError` for a failed comparison. Do not
inspect `new_body` for separators, comments, fences, whitespace, languages,
slots, reveal syntax, or emphasis syntax; pin real refusal causes, keep the
counterexample acceptance rows, and verify this constraint in review.

Before parsing or validating the completed candidate, remove every leading
U+FEFF so it is in the same BOM-free coordinate space as parser spans and
validation slices. Restore exactly the original deck's BOM, if any, only when
building the accepted `SlideBodyRewrite.source`.

**Verification.**

```sh
cargo test -p peitho-core rewrite_slide_body_refusals_fall_out_of_reparse
cargo test -p peitho-core rewrite_slide_body_is_parse_identity_preserving_and_idempotent
cargo test -p peitho-core slide_source::tests
cargo test -p peitho-core slide_edit::tests
```

### Task group 3: Save service, origin translation, and HTTP outcome

#### Task 5: Save fresh source and pin included first/middle/last slides

**Goal.** Re-read, reparse, drift-check, rewrite, and atomically write the
correct origin file while reusing the existing slide-span translation scope.

**Files.**

- `crates/peitho/src/main.rs`

**Test (Red).** Add direct service tests for key-gone 409, body-drift 409 with
the exact existing message `the deck changed on disk; reload and retry`,
an untranslatable/mixed-origin slide returning 409 with its file name,
structural 422, a parse-valid layout-arity violation that is still written, a
skipped-slide success, BOM, pure CRLF, and a `code_images` command that must
not execute. Before any TypeScript task, add this table-driven include
regression:

```rust
let included_source = concat!(
    "# Included first\n\n---\n\n",
    "# Included middle\n\n---\n\n",
    "# Included last\n",
);
for (old_key, old_body, new_body, response_key) in [
    ("included-first", "# Included first", "# Revised first\n\n- added", "revised-first"),
    ("included-middle", "# Included middle", "# Revised middle\n\n- added", "revised-middle"),
    ("included-last", "# Included last", "# Revised last\n\n- added", "revised-last"),
] {
    let (dir, deck, included, top_source) =
        include_deck_fixture(TOP_SOURCE, included_source);
    let key = SlideKey::new(old_key).unwrap();
    let (result_key, result_body) =
        write_preview_slide_source(&deck, &key, old_body, new_body).unwrap();
    assert_eq!(result_key.as_str(), response_key);
    assert_eq!(result_body, new_body);
    assert_eq!(fs::read_to_string(&deck).unwrap(), top_source);
    assert_eq!(
        fs::read_to_string(&included).unwrap(),
        included_source.replacen(old_body, new_body, 1),
    );
    drop(dir);
}
```

The local `include_deck_fixture` writes this exact top-level source and returns
the four values used above:

```rust
const TOP_SOURCE: &str = "<!-- {\"include\":\"included.md\"} -->\n\n---\n\n# Top\n";

fn include_deck_fixture(
    top_source: &'static str,
    included_source: &str,
) -> (tempfile::TempDir, PathBuf, PathBuf, &'static str) {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let included = dir.path().join("included.md");
    fs::write(&deck, top_source).unwrap();
    fs::write(&included, included_source).unwrap();
    (dir, deck, included, top_source)
}
```

Name the test
`write_preview_slide_source_writes_first_middle_and_last_included_slides` and
assert every non-target include byte is unchanged in each row. The exact
`replacen` expectation agrees with Task 3's byte-range definition: each
`old_body` stops before its heading's line terminator, while each `new_body` has
no terminal line ending, so the original terminator and following blank lines
remain the trailing run. The first and middle spans retain `"\n\n"` after their
last non-blank line; the last retains `"\n"`. Any blank line at a middle/last
span's start remains in its leading run.

The fixture keys are not assumptions: `derive_key_from_fragments` at
`parser.rs:4118-4135` lowercases the first heading, turns non-ASCII-alphanumeric
runs into one `-`, and trims empty pieces. Thus `# Included first`,
`# Included middle`, and `# Included last` derive exactly `included-first`,
`included-middle`, and `included-last`; keep those fixture keys and assert them
through the service results above.

Add regressions for the clipped trailing edge shared by all preview writes. A
note save on the last included slide writes `a`, then `b`, then `c` without
copying the top-level separator's synthetic blank line into the include file;
an include origin that ended in LF keeps one LF terminator, and a pure-CRLF
origin keeps one CRLF terminator. A one-slide include pins the same behavior
for both source and note saves across a second save. The tight-separator source
fixture expects inserted blank lines before separators that are inside the
include file, but its last slide keeps only its origin terminator because the
following top-level separator boundary is synthetic.

Also cover an unterminated one-slide include with tight and loose top-level
separators in LF and pure CRLF decks. Adding, replacing, deleting, and adding a
note again must remain stable with one final line ending; a source save must
terminate the include and leave a following note save editable. Pin that the
last content line's trailing spaces survive by deleting the note from a last
included slide containing `"# I1\n\ntext  \n\n<!-- n -->\n"` and expecting
exactly `"# I1\n\ntext  \n"`.

Call `write_preview_origin_rewrite` directly after replacing one clipped
synthetic LF with a non-newline byte. Assert that `translate_span` rejects the
scope and the writer maps that untranslatable scope to a 409 naming the origin
file, with both files unchanged. Pin the mixed-origin slide-scope refusal at
this writer too: the service only supplies parsed slide spans, while the shared
writer owns the origin-translation boundary.

**Implementation (Green).** Add:

```rust
fn write_preview_slide_source(
    input: &Path,
    key: &SlideKey,
    old: &str,
    new: &str,
) -> Result<(SlideKey, String), server::DeckWriteError>;
```

Generalize the existing test helper to
`fn assert_preview_deck_drift<T>(result: Result<T,
server::DeckWriteError>)`, so inline and whole-slide services assert the same
409 without two helper implementations.

Its order is fixed: `load_and_expand_deck_source` -> resolve highlighter ->
parse-only `parse_deck` -> key lookup -> call
`peitho_core::slide_source::slide_body(combined, slide)`, map its error to
`server::DeckWriteError::Unprocessable` with `plain_diagnostic_text`, and
compare the returned body with `old` -> `rewrite_slide_body` -> destructure its
accepted `{ source, key, body }` ->
`write_preview_origin_rewrite` with `source` -> return `(key, body)`.
The service must neither parse `source` again nor look up the rewritten target
by index: the one candidate reparse inside `rewrite_slide_body` already proved
and returned both pieces of post-save identity.
Map missing keys and drift to 409, core refusals to 422, and I/O to 500.
Return immediately after the origin write; do not invoke a build, generation
swap, or sync notification because the watcher is the sole rebuild trigger.
Lift the missing-key conflict construction next to the other preview-deck error
helpers and use it from every service that performs the same key lookup.

Rename `PreviewOriginRewriteScope::NoteSlide` to `Slide`; use it for notes and
whole-slide source, leave `EditableBlock` intact, and make the `Slide` conflict
copy refer to editing the slide in the named file. `source_span` matches both
variants and `requires_whole_translation` remains `false` for `Slide`, `true`
for `EditableBlock`.

Fix clipped-edge mapping once in `write_preview_origin_rewrite`.
`translate_span` guarantees that clipped edge units are synthetic single LFs;
a non-newline synthetic byte makes the scope untranslatable before mapping.
For a slide-scope write, the rewritten scope must still start with the identical
clipped head or the writer returns the existing span conflict naming the origin
file. When the tail is clipped, remove the clipped head from the rewritten
scope, keep through the content end of its last non-blank line (including that
line's trailing spaces), then append the origin range's own trailing run
measured from its last non-blank line. If that run has no line terminator,
append CRLF for a pure-CRLF origin file and LF otherwise. Apply the existing
pure-CRLF conversion afterward. When no edge is clipped, keep the pre-task
write byte-identical; `EditableBlock` still requires a whole-span translation.

Do not infer a rewritten tail by counting backward from the rewritten scope's
end. Note append reshapes the trailing blank run, so that inference both copied
the top-level boundary newline into an included file on main and made the
interim clipped-edge fix permanently reject unterminated includes. The mapped
write may differ from the validated candidate only in blank lines immediately
before a following separator, which cannot change parsing.

**Verification.**

```sh
cargo test -p peitho --bin peitho write_preview_slide_source
cargo test -p peitho --bin peitho write_preview_note_does_not_leak_a_clipped_include_tail
cargo test -p peitho --bin peitho write_preview_note_preserves_a_crlf_terminator_for_a_clipped_include_tail
cargo test -p peitho --bin peitho write_preview_note_writes_unterminated_includes_across_separator_and_line_endings
cargo test -p peitho --bin peitho write_preview_slide_source_then_note_writes_an_unterminated_include
cargo test -p peitho --bin peitho preview_slide_and_note_saves_do_not_leak_a_single_slide_include_tail
cargo test -p peitho --bin peitho write_preview_note_preserves_trailing_spaces_on_the_last_content_line
cargo test -p peitho --bin peitho write_preview_origin_rewrite_maps_a_non_newline_synthetic_byte_to_scope_conflict
cargo test -p peitho --bin peitho write_preview_origin_rewrite_rejects_a_mixed_origin_slide_scope
rg -n 'PreviewOriginRewriteScope::Slide' crates/peitho/src/main.rs
! rg -n 'PreviewOriginRewriteScope::NoteSlide' crates/peitho/src/main.rs
```

#### Task 6: Add `/slide-source` with operation-typed writer results

**Goal.** Serialize all three Markdown write paths under the existing mutex,
return the post-save key and body for source edits, and preserve the two old
success responses byte for byte. Make the request/response pairing part of the
writer type: no consumer can return a note response for a source write, or a
source response for a note or inline write.

**Files.**

- `crates/peitho/src/server.rs`
- `crates/peitho/src/main.rs`

**Test (Red).** Add
`slide_source_route_without_deck_writer_returns_404_before_parsing`,
`slide_source_route_rejects_malformed_missing_and_unknown_fields`, and
`slide_source_route_passes_exact_request_and_returns_response_identity`. They
prove:

```rust
let absent = PresentServer::bind(PathBuf::new(), 0, "present.html").unwrap();
let response = http_request_with_content_type(
    &absent,
    "POST",
    "/slide-source",
    "{",
    Some("text/plain"),
);
assert_eq!((response.status, response.body.as_str()), (404, "404\n"));

let writer = RecordingDeckWriter::default().with_slide_source_result(Ok(
    SlideSourceSaved {
        key: SlideKey::new("new-derived-key").unwrap(),
        body: "# New".to_owned(),
    },
));
let calls = writer.calls();
let server = deck_write_server(writer);
let response = json_http_request(
    &server,
    "POST",
    "/slide-source",
    r##"{"key":"old","old":"# Old","new":"# New","extra":true}"##,
);
assert_eq!((response.status, response.body.as_str()), (400, "invalid slide source body\n"));

let valid_body = r##"{"key":"old","old":"# Old","new":"# New"}"##;
let saved = json_http_request(&server, "POST", "/slide-source", valid_body);
assert_eq!(
    (saved.status, saved.body.as_str()),
    (200, r##"{"key":"new-derived-key","body":"# New"}"##),
);
assert_eq!(
    calls.lock().unwrap().as_slice(),
    [RecordedDeckWrite::SlideSource {
        key: SlideKey::new("old").unwrap(),
        old: "# Old".to_owned(),
        new: "# New".to_owned(),
    }],
);
```

The invalid-request test also pins missing and wrong content types to
`"invalid slide source content type\n"`, missing fields and malformed keys to
`"invalid slide source body\n"`, and zero writer invocations for every 400.

Keep the existing `notes_route_saves_with_writer` and
`slide_edit_route_passes_exact_request_to_deck_writer` assertions exactly
`{"saved":true}` after their typed writer methods return `()`.

Use one `RecordingDeckWriter` in the server test module for every route test.
It records a test-only `RecordedDeckWrite`, returns independently configured
results for each operation, and can measure concurrent entry. Capture the
exact source `key`, `old`, and `new`, add
`slide_source_route_maps_conflict_unprocessable_and_io`, and rename the
expanded mutex regression to
`notes_slide_edits_and_slide_sources_share_one_deck_writer_mutex` so Note,
SlideEdit, and SlideSource can never execute concurrently.

**Implementation (Green).** Replace the request enum, outcome enum, and closure
alias with a per-operation surface whose method signatures enforce the
response shape:

```rust
pub struct SlideEditWrite {
    pub key: SlideKey,
    pub start: usize,
    pub end: usize,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SlideSourceSaved {
    pub key: SlideKey,
    pub body: String,
}

pub trait DeckWriter: Send {
    fn note(&mut self, key: SlideKey, text: String) -> Result<(), DeckWriteError>;
    fn slide_edit(&mut self, edit: SlideEditWrite) -> Result<(), DeckWriteError>;
    fn slide_source(
        &mut self,
        key: SlideKey,
        old: String,
        new: String,
    ) -> Result<SlideSourceSaved, DeckWriteError>;
}
```

Add deny-unknown-fields `SlideSourceRequest`, `DeckWriteRoute::SlideSource`,
and the `/slide-source` route arm. `DeckWriteRoute` parses each request and
selects both its trait call and its response serializer in one place. Map only
the `()` returned by `note` and `slide_edit` to the existing
`{"saved":true}` bytes. Serialize only `SlideSourceSaved` as
`{"key":"...","body":"..."}` through serde; its declared field order pins
the exact response without JSON-map ordering. Keep a single
`Arc<Mutex<Box<dyn DeckWriter>>>`, so content-type, body-parse, 404-before-
parsing, error mapping, and mutual exclusion remain in the shared handler.

Make `PreviewDeckWriter` a small struct holding the input path and implement
the trait by delegating to the three existing `write_preview_*` services. Map
the source service's accepted `(key, body)` tuple to `SlideSourceSaved`; note
and inline methods return their existing `()` directly. The type signatures,
not a convention checked by each consumer, now make mismatched successful
responses unrepresentable.

**Verification.**

```sh
cargo test -p peitho slide_source_route
cargo test -p peitho notes_route_saves_with_writer
cargo test -p peitho slide_edit_route_passes_exact_request_to_deck_writer
cargo test -p peitho notes_slide_edits_and_slide_sources_share_one_deck_writer_mutex
cargo test -p peitho --bin peitho preview_deck_writer
```

### Task group 4: Preview-only source contract and contamination boundary

#### Task 7: Generate `SlideSources` and emit it from the generation parse

**Goal.** Deliver every editable surviving slide's normalized body to preview
with a Rust-owned contract, using the same `slide_body` function as the
save-time drift guard.

**Files.**

- `crates/peitho-core/src/slide_source.rs` (including `slide_count_refusal()`
  help wording)
- `crates/peitho-core/src/lib.rs`
- `crates/peitho/src/main.rs`
- `bindings/SlideSources.ts`
- `packages/peitho-present/test/generated.test.ts`

**Test (Red).** Add
`slide_sources_json_uses_slide_body_for_every_editable_surviving_slide` and
`exports_slide_sources_binding_as_keyed_record` for deterministic JSON and
binding export:

```rust
let sources = SlideSources::from_slides(source, deck.parsed_slides());
assert_eq!(
    slide_sources_json(&sources).unwrap(),
    "{\n  \"version\": 1,\n  \"sources\": {\n    \"intro\": \"# Title\\n\\nBody\"\n  },\n  \"unavailable\": {}\n}\n"
);
assert!(generated_binding.contains("sources: Record<string, string>"));
assert!(generated_binding.contains("unavailable: Record<string, string>"));
```

Extend `emit_preview_cache_writes_preview_only_files_in_generation_dir` to
parse `sources.json` and assert the body excludes its settings comment and all
note comments. A core map test includes a skipped slide and a draft slide and
asserts the skipped key is present while the parser-dropped draft key is
absent. The include/draft/skip integration fixture also asserts that
`unavailable` is empty and pins the included and post-include body bytes, so
using the unexpanded top-level file cannot satisfy the key-set assertion by
misclassifying slides. Add a deck containing a lone CR; assert `sources` is
empty, every surviving slide key is present in `unavailable` with the bare-CR
message and help, and both the build and preview emission succeed. At unit
level, call `from_slides` with one genuine slide and one foreign `ParsedSlide`
parsed from a different source; assert the genuine key is in `sources`, the
foreign key is in `unavailable` with the span-mismatch reason, and each key is
in exactly one map. Add an Issue #584 deck with two comments on one line;
assert its slide is in `sources`, `unavailable` is empty, and the stored body's
exact bytes match `slide_body`. Add
`build_artifacts_compute_slide_sources_for_both_annotation_modes`, building the
same lone-CR deck once with `EditAnnotations::Off` and once with
`EditAnnotations::On`, and assert both non-optional
`artifacts.slide_sources_json` strings are equal. Add a
TypeScript compile fixture:

```ts
import type { SlideSources } from "../../../bindings/SlideSources";

const sources: SlideSources = {
  version: 1,
  sources: { intro: "# Title" },
  unavailable: {},
};
expect(sources.sources.intro).toBe("# Title");
```

Update the tests that pin `slide_count_refusal()`'s help. Its one-sentence
guidance must cover all measured causes: a `---` line in the body, an unclosed
code fence swallowing the next separator, and emptying a slide when it has no
settings comment or note.

**Implementation (Green).** Define and export:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
pub struct SlideSources {
    version: u8,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "Record<string, string>"))]
    sources: BTreeMap<SlideKey, String>,
    #[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "Record<string, string>"))]
    unavailable: BTreeMap<SlideKey, String>,
}

impl SlideSources {
    pub fn from_slides(source: &str, slides: &[ParsedSlide]) -> Self;
}

pub fn slide_sources_json(sources: &SlideSources) -> Result<String>;
```

`from_slides` calls `slide_body` for every surviving parsed slide. It inserts
successful bodies into `sources`; for each refusal it inserts the key and
`BuildError::to_string()` into `unavailable`, preserving both the message and
help. A refused slide must not fail a build that otherwise succeeds and must
never disappear silently. The only refusal produced by a real parsed deck
today is the deck-wide bare-CR refusal, so in practice every surviving slide
of such a deck is unavailable; the per-slide match also preserves an honest
partition if a parsed slide does not belong to the supplied source. The JSON
shape is always `{"version":1,"sources":{…},"unavailable":{…}}`. Task 11
surfaces the recorded reason when the author requests source editing. In
`build_artifacts_with_services`, construct the map from `loaded.source` and
the same Parsed deck immediately before mapping consumes it. Add
`slide_sources_json: String` to `BuildArtifacts` and populate it on every build,
independent of `EditAnnotations`. Write that string only inside
`emit_preview_cache_generation`, by direct field access with no `Option`,
`unwrap`, `expect`, or missing-source branch. Present-cache, ordinary-build,
PDF, lint, and publish emitters never write it. Generate and commit
`bindings/SlideSources.ts` through the ts-rs export test.

Fold the `slide_count_refusal()` wording fix into this task: tell the author to
remove a `---` separator, close an unclosed code fence, or keep body content
when the slide has neither a settings comment nor a note. The older wording
omitted the fence case and overstated when removing all content removes a
slide.

**Verification.**

```sh
cargo test -p peitho-core slide_sources
cargo test -p peitho --bin peitho build_artifacts_compute_slide_sources_for_both_annotation_modes
cargo test -p peitho --bin peitho emit_preview_cache_writes_preview_only_files_in_generation_dir
cd packages/peitho-present && npm test -- test/generated.test.ts && npm run typecheck
rg -n 'slide_body\(source, slide\)' crates/peitho-core/src/slide_source.rs
rg -n 'slide_source::slide_body\(combined_source, slide\)' crates/peitho/src/main.rs
```

#### Task 8: Enforce the cache and publish boundary for `sources.json`

**Goal.** Make preview generation directories the only emitted location for
body source and reject accidental copies in `dist/`.

**Files.**

- `crates/peitho/src/main.rs`
- `crates/peitho/src/lint.rs` (negative test for the lint workspace emitter)
- `crates/peitho/tests/build.rs`
- `crates/peitho/tests/publish.rs`

**Test (Red).** Extend
`build_still_writes_distribution_after_pipeline_refactor` with
`assert!(!out.join("sources.json").exists())`, add
`emit_present_cache_omits_sources_json`, and add:

```rust
#[test]
fn publish_rejects_slide_sources_file() {
    let dir = tempdir().unwrap();
    let dist = dir.path().join("dist");
    write_valid_dist(&dist);
    fs::write(
        dist.join("sources.json"),
        r#"{"version":1,"sources":{},"unavailable":{}}"#,
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["publish", "--dist"])
        .arg(&dist)
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "distribution contains presentation-only file: sources.json",
        ));
}
```

**Implementation (Green).** Add exactly `"sources.json"` to
`PRESENTATION_ONLY_DIST_FILES`. Do not add a source writer to
`emit_present_cache`, ordinary build/watch emission, PDF/lint workspaces, or
published output.

**Verification.**

```sh
cargo test -p peitho --test build build_still_writes_distribution_after_pipeline_refactor
cargo test -p peitho --test publish publish_rejects_slide_sources_file
cargo test -p peitho --bin peitho emit_present_cache_omits_sources_json
cargo test -p peitho --bin peitho emit_pdf_workspace_omits_sources_json
cargo test -p peitho --bin peitho emit_lint_workspace_omits_sources_json
```

### Task group 5: Shell request, editor, and closed coordination state

#### Task 9: Load the generated source contract and emit the `e` request

**Goal.** Make keyboard handling request source editing without owning any DOM
or save behavior, and load the preview-only source map with the generation.

**Files.**

- `packages/peitho-present/src/keyboard.ts`
- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`
- `packages/peitho-present/dist/preview.js`

**Test (Red).** Add `preview_keyboard_only_emits_source_edit_request`:
lowercase unshifted `e` dispatches once and is prevented;
uppercase/Shift+E, Cmd/Ctrl/Alt+E, IME composition, keyCode 229,
notes/input/select/contenteditable targets, and an inline editor dispatch
nothing. Assert the keyboard handler performs no fetch or DOM mutation. Extend
the existing `handshakes sync generation before fetching preview content`
test so the successful sequence is `/sync`,
`manifest.json`, `notes.json`, `sources.json`, CSS, and slide fragments, and a
non-2xx `sources.json` or a JSON body that does not match `SlideSources` leaves
the shell in its existing load-error state.

**Implementation (Green).** Import the generated `SlideSources` type, store:

```ts
private sources: SlideSources = { version: 1, sources: {}, unavailable: {} };
```

Give `PreviewFetchFixture` a cloned `SlideSources` value and a
`sources.json` response, and add that response to each ad hoc successful-load
mock in `preview.test.ts`. Fetch `sources.json` during `load`, validate its
number `version` and non-array, string-valued `sources` and `unavailable`
records, and route an invalid shape through the existing load-error path. In
`installPreviewKeyboard`, after the
existing chord/composition/editable-target gates, map only
`event.key === "e" && !event.shiftKey` to `preventDefault()` plus:

```ts
bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
```

Move the existing `isComposingKey` implementation from `preview.ts` to the
shared keyboard module as
`export function isComposingKey(event: Pick<KeyboardEvent, "isComposing" |
"keyCode">): boolean`, next to `hasChordModifier`; Task 10 imports that exact
predicate. No keyboard code may open an editor, inspect the active slide, or
call the network. Because `keyboard.ts` is shared by multiple entry graphs,
**every shell task** rebuilds all entries and commits every bundle whose bytes
change; the bundle drift gate runs per commit, so source ownership alone does
not determine which generated files belong to a task.

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'preview_keyboard_only_emits_source_edit_request'
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'handshakes sync generation before fetching preview content'
cd packages/peitho-present && npm run typecheck
```

#### Task 10: Build the isolated textarea and request state machine

**Goal.** Put all source-editor DOM, local keyboard behavior, normalization,
HTTP state, and teardown in the new module.

**Files.**

- `packages/peitho-present/src/previewSourceEdit.ts`
- `packages/peitho-present/test/previewSourceEdit.test.ts`

**Test (Red).** Unit-test this complete matrix:

- The textarea is a light-DOM child of the stage tile, outside the slide
  shadow root; it has `data-peitho-preview="source"`, monospace styling,
  `tab-size: 2`, `spellcheck=false`, the supplied body, focus, and selection
  `0..0`; the slide host is hidden.
- `setFrame` writes the fitted stage rectangle without applying a scale
  transform.
- Plain Enter is untouched. Composing Enter/Escape does nothing. Escape asks
  the controller to cancel. Cmd+Enter and Ctrl+Enter ask it to commit. Blur
  asks it to commit once.
- Edge blank lines and CRLF feed only the unchanged shortcut: when
  `normalizeSourceBody(textarea.value) === old`, close without POST. A dirty
  value posts exactly `{key,old,new:textarea.value}` to `/slide-source`; it does
  not replace `new` with the TypeScript-normalized string.
- During the request the textarea is `readOnly` and Cmd/Ctrl+Enter/Escape are
  swallowed. Success requires string `key` and `body` response fields and
  returns that server body byte-for-byte; a missing or non-string field is a
  failed response. 409/422/500 JSON and thrown fetches keep the draft, unlock,
  refocus, and return the displayed message.
- In a focused contract row, type a value whose TypeScript normalization is
  `"# Typed"`, mock `{"key":"renamed","body":"# Server canonical"}`, and
  assert `saved.body === "# Server canonical"`, not `"# Typed"`.
- Cancel, unchanged close, successful close, and `destroy` remove every local
  listener/node and restore the host's prior `hidden` value.

**Implementation (Green).** Export these concrete boundaries:

```ts
export type PreviewSourceEditFrame = {
  left: number;
  top: number;
  width: number;
  height: number;
};

export type PreviewSourceEditCommitResult =
  | { status: "saved"; previousKey: string; key: string; body: string }
  | { status: "unchanged"; key: string; body: string }
  | { status: "failed"; message: string };

export type PreviewSourceEdit = {
  readonly textarea: HTMLTextAreaElement;
  commit(): Promise<PreviewSourceEditCommitResult>;
  cancel(): void;
  setFrame(frame: PreviewSourceEditFrame): void;
  destroy(): void;
};

export function openPreviewSourceEdit(options: {
  document: Document;
  fetcher: typeof fetch;
  tile: HTMLElement;
  host: HTMLElement;
  key: string;
  body: string;
  onCommitRequest(): void;
  onCancelRequest(): void;
}): PreviewSourceEdit;
```

Use the internal LF/edge-blank normalization function only for the
unchanged-without-POST comparison. Send the textarea's current value as `new`.
For an unchanged result return the existing `old` body; for a saved result use
`response.body` verbatim and never the TypeScript-normalized draft. Serialize
concurrent commits through one stored promise. The module does not know preview
mode, navigation, notes, reloads, manifests, or session storage.

```ts
function normalizeSourceBody(value: string): string {
  const lines = value.replace(/\r\n?/g, "\n").split("\n");
  while (lines[0]?.trim() === "") lines.shift();
  while (lines.at(-1)?.trim() === "") lines.pop();
  return lines.join("\n");
}
```

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/previewSourceEdit.test.ts
cd packages/peitho-present && npm run typecheck
```

#### Task 11: Replace parallel edit flags with one closed `ActiveEdit` union

**Goal.** Open only one inline or source editor and update the source identity
after a derived-key save without mutating stale manifest/note identity.

**Files.**

- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (Red).** Add
`source_edit_request_obeys_single_mode_and_one_edit_union` and
`source_edit_success_uses_server_identity_for_the_next_save`. Prove the request
is ignored in grid, while a transition settles, and while either edit kind is
open; it opens only the current single-mode slide; source-open blocks inline-
click start and an inline edit blocks source-open; the textarea replaces the
fitted stage and resize updates its frame; Escape restores the stale rendered
host. Using the recorded deck-wide bare-CR refusal as the
`sources.unavailable` fixture, assert that no editor opens and the
`slide-source` status channel shows that recorded reason, including its
actionable help. For a first draft whose
TypeScript-normalized text is `"# Typed"`, return
`{key:"renamed",body:"# Server canonical"}`. Assert the map moves from the old
key to `renamed`, stores the response body verbatim, the source target uses
`renamed`, and a second `e` opens with `"# Server canonical"`. Change that
second draft, save again, and assert the second POST carries `key:"renamed"`
and `old:"# Server canonical"`, not the first draft or its TypeScript-normalized
form. Notes still use the manifest's old key until a successful rebuild.

**Implementation (Green).** Replace `activeSlideEdit` with exactly one closed
state and one generic predicate:

```ts
type ActiveEdit =
  | { kind: "inline"; edit: ActiveSlideEdit }
  | { kind: "source"; edit: PreviewSourceEdit; view: PreviewSlideView };

private activeEdit: ActiveEdit | null = null;

private isEditOpen(): boolean {
  return this.activeEdit !== null;
}
```

Add `sourceKey: string` to `PreviewSlideView`, initialized from `meta.key`, and
listen for `peitho:sourceeditrequest` in the controller. Every former generic
`activeSlideEdit !== null` guard must call `isEditOpen()`; inline-only code
narrows the union by `kind`. Opening first checks
`sources.unavailable[view.sourceKey]` and reports that recorded reason through
the `slide-source` status channel while leaving the rendered host in place.
Otherwise it obtains `sources.sources[view.sourceKey]` and delegates to
`openPreviewSourceEdit`. On keyed success, delete the old map entry, write
`sources.sources[result.key] = result.body`, and set
`view.sourceKey = result.key`; `result.body` is the unmodified server response,
not a locally normalized draft. Deliberately leave `view.meta.key`, the notes
map, and rendered HTML unchanged. Add `"slide-source"` to `PanelStatusSource`
and its stable display order after notes and inline edit. In
`applySingleLayout`, keep the active source view's host hidden and call
`edit.setFrame` with the fitted slide's left/top and scaled width/height;
ordinary active slides continue through `applyHostFrame` unchanged.

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'source_edit_request_obeys_single_mode_and_one_edit_union'
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'source_edit_success_uses_server_identity_for_the_next_save'
! rg -n 'activeSlideEdit' packages/peitho-present/src/preview.ts
test "$(rg -n 'private isEditOpen\(' packages/peitho-present/src/preview.ts | wc -l | tr -d ' ')" -eq 1
```

#### Task 12: Make existing transition and reload rules cover source edits

**Goal.** Settle the union before notes, block movement on failure, and release
one deferred reload on every successful way out.

**Files.**

- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (Red).** Add
`source_edit_transition_commits_before_notes_and_navigation`,
`source_edit_failure_blocks_transition`,
`source_edit_reload_releases_once`, and
`source_edit_derived_key_can_recover_after_build_failure`. Add the table-driven
`stale_source_map_after_inline_or_external_failed_rebuild_preserves_draft_on_409`
for the two stale-map causes below. Together they cover these sequences:

1. Cmd/Ctrl+Enter and blur save; unchanged close performs no POST; Escape
   cancels without POST.
2. PageUp/PageDown from the textarea call `commitTransition`, await source
   success, then flush notes, then navigate. Other editable-target navigation
   keys remain text behavior.
3. A source 409/422/500 keeps the textarea/draft open, reports only the
   `slide-source` status, and blocks slide change and grid entry.
4. A generation change during source edit or transition settlement is
   deferred. Saved, unchanged, and Escape closure release it exactly once;
   failure does not release it.
5. Clicking the notes textarea causes source blur/save first. A successful
   source save leaves the old rendered slide visible until reload.
6. A derived-key save followed by a check-phase build failure can receive a
   second source save under the response key from the stale page.
7. For the stale-map regression, start from a generation whose source entry is
   `"# Old"`. In one row, complete an inline edit and then deliver a build-error
   sync update without a generation bump; in the other, model an external file
   change with the same failed-build/no-generation-advance update. Open `e`,
   type `"# Draft must survive"`, and answer its stale `old:"# Old"` POST with
   the exact 409 drift error. In both rows the textarea stays open and writable
   with the draft unchanged, the `slide-source` status shows the 409, the source
   map remains `"# Old"`, no deferred reload is released, and the attempted
   transition does not proceed. This pins the accepted honest-conflict tradeoff
   without losing any editor text.
8. Source drafts never appear in the serialized `PreviewState`.

**Implementation (Green).** Replace the inline-only settlement call with one
`commitActiveEdit(): Promise<boolean>` that switches on `ActiveEdit.kind`.
Source success/unchanged closes the union, clears only source status, and may
release reload; the key/blur wrapper releases only when
`pendingTransitionSettlements === 0`, while a transition releases after its
commit. Source failure returns `false` without closing. Keep
`settleForTransition` ordered as active edit first, then the existing
`notesSettled` loop. Make `requestGenerationReload`, the transition fast path,
the failed-settlement release branch, `applySingleLayout`, and `destroy`
consume the union and `isEditOpen()` instead of mode-specific flags. Do not
add source text to `PreviewState` or `saveState`.

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'source_edit_transition_commits_before_notes_and_navigation'
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'source_edit_failure_blocks_transition'
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'source_edit_reload_releases_once'
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'source_edit_derived_key_can_recover_after_build_failure'
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'stale_source_map_after_inline_or_external_failed_rebuild_preserves_draft_on_409'
cd packages/peitho-present && npm run typecheck
```

#### Task 13: Cover page exit, teardown, and the committed bundles

**Goal.** Preserve dirty source on page exit within Chrome's keepalive limit,
clean up every listener, and commit every generated entry bundle changed by
the shared TypeScript graph.

**Files.**

- `packages/peitho-present/src/previewSourceEdit.ts`
- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/previewSourceEdit.test.ts`
- `packages/peitho-present/test/preview.test.ts`
- Whichever bundles change under `packages/peitho-present/dist/`
  (`shell.js`, `preview.js`, and/or `remote.js`)

**Test (Red).** Assert `pagehide` calls `saveForPageHide` only for a dirty
source edit, repeats an in-flight normal save as unload insurance, sends
`keepalive:true` when the UTF-8 encoded JSON request is at most 60,000 bytes,
and sends `keepalive:false` above that threshold. Assert this path uses the
server-returned `old` and the textarea value verbatim as `new`; normalization
only decides whether the edit is clean. Clean, cancelled, and absent source
edits make no source POST. Assert controller `destroy` removes its
source-request, textarea key/blur, resize, pagehide, and tile listeners, while
the existing bootstrap cleanup still removes keyboard and sync listeners.
Late-resolving requests cannot mutate the destroyed shell or release a
discarded reload.

**Implementation (Green).** Extend `PreviewSourceEdit` with
`saveForPageHide(): void` and implement it with the same encoded request-size
rule as notes `doFlush`, but keep it in `previewSourceEdit.ts`; its
fire-and-forget request catches rejection because no UI can recover during
unload. Reuse the ordinary commit payload builder so pagehide also posts the
raw draft against the last server body. Dispatch
`onPageHide` through the `ActiveEdit` union before the existing notes flush.
Make source teardown idempotent and have controller destruction call the
active variant's cleanup. Before applying an asynchronous result, compare the
captured union member with `this.activeEdit`; destruction or replacement makes
the result inert. Rebuild all three entries with the existing esbuild script.
Rebuild and commit every bundle that changed; the drift gates compare against
the committed files. Moving `isComposingKey` into shared `keyboard.ts` may or
may not change `dist/shell.js` or `dist/remote.js`, so old-versus-new byte
identity is not an acceptance requirement.

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/previewSourceEdit.test.ts test/preview.test.ts && npm run typecheck
cd packages/peitho-present && npm run build
cargo test -p peitho --bin peitho builtin_preview_shell_matches_committed_bundle
cargo test -p peitho --bin peitho builtin_remote_shell_matches_committed_bundle
git diff --check -- packages/peitho-present/dist/shell.js packages/peitho-present/dist/preview.js packages/peitho-present/dist/remote.js
```

### Task group 6: Documentation and browser acceptance

#### Task 14: Document the third Markdown write path

**Goal.** Explain the whole-slide workflow and record the invariants that
future parser, server, and shell changes must preserve.

**Files.**

- `CLAUDE.md`
- `site/content/guide/cli.md`
- `README.md`

**Test (Red).** Before editing, these contract searches must fail:

```sh
rg -q 'Whole-slide Markdown source' CLAUDE.md
rg -q 'Press `e`.*body Markdown' site/content/guide/cli.md
rg -q 'Cmd/Ctrl\+Enter' site/content/guide/cli.md
rg -q 'stale source map.*409' site/content/guide/cli.md
rg -q 'whole slide.*`e`' README.md
```

**Implementation (Green).** Add one CLAUDE invariant bullet recording:
`slide_body` as the single body definition; settings/notes exclusion;
reparse-only refusals; shared comparison/removal/BOM helpers; parse-only
saves and the accepted reparse as the sole source of response key/body; the
three-method `DeckWriter` trait behind one mutex; `()` note/inline results and
the `SlideSourceSaved` source result so request/response pairing is a type, not
a convention; the unchanged origin writer; preview-only `sources.json`; the
shell's verbatim use of the server body; and the closed shell edit union.
Update CLAUDE's repository map and long-polling pitfall route inventories to
include `POST /slide-source`; start the invariant with
“Whole-slide Markdown source editing in preview”.

Add “Editing a whole slide in preview” to the CLI guide. State `e`, stage
replacement, caret-at-start, body-only contents, plain Enter newline,
Cmd/Ctrl+Enter/blur/transition save, Escape cancel, single-mode scope,
structural edits allowed inside one slide, slide/settings/note protections,
include-file writes, BOM/CRLF behavior, 409 drift recovery, 422 parse refusal,
last-good check-failure recovery, response-key handling, and the known stale
notes-key tradeoff. Also document the second known tradeoff explicitly: an
inline edit or external change followed by a failed rebuild leaves a stale
source map, so a source save returns an honest 409 while the open editor keeps
its draft and status until a successful generation reload. Use the exact
phrase “stale source map returns an honest 409” for the acceptance search. Add
a concise README preview paragraph distinguishing whole-slide `e` editing from
click-to-edit inline blocks. Include these exact sentences so the acceptance
searches remain stable:

```text
Press `e` in single mode to replace the rendered slide with its body Markdown.
Cmd/Ctrl+Enter or blur saves; plain Enter inserts a newline; Escape cancels.
Edit a whole slide with `e`; click a rendered text block for a smaller inline edit.
```

Use the first two sentences in the guide and the third in README.

**Verification.**

```sh
make demo-site
rg -n 'Whole-slide Markdown source' CLAUDE.md
rg -n 'Press `e`.*body Markdown|Cmd/Ctrl\+Enter|stale source map.*409' site/content/guide/cli.md
rg -n 'whole slide.*`e`' README.md
git diff --check -- CLAUDE.md site/content/guide/cli.md README.md
```

#### Task 15: Have Claude execute the required real-Chrome checklist

**Goal.** Validate real focus, layout, IME, watcher, stale-generation, and blur
behavior that jsdom cannot observe. This task is executed by Claude, not
Codex, because the Codex sandbox cannot launch Chrome.

**Files.** These fixtures are ignored and must not be committed:

- `target/preview-source-edit-e2e/deck.md`
- `target/preview-source-edit-e2e/included.md`
- `target/preview-source-edit-e2e/css/base.css`
- `target/preview-source-edit-e2e/css/overrides.css`

**Test (Red).** Prepare an include-backed slide with a settings comment, body,
speaker note, derived heading key, and keyed CSS. Confirm the browser session
initially lacks a whole-slide editor and record the initial include bytes.

**Implementation (Green).** Claude performs this checklist from the design's
Verification section, recording the observed result of every checkbox:

- [ ] Navigate to the preview tab, then click once only to focus the document;
      the next interaction is the step under test.
- [ ] Open the include-backed slide, press `e`, and visually confirm an
      unscaled textarea replaces the rendered stage, starts with its caret at
      byte zero, and contains body Markdown but no settings or note comment.
- [ ] Type with a real IME and confirm composition Enter neither commits nor
      inserts a shell action; then press Cmd+Enter and confirm the included
      Markdown file changes, the watcher rebuilds, and reload lands on the
      rendered current slide.
- [ ] Save parse-valid Markdown that fails check phase; confirm the last-good
      slide stays visible with the build-error banner, then press `e` and
      confirm the saved body reopens.
- [ ] Change the derived heading key while keyed CSS makes the rebuild fail;
      confirm another `e` edit posts under the returned key and can repair the
      body from the stale page.
- [ ] Open the source editor, change the body, click the notes textarea, and
      confirm blur commits the source before notes editing proceeds.

Run the fixture outside tracked paths:

```sh
test "$(git check-ignore target/preview-source-edit-e2e/deck.md target/preview-source-edit-e2e/included.md target/preview-source-edit-e2e/css/base.css target/preview-source-edit-e2e/css/overrides.css | wc -l | tr -d ' ')" -eq 4
# Terminal 1:
cargo run -p peitho -- preview target/preview-source-edit-e2e/deck.md --port 6175 --no-open
# Terminal 2 (macOS):
open -a "Google Chrome" http://localhost:6175/
git status --short -- target/preview-source-edit-e2e
```

**Verification.** Claude checks all six boxes, inspects the final
`included.md` bytes for settings -> body -> one canonical note ordering, and
confirms `git status --short` contains no tracked E2E artifact.

## Summary

<!-- derived-from #file-map -->
<!-- derived-from #tasks -->

The dependency order is parser settings provenance -> one core body
definition -> canonical rewrite and reparse proof -> unchanged origin writer
and key/body HTTP outcome from that same accepted reparse -> source JSON
computed by every build but emitted only to preview -> closed shell edit state
that trusts the returned body -> documentation -> Claude's real-Chrome
acceptance pass. There is one `slide_body`, one non-target comparison, one
derived-key rule, one family of removal/BOM helpers, one deck-writer mutex, and
one origin-write function.

## Gates (all must pass before committing)

```sh
cargo test --workspace
cargo test --workspace
cargo test --workspace          # three consecutive runs (past test-race incidents)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/  # contract drift
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
git diff --exit-code packages/peitho-present/dist/remote.js
```
