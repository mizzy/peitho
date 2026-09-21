# Whole-slide Markdown source editing in preview — design

Date: 2026-09-21
Status: design approved by the author in conversation; implementation plan in
`docs/plans/2026-09-21-preview-source-edit.md`.

## Goal

Inline editing (`docs/specs/2026-09-20-preview-inline-edit-design.md`) fixes
the text of one block and refuses every structural change. This adds the
complementary mode: in `peitho preview` single mode, press `e` and the rendered
slide is replaced by a textarea holding **the slide's body Markdown**. The
author can add a bullet, turn a paragraph into a list, add a code block, or
rewrite the slide; committing writes the Markdown file, the ordinary watch
rebuild runs, and the page reloads with the rendered result.

Markdown stays the single source of truth. This is a third write path into the
Markdown file next to `POST /notes` and `POST /slide-edit`, sharing their
mutex and their origin-write seam.

### Non-goals

- Live preview while typing. The editor replaces the slide; the result is seen
  after commit → rebuild → reload.
- Adding, removing, splitting, or reordering slides (`---` in the body is
  refused in v1).
- Editing speaker notes or page settings here. Notes have the notes panel;
  settings (`layout`, `key`, `section`, `time`, `skip`, `page_number`) are
  edited in the author's editor.
- Editing frontmatter, layout HTML, or CSS.
- Editing in grid mode, in `peitho present`, or in `dist/`.

## Author decisions (2026-09-21)

1. **The editor replaces the slide on the stage** (not side-by-side, not in the
   notes panel).
2. **Body only.** The editor text excludes speaker-note comments *and* the page
   settings JSON comment. The notes panel stays independently editable, so no
   state exists where two editors hold the same bytes.
3. **`e` starts**, **Cmd/Ctrl+Enter or blur commits**, **Escape cancels**
   (same cancel meaning as inline edit). Plain Enter is a newline.
4. **No slide add/remove in v1**: the slide count must not change.
5. **Unparseable source is refused** (422), not written: without a successful
   reparse there is no proof the other slides are intact.
6. Saving canonicalizes the slide to *settings comment → body → one note
   comment*. This is the same canonicalization `rewrite_note` already performs
   for notes.

7. A successful save answers with the slide's post-save key (proposed during
   design, approved by the author) — see §3 "Response".

## Approach

### 1. One definition of "body": `peitho_core::slide_source`

A new core module owns both directions, so the text shown in the browser and
the text compared at save time can never come from two implementations:

```rust
/// The slide's body: `source[slide.source_span]` with the page settings
/// comment and every note comment removed, then leading/trailing blank lines
/// trimmed. Line endings normalized to LF.
///
/// Refuses (like `rewrite_note`'s span check) when the slide's spans do not
/// belong to `source`, so a `ParsedSlide` from another parse is an error in
/// the caller, never a slicing panic in a server request thread.
pub fn slide_body(source: &str, slide: &ParsedSlide) -> Result<String>;

/// Full source with `target`'s body replaced, or a refusal. `key` and `body`
/// are read from the same reparse that accepted the candidate, so no caller
/// re-derives them.
pub fn rewrite_slide_body(
    source: &str,
    target: &ParsedSlide,
    new_body: &str,
    highlighter: &Highlighter,
) -> Result<SlideBodyRewrite>;

pub struct SlideBodyRewrite { pub source: String, pub key: SlideKey, pub body: String }
```

- The parser already threads the comment's `SourceSpan` into
  `process_html_chunk` and drops it on the settings branch. `ParsedSlide` gains
  `settings_span: Option<SourceSpan>` next to `note_spans`. It is parse-time
  bookkeeping like `note_spans`: it never reaches the manifest or `bindings/`.
- Comment removal is one function, `notes_edit::remove_comment_spans`, lifted
  out of `splice_note` and used by both (the reverse-order loop with its byte
  accounting exists once), for notes and for the settings comment alike. An
  inline note (`text <!-- n --> more`) removes exactly the comment bytes, so
  the body shows `text  more`; surrounding whitespace is untouched (measured in
  `removal_edit`: the inline range ends at the comment's end, not the line's).
- The parser enforces "page settings comment must appear before slide
  content", but `seen_content` only flips at the end of the first block, so an
  *inline* settings comment inside the slide's first paragraph or heading
  (`text <!-- {"key":"a"} -->`) is accepted (measured while reviewing Task 1).
  Re-emitting the comment first therefore moves it past a preceding note
  comment or out of that first block. Both are the same canonicalization, need
  no special case (`removal_edit` handles inline and whole-line spans), and the
  postcondition still proves the settings are unchanged.

`rewrite_slide_body` builds the replacement for the whole `source_span` as

```
<original leading blank run>
<settings comment, verbatim bytes>      (if any) + blank line
<new body, edge blank lines trimmed>
<blank line> + <canonical note comment> (if the slide has notes)
<original trailing blank run>
```

using the slide's existing line ending and `notes_edit::canonical_comment` for
the note (text = the fresh parse's `notes`, so multiple comments join exactly
as the parser joined them). Keeping the original edge blank runs means the
changed bytes stay interior to the span — the same profile as a note rewrite,
which is what lets included slides translate (§3).

### 2. Postcondition: refusals fall out of one comparison

No input is special-cased. The candidate is reparsed (`parse_frontmatter` +
`parse_markdown`, parse-only, so `code_images` never run) and accepted only
when:

- it parses (else 422 with the parser's message and line);
- slide count and `settings().sections()` are unchanged;
- every non-target slide passes `slide_edit::compare_non_target_slide` (key,
  key-source kind, layout request, skip, page-number flag, notes) — lifted to
  a shared `pub(crate)` location, not copied;
- the target's layout request, `skip`, `page_number_hidden`, and `notes` are
  unchanged;
- the key rule: the target key may change only when both key sources are
  `Derived` (same rule as `rewrite_block`, shared, not copied);
- `slide_body(candidate, target_after)? == normalized new body` (the
  round-trip proof that what was typed is what the parser sees as body).

What this refuses without any dedicated code:

| typed into the body | caught by |
|---|---|
| `---` | slide count |
| a non-JSON `<!-- comment -->` | target notes changed |
| a JSON settings comment | "duplicate page settings comment" parse error, or settings changed |
| an unclosed code fence that swallows the note comment / next slide | notes changed / slide count |
| only whitespace | slide count (blank ranges are not slides) or body round-trip |
| an unknown code language, bad `::: {slot=}` / `::: {reveal}` / emphasis spec | parse error |

Fragment shape, editable spans, and reveal step count are deliberately *not*
compared — changing them is the point of this mode. Check-phase failures
(slot arity, unknown slot for the layout) are not detectable at parse time;
they are written, fail the watch rebuild, and surface through the existing
`buildError` banner (§4).

### 3. Save path: `POST /slide-source`

Request `{key, old, new}` (`application/json`, `deny_unknown_fields`).

- `DeckWrite::SlideSource { key, old, new }` joins the closed enum; one route
  arm and one `DeckWriteRoute` entry. 404 without a `DeckWriter` (present),
  content-type and body checks, and the 409/422/500 mapping come from the
  shared handler unchanged.
- `write_preview_slide_source`: `load_and_expand_deck_source` → `parse_deck`
  → find the slide by `SlideKey` (gone → 409) → **drift guard**
  `slide_body(combined, slide)? == old` (a `slide_body` refusal cannot come
  from a fresh parse of the same source; it maps to 422 like every other core
  refusal, never to the drift 409), else the single existing 409 "the deck
  changed on disk; reload and retry" → `rewrite_slide_body` (refusal → 422)
  → `write_preview_origin_rewrite`.
- Origin scope: the rewrite replaces a slide span whose edge blank runs are
  preserved, exactly the shape `PreviewOriginRewriteScope::NoteSlide` exists
  for (synthetic edges may clip; changed bytes are interior). The variant is
  renamed `Slide` and used by both; no third scope. An include-file slide is
  written to its include file; a span that cannot translate is a 409 naming
  the file. **The plan must pin this with tests on the first, middle, and last
  slide of an included file before any shell work.**
- No rebuild is triggered by the write; the watcher remains the only trigger.

**Response.** Success answers `{"key":"<post-save key>","body":"<post-save
slide_body>"}`. The shell stores that `body` verbatim as its new `old`; it
never computes the canonical body itself, so a TypeScript normalizer that
disagrees with `slide_body` on some whitespace edge cannot turn the next save
into a false 409 (the shell's own normalization is only an "unchanged, skip
the POST" shortcut). Reason for the key: a body edit
commonly passes parse but fails check (one bullet too many for the layout).
The rebuild fails, the generation does not bump, the page keeps the last good
generation — and the author's next move is `e` again to fix it. If the edit
also changed a derived key, a second POST under the old key would be "key
gone" with no in-page way out. `DeckWriter` therefore returns
`Result<DeckWriteOutcome, DeckWriteError>`; `Note` and `SlideEdit` return the
unit outcome and keep their current empty-success bodies byte-identical.

### 4. Body text reaches the browser through the preview cache only

Each preview generation directory gains `sources.json`
(`{"version":1,"sources":{"<key>":"<body>"}}`, Rust-owned type exported with
ts-rs like `Notes`), written in `emit_preview_cache_generation` only — not the
present cache. `"sources.json"` joins `PRESENTATION_ONLY_DIST_FILES`, so
publish rejects it under `dist/`. The bodies are computed by `slide_body` from
the same parse that produced the generation. Every build computes the map
(it is a cheap string pass) and only the preview emitter writes it, so there
is no "preview artifacts without sources" state to handle.

### 5. Preview shell

- `installPreviewKeyboard` maps unshifted `e` to a new request event
  `peitho:sourceeditrequest` (§16: the keyboard only emits; the shell acts).
  The existing guards apply: chord modifiers, IME composing, and editable
  targets (so `e` typed into the notes textarea or an inline editor is text).
  The shell ignores the request in grid mode, while any edit is open, and
  while a transition is settling.
- Opening: the stage tile's slide host is hidden (`el.hidden`) and a
  shell-owned `<textarea data-peitho-preview="source">` fills the stage area.
  It lives **outside** the slide's shadow root: the host is
  transform-scaled (text would scale with it) and a shadow-root caret needs
  the `getComposedRanges` workaround (pitfall #543). Monospace, `tab-size: 2`,
  `spellcheck=false`; value = the shell's source map entry; focus with the
  caret at the start.
- One edit at a time **across both kinds**. The inline edit's guards
  (`activeSlideEdit !== null`) become a single derived "an edit is open"
  predicate over a closed union `ActiveEdit = inline | source`, so every
  existing rule covers the new mode by construction rather than by a second
  set of checks: transitions settle the open edit first, then notes; a failed
  save blocks the transition; a generation reload is deferred while an edit is
  open and released exactly once on successful commit, unchanged close, or
  Escape; no edit starts while a transition is settling; nothing enters
  `sessionStorage`.
- Commit (Cmd/Ctrl+Enter, blur, or a transition): unchanged text closes
  without a POST. Otherwise the textarea is `readOnly` and Cmd/Ctrl+Enter /
  Escape are swallowed while the POST is in flight. On success the shell sets
  `sources[response.key] = response.body`, re-keys its target, closes the
  editor, and shows the (stale) slide until the generation reload arrives. On
  failure the editor stays open with the draft and the error is shown through
  a third `PanelStatusSource`, `"slide-source"`.
- While typing, only PageUp/PageDown navigate (existing editable-target rule),
  and they go through `commitTransition`, so they commit first.
- `pagehide` posts a dirty source edit with `keepalive`, downgraded to a plain
  request above 60 KiB like `doFlush` (slide bodies with pasted code can exceed
  Chrome's 64 KiB keepalive cap; `sendSlideEdit` never could).
- The preview controller is already 1600 lines coordinating two channels. The
  source editor's DOM/state lives in its own module (`previewSourceEdit.ts`);
  `PreviewShellController` keeps only the `ActiveEdit` union and the
  transition/reload coordination.

## Files

- `crates/peitho-core/src/slide_source.rs` (new), `parser.rs` /`phase.rs`
  (`settings_span`), `notes_edit.rs` and `slide_edit.rs` (lift shared helpers
  to `pub(crate)`), a `SlideSources` contract type + `bindings/`
- `crates/peitho/src/server.rs` (`DeckWrite::SlideSource`, route,
  `DeckWriteOutcome`), `main.rs` (`write_preview_slide_source`, scope rename,
  `sources.json` emission, contamination list)
- `packages/peitho-present/src/preview.ts`, `previewSourceEdit.ts` (new),
  rebuilt `dist/preview.js`
- `CLAUDE.md`, `site/content/guide/` preview page, README feature list

## Edge cases

- **Explicit key**: the settings comment is re-emitted verbatim, so the key
  cannot change. **Derived key**: a heading change may change it (rule above).
- **Slide with a section marker**: the marker lives in the settings comment
  and is preserved; sections are compared anyway.
- **Skipped slides** are editable (they are real slides). **Draft slides** do
  not exist after parse and have no source entry.
- **First slide after frontmatter**: `source_span` starts after the
  frontmatter; nothing special.
- **CRLF decks**: bodies are LF in the browser; `write_preview_origin_rewrite`
  already restores pure-CRLF origins. **BOM**: `strip_bom` / `restore_bom`.
- **Notes edited in the panel while the source editor is open** cannot happen:
  focusing the notes textarea blurs the source textarea, which commits first.
- **Known tradeoff**: after a save that changes a derived key *and* breaks the
  rebuild, the stale page's notes map is keyed by the old key, so a note save
  answers 409 until the build is fixed. The source editor itself keeps working
  (that is what the response key is for), and fixing the body is the way out.
- **Known tradeoff**: a whole-line note comment between two non-blank lines
  is replaced by a blank line, not deleted (the shared rule that keeps `text`
  from joining a following `---` into a setext heading). Between the items of
  a tight list that blank line makes the list loose, so the body shows — and a
  save writes — a loose list. Deleting the line instead would join two
  paragraphs written without blank lines around the comment. The body the
  author sees is the body that is saved; note saves have the same effect
  (Issue #582).
- Empty comments (`<!-- -->`) carry no span and stay in the body verbatim.
- **Known tradeoff**: the shell's source map is refreshed by a generation
  reload or by its own source save. An *inline* edit or an external editor
  change followed by a failed rebuild leaves it stale, so `e` answers the
  honest 409 "the deck changed on disk" until a build succeeds. Accepted: the
  alternative is an endpoint serving live source, a second way for body text
  to reach the browser.
- **Known tradeoff**: the first source save of a slide whose note comments are
  scattered or precede the settings comment canonicalizes their position. Note
  saves already do this.

## Verification

- Core: table-driven tests for `slide_body` (no comments / settings only /
  notes before, inside, inline, after / CRLF / BOM) and for every refusal row
  above; property: `rewrite_slide_body(src, s, slide_body(src, s))` leaves
  every slide's parse-level identity unchanged and is idempotent on its own
  output.
- Server: 404 under present, 409 drift, 409 key gone, 422 refusals, include
  file targets (first/middle/last), response key on a derived-key change,
  `Note`/`SlideEdit` success bodies unchanged.
- Publish: `sources.json` under `dist/` is rejected.
- Shell (vitest): `e` gating, one-edit-at-a-time across kinds, commit/cancel,
  in-flight lock, deferred reload release-once, failed save blocks transition,
  pagehide keepalive and its 60 KiB downgrade.
- Real Chrome E2E (required; jsdom cannot see layout or focus): open, type
  with a real IME, Cmd+Enter, reload lands on the rendered slide; a check-phase
  failure shows the banner and `e` reopens with the saved text; derived-key
  change + broken build can still be fixed from the page; blur commit by
  clicking the notes textarea. First click after navigation only focuses the
  tab (pitfall), so click once before the step under test.
