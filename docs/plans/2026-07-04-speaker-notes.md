# Speaker notes implementation plan (2026-07-04)

## Decisions

- **Notation**: HTML comments `<!-- ... -->` (Marp / k1LoW/deck style)
  - Discriminate against the existing "HTML comment containing a JSON object = page settings (`key`/`layout`)" by **whether it parses as JSON** (reuse the existing `parse_page_comment` branch)
  - Any non-empty HTML comment that is neither a JSON settings comment nor an empty comment is picked up as a **speaker note**
- **Placement constraint**: may appear anywhere in the slide body. When a slide has multiple comments, join them with `\n\n` (same as k1LoW/deck)
- **Content interpretation**: stored as plain text (only surrounding whitespace `trim`med). Presenter-side rendering also stays plain text (`textContent`) in v1
  - Markdown formatting interpretation is kept open as a later extension axis (`notes.json` is versioned, so forward-compatible)
- **Embedding in distributed HTML**: none. Per existing design, `dist/index.html` does not contain notes; only `peitho present` reads `notes.json`

## Alignment with the three pillars / invariants

- **Pillar 1 (separation of content and design)**: notes are written on the Markdown side as HTML comments → confined to the content side. Layout HTML is untouched ✅
- **Pillar 3 (type checking / no silent dropping)**:
  - Today `ignores_plain_html_comments` in `parser.rs:1000` is a test that "silently drops non-JSON comments." Replace it with **"non-JSON comments are collected as speaker notes"**
  - Empty HTML comments `<!-- -->` are already ignored today. Pin down "empty comments are not treated as notes (ignored)" with a unit test
  - Comments that look like JSON but contain fields other than `key`/`layout` are already rejected by `parse_page_comment` → no change needed
- **typestate**: add `notes: Option<String>` to `ParsedSlide` and carry it through `Mapped→Checked→Rendered`. Assemble the `Notes` collection during the `Checked→Rendered` transition once `SlideKey` is fixed

## Parser changes (`crates/peitho-core/src/parser.rs`)

Current `Event::Html | Event::InlineHtml` branch (`parser.rs:465-497`):

1. If `parse_page_comment` returns `Some(settings)`, handle as settings
2. Otherwise, for non-empty HTML that is not `is_html_comment`, raise `unsupported_construct`
3. HTML comments that are not settings do **nothing** (← this is where silent dropping happens)

Changes:

- In case 3, extract the comment body (between `<!--` and `-->`), `trim` it, and if non-empty push it into `ParsedSlide.note_fragments: Vec<String>`
- At the end of `parse_slide`, collect `note_fragments.join("\n\n")` into `notes: Option<String>` (`None` if empty)
- No position constraint (before / inside / after the body is all fine). The "leading only" constraint for settings comments is preserved
- Tests:
  - `collects_speaker_note_from_html_comment` (one comment enters `notes`)
  - `joins_multiple_html_comments_with_blank_line` (multiple comments join with `\n\n`)
  - `note_with_page_settings_comment_coexist` (settings comment + a separate note comment coexist)
  - `empty_html_comment_is_ignored` (`<!-- -->` is ignored)
  - `note_can_appear_after_content` (comments after the body are also collected)
  - Existing `ignores_plain_html_comments` is replaced by the above

## Type / domain layer changes

- Add `notes: Option<String>` to `ParsedSlide` (the type inside parser.rs)
- Propagate `notes` inside `Mapped<Slide>` / `Checked<Slide>` / `Rendered<Slide>` (following the existing field-addition pattern)
- `SlideKey` is fixed from Mapped onward, so build the `Notes` collection (`BTreeMap<SlideKey, String>`) at the Rendered stage

## Notes construction and manifest

- `crates/peitho-core/src/notes.rs`: use the existing `Notes::new(BTreeMap)` as is. Add a builder helper:
  ```rust
  impl Notes {
      pub fn from_slides(slides: &[Rendered<Slide>]) -> Self { ... }
  }
  ```
- `manifest.rs`: pass `slide.notes.is_some()` to `SlideEntry::new`'s `has_notes` (fix the site currently hard-coded to `false`)
- CLI (`crates/peitho/src/main.rs:701`): currently writes `Notes::empty()` → switch to `Notes::from_slides(...)`

## Presenter side

- Currently `presenter.ts:150` does `notesRoot.textContent = options.notes.notes[detail.key] ?? "No notes for this slide."`
- v1 keeps this as **plain-text rendering**. Extension to Markdown interpretation is future work (feasible via a `notes.json` version bump)
- Types (`bindings/Notes.ts`) are unchanged

## E2E verification

- Add a sample with speaker notes to one of `examples/` (appending to an existing deck is fine)
- `peitho build` → inspect the contents of `dist/notes.json`
- `peitho present` → visually confirm notes rendering on the presenter screen while cycling with Cmd+Left/Right
- For multi-display physical-device behavior, use the CLAUDE.md [BetterDisplay virtual display procedure](file:///Users/mizzy/.claude/projects/-Users-mizzy-src-github-com-mizzy-peitho/memory/betterdisplay-virtual-display-e2e.md)

## Gates

- `cargo test --workspace` three times
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` (the `Notes` type is not changed here, so the diff should be zero)
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js` (presenter wiring stays on textContent, so no shell-side changes → diff should be zero)

## Deferred as Undecided

- **Markdown formatting interpretation**: v1 is plain-text only. Leave room to add an axis like `NotesFormat: "plain" | "markdown"` to frontmatter later
- **`::: notes` fenced-div notation**: revisit alongside the §18 fenced div slot notation. Not tackled here

## PR

- Branch: `feat/speaker-notes` (worktree already created: `../peitho-speaker-notes`)
- Open as a draft PR
- Title proposal: `feat: extract speaker notes from HTML comments`
