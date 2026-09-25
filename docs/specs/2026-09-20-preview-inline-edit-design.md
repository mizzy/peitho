# Inline slide text editing in preview — design

Date: 2026-09-20
Status: design approved by the author in conversation; implementation plan in
`docs/plans/2026-09-20-preview-inline-edit.md`.

## Goal

In `peitho preview` single mode (the filmstrip layout), let the author fix a
typo or reword a sentence **on the rendered slide itself**, without switching to
an editor. Clicking a text block swaps it to its inline Markdown source in
place; committing writes the change back to the deck's Markdown file, the
ordinary watch rebuild runs, and the page reloads with the result.

Markdown stays the single source of truth. This is a convenience write path
into the Markdown file, exactly like editable notes (`POST /notes`), not a
second content model.

### Non-goals

- A general WYSIWYG editor. There is no HTML→Markdown conversion anywhere.
- Structural edits: adding/removing/reordering blocks, changing heading levels,
  turning a paragraph into a list, splitting slides. These are refused with a
  reason; the author uses their editor.
- Editing code blocks, `code_images` output (Mermaid, math, embeds), images,
  raw HTML blocks, page settings, or layout HTML. (Footnote definition bodies
  were a non-goal until 2026-09-25; see
  `docs/plans/2026-09-25-inline-edit-footnotes.md`.)
- Editing in grid mode, in thumbnails, in `peitho present`, or in `dist/`.

## Author decisions (2026-09-20)

1. **Edit on the rendered slide**, not in a separate source textarea.
2. **Block-level source swap**: the clicked paragraph / heading / list item /
   table cell shows its inline Markdown source (`Peitho is a *fast* tool`), so
   inline syntax such as `**bold**` can be added *and* removed, and edits can
   cross emphasis boundaries. (Rejected: text-node-only editing, which could
   add syntax but never remove it, and needed text matching to find the source.)
3. **A heading edit may change the slide's derived key.** Only the target
   slide's key may change, only when it is `KeySource::Derived`. Because a
   keyed CSS selector may reference the old key and break the following
   rebuild, **watch rebuild failures are shown in the preview page** (today
   they are terminal-only). That banner also benefits ordinary editor-driven
   workflows.
4. **Interaction**: click starts editing; Enter or blur commits; Shift+Enter
   inserts a newline; **Escape cancels** and restores the rendered block
   (deliberately different from the notes textarea, where Escape blurs and
   saves — a click on a slide is easier to make by accident than focusing the
   notes box). A failed save keeps the edit open, shows the error, and blocks
   slide changes, like notes.

## Approach

### 1. The parser is the only authority on what is editable

`SourceFragment` gains editable spans: byte ranges into the include-expanded,
BOM-stripped combined source (the same coordinate space as
`ParsedSlide.source_span` and `note_spans`).

- An `EditableSpan` is the **inline content range** of one block: first inline
  event start to last inline event end. For `# Title` that is `Title` (the `# `
  marker is outside the span, so the heading level cannot be edited); for a
  list item it is the item's own inline run (marker and nested lists outside);
  for a table cell the cell's inline content.
- Editable blocks: paragraphs (including those inside blockquotes, loose list
  items, and `::: {slot=…}` / `::: {reveal}` groups), headings, tight list
  items' inline runs, table cells.
- `EditableSpan`'s constructor is `pub(crate)` to the parser and is only built
  when `combined[span]` is byte-identical to the Markdown the renderer will
  re-parse for that block. Any block where the parser cannot prove that (a
  transformed or synthesized fragment) simply has no span and is not editable.
  No consumer can fabricate a span, so "the DOM says this range is editable"
  always traces back to a parser decision.
- Spans ride Parsed→Mapped→Checked alongside `line` so the renderer can use
  them. They are not part of the manifest or any ts-rs contract type.

### 2. Preview-only render annotations

`render_deck` takes an explicit `EditAnnotations::{Off, On}` argument.
`peitho preview` is the only caller that passes `On`.

With `On`, each editable block element carries two attributes:

- `data-peitho-src="<start>-<end>"` — the `EditableSpan`
- `data-peitho-md="<escaped inline Markdown source>"` — `combined[span]`

`<p>`, `<h1>`–`<h6>`, `<td>`/`<th>` carry them directly. A tight `<li>` carries
them and they describe the `<li>`'s leading inline child nodes (everything
before its first nested block element). Title-slot headings
(`Accepts::Inline`) get a per-fragment `<span>` wrapper carrying the
attributes, in `On` mode only.

The renderer already iterates body Markdown with `into_offset_iter()` and
tracks each fragment's range inside the joined run
(`BodyMarkdownSource.range`), so the absolute offset of an event is
`fragment_span.start + (event.start - source.range.start)`. The element
annotation is emitted where the renderer already normalizes events.

Shipping the source in an attribute means **no new file, no new fetch, and no
new contract type**. The cost is larger preview HTML, which is local-only.

With `Off` the output is byte-identical to today (pinned by a test over the
example decks). `dist/`, PDF, lint, and present never see the attributes by
construction; the publish contamination check additionally rejects
`data-peitho-src` / `data-peitho-md` in `dist/`.

### 3. Save path: `POST /slide-edit`

Request (`application/json`, `deny_unknown_fields`):

```json
{"key": "<SlideKey>", "start": 120, "end": 143, "old": "Peitho is a *fast* tool", "new": "Peitho is a **very fast** tool"}
```

Only preview installs the writer; elsewhere the route answers 404 (same rule as
`/notes`).

Note saves and slide edits both rewrite deck files, so they must not interleave.
The existing `NotesWriter` closure and its mutex become one **deck writer**
value owning both operations behind a single mutex. `write_preview_note`'s tail
(translate span through `LineMap` → read origin → drift check against combined
bytes → CRLF preservation → `write_atomic`) is extracted into one shared
function that both operations call; it is not duplicated.

Flow for an edit:

1. `load_and_expand_deck_source` → `parse_deck` (`UntransformedDeck`, so
   `code_images` renderers never run on a save).
2. Find the slide by `key` → 409 if gone.
3. Require `start..end` to be one of that slide's `EditableSpan`s in the
   **fresh** parse and `combined[start..end] == old` → otherwise 409 "the deck
   changed on disk; reload and retry". This is the drift guard: ranges in the
   browser come from an older generation.
4. `slide_edit::rewrite_block(combined, slide, span, new, highlighter)` (pure,
   in peitho-core, sibling of `notes_edit`): splice, then reparse and check the
   postcondition below → 422 with the specific reason on refusal.
5. Translate the **block span** (not the slide span — smaller, so more of an
   included deck is editable) to the origin file and write atomically.
6. Respond `{"saved":true}`. The watcher rebuild follows on its own.

`new` is normalized by trimming trailing/leading newlines only. A `new` equal
to `old` is never posted by the shell and is a no-op `Ok` on the server.

#### Postcondition (`preserves_deck_for_block_edit`)

Reparse old and new; require all of:

- both parse (a new-source parse error is a 422 carrying the diagnostic);
- same slide count; equal `sections`;
- every **non-target** slide: equal key, key-source kind, layout request,
  `skip`, page-number flag, notes;
- **target** slide: equal notes, layout request, flags; equal fragment-kind
  sequence (recursing into `SlotGroup` children, heading levels included);
  equal number of editable spans; equal reveal step count;
- target key: unchanged, or changed only when both old and new key sources are
  `Derived` (key uniqueness is already a parse error, hence covered above).

Consequences, all by construction rather than by special cases: emptying a
block, typing `- ` / `# ` / a blank line that creates a new block, a stray
`---`, a note comment, or a `:::` fence are all refused because the kind sequence, span
count, slide count, or notes change. (Corrected during implementation: a bare
`-->` is harmless paragraph text and is accepted. The implemented postcondition
is also stricter than this list — span text and each fragment's block-level
event sequence are compared too; see the plan's Task 4 note.)

### 4. Rebuild failures in the page

`/sync` gains one more server-owned absolute field on every JSON GET response:
`"buildError": string | null`. The `/sync` GET response has no Rust-owned
contract type today (a server-local struct and a handwritten TS type), so this
change first moves the whole response shape into `peitho_core::sync`
(`SyncResponse`, `SyncTimerSnapshot`) with ts-rs bindings, per the
single-source-for-the-contract invariant, and adds the field there. A failed watch rebuild sets it (and wakes
pollers); a successful rebuild clears it together with the `generation` bump.
`POST /sync` does not accept it.

The preview shell renders a non-null `buildError` as a fixed banner at the top
of the page (`textContent`, preformatted, scrollable, shell chrome only). The
last good generation stays on screen underneath. The first-build-failure error
page is unchanged.

### 5. Preview shell

- Single mode only. A click on the stage whose `composedPath()` reaches a
  `[data-peitho-src]` element inside the current slide's shadow root — and is
  not inside an `<a>` (links keep opening) — starts an edit. Clicking the stage
  in single mode does nothing today, so no behavior is displaced.
- The block's editable nodes are replaced by the `data-peitho-md` text and the
  element (for `<li>`, a wrapper around the leading inline run) becomes
  `contenteditable="plaintext-only"` with an outline. Typography stays the
  slide's own, so the text stays roughly in place.
- Enter commits; Shift+Enter inserts a newline; blur commits; Escape cancels.
  IME composition keys (`isComposing` / `keyCode 229`) are ignored, so the
  Enter that confirms a Japanese conversion never commits.
- One edit at a time. State is derived: `editing = {key, start, end, old}` plus
  the element's current text; dirty = `text !== old`.
- Commit → `POST /slide-edit`. Success: keep showing the source text until the
  generation reload replaces the page. Failure: keep the edit open and show the
  `{error}` in the existing notes-panel status area (generalized to a panel
  status).
- `commitTransition` settles edits as well as notes: a pending edit is
  committed first and a failed save blocks the slide change / grid entry.
  `installPreviewKeyboard` already treats contenteditable targets as editable
  (only unshifted PageUp/PageDown navigate).
- A generation change (or reload) that arrives **while an edit is open is
  deferred** until the edit commits or cancels; otherwise an external-editor
  save would discard the in-progress text. If the file really changed under
  the edit, the commit gets the 409 drift error and Escape releases the
  deferred reload.
- `pagehide`: a dirty open edit is posted directly with `keepalive`, like
  notes. No sessionStorage draft for edits (the deferred reload removes the
  case that needed one for notes).
- Thumbnails carry the same attributes but are `pointer-events: none`; grid
  tiles keep their click-to-open meaning.
- §16: the slide body stays unaware of the shell; all of the above is preview
  shell code reading data attributes.

## Files

```
crates/peitho-core/src/domain.rs      EditableSpan, SourceFragment spans
crates/peitho-core/src/parser.rs      span capture (verbatim invariant)
crates/peitho-core/src/render.rs      EditAnnotations, attribute emission
crates/peitho-core/src/slide_edit.rs  rewrite_block + postcondition (new)
crates/peitho/src/server.rs           POST /slide-edit, deck writer, buildError in /sync
crates/peitho/src/main.rs             preview wiring, shared origin-write seam, rebuild error state
crates/peitho/src/publish…            contamination check for data-peitho-src/md
packages/peitho-present/src/preview.ts  click-to-edit, settle/transition, banner
crates/peitho-core/src/sync.rs        SyncResponse contract (new)
bindings/                             SyncResponse.ts, SyncTimerSnapshot.ts (new)
CLAUDE.md, site/content/guide/…       docs
```

## Edge cases

- **Included slides**: the block span translates through `LineMap`; a span
  touching a synthetic edge is a 409 naming the file, like notes.
- **CRLF origin files**: `new` is converted with the existing bare-LF→CRLF
  helper.
- **`breaks: true`**: source newlines are meaningful; Shift+Enter writes a
  real newline and the postcondition still guards structure.
- **Footnote references / inline code / links inside a paragraph** appear as
  their Markdown source and are editable as text. Inline images do not exist in
  this grammar (text mixed with an image is already a parse error, and a sole
  image paragraph is `FragmentKind::Image`, which has no span). Removing the
  last reference to a footnote is refused with the parser's own diagnostic for
  the now-unused definition — no shell-side special case.
- **Multi-line blockquote paragraphs** show their continuation markers
  (`quoted one\n> quoted two`) because the span is a verbatim source slice.
  Accepted: rewriting the slice would break the verbatim invariant.
- **Reveal / emphasis**: preview shows final state; edits inside revealed
  content are fine, and a changed reveal step count is refused.
- **Same text twice on a slide**: irrelevant — addressing is by byte range.
- **Two preview tabs**: the second tab's ranges go stale after the first
  tab's save → 409 drift → reload.

## Verification

- Rust unit tests: span capture per block kind (incl. CJK, nested lists,
  tables, slot/reveal groups, includes), `Off` byte-identity over `examples/`,
  `rewrite_block` acceptance and every refusal reason, server status mapping,
  `/sync` `buildError` set/clear.
- vitest: click→edit→commit/cancel, IME Enter ignored, transition blocking,
  deferred reload, banner. Shells/listeners destroyed per test.
- Real-Chrome E2E (required before claiming done): typo fix in a paragraph,
  heading edit with derived key (position kept after reload), keyed-CSS break
  shows the banner, Japanese IME edit in a CJK deck, included-file slide,
  link click still opens the link.
