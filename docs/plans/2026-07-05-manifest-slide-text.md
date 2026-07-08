# Slide plain text in manifest.json (Issue #136)

## Summary

Include the **plain text** for each slide's title/body/code in `manifest.json`'s `slides[i]` as a new field `text: {title, body, code}`. This roots out the "altitude inversion" where deck crawlers, search indexers, and accessibility tools regex-parse `dist/*/slides/*.html` — peitho now emits the structured data itself.

## Motivation

decks (`mizzy/decks` #16) wants to surface slide bodies for SEO crawlers. Today's manifest.json does not carry the slide text, so decks has to regex-parse `dist/<slug>/slides/*.html` fragments to pull out title / body / code. This is a textbook altitude inversion (downstream re-parses upstream output) with the following brittleness:

- Matching fails as soon as `class="slot-title"` becomes single-quoted
- Code-block newlines collapse under `\s+` and become one line when pushed into `<pre>`
- No guarantee the HTML entity decoder covers every pattern peitho emits
- Layout HTML changes (renaming a slot, adding a new slot) silently break extraction on the decks side

peitho itself, which already holds deck.md as an AST, is the right place to emit the structured data. Along the `Parsed → Mapped → Checked` chain, the Checked phase already has typed structure with slot contracts resolved (`BTreeMap<SlotName, Vec<SourceFragment<ResolvedImagePath>>>`), and plain text is already sitting on it (`SourceFragment::plain_text()`, `SourceFragment::code_text()`, `heading_text()`). Grow a plain-text serializer alongside the HTML one.

## Design decisions

### Field shape: `text: { title, body, code }` per slide

Following the Issue body's proposal, add a new field to `ManifestSlide`:

```json
{
  "index": 0,
  "key": "cover",
  "src": "slides/000-cover.html",
  "hasNotes": true,
  "text": {
    "title": "Peitho",
    "body": "",
    "code": ""
  }
}
```

- **`title`**: take plain text from headings across all `slot-title` slots (usually one); if multiple, join with newline. Markdown inline notation is textified (`**bold**` → `bold`)
- **`body`**: take plain text from `Paragraph` and `List` across all `slot-body` slots (also includes suffixed forms like `slot-body-*`); join with newline
- **`code`**: take **newline-preserving raw source** from `Code` across all `slot-code` slots; if multiple code blocks, join with a blank line
- If a slot has no matching content, its field is an empty string. The `text` object itself is always present (all three keys always present)

**"slot-title / slot-body / slot-code" is determined by slot name string**. Reasons:

- `mapping.rs`'s convention mapping is name-based (`heading` → `title`, `paragraph`/`list` → `body`, `code` → `code`, `image` → `image`) — a sub-convention under pillar ② ("the layout is the contract"). Existing content that rides this convention has a 1:1 correspondence between slot name and the kind of fragments inside
- If the author routed to a differently-named slot via explicit slot syntax `::: {slot=name}`, that slot name won't match `title`/`body`/`code` and so won't be included in `text.title/body/code`. This is the intended behavior (the author who wrote explicit slots does not want the convention-name mapping to apply)
- The Issue body's proposal is written explicitly slot-name based: "contents that land in `slot-title` slot", "contents that land in `slot-body` slot", "contents that land in `slot-code` slot"

### Where to convert: new module `plain.rs` in peitho-core

Add `crates/peitho-core/src/plain.rs` (new file) with a function that extracts plain text from a Checked slide.

```rust
pub struct SlideText {
    pub title: String,
    pub body: String,
    pub code: String,
}

pub fn slide_text<S>(slide: &CheckedSlide<S>) -> SlideText { ... }
```

- Separate roles: `render.rs` is the HTML serializer, `plain.rs` is the plain-text serializer
- `manifest.rs` calls `plain.rs` to assemble a `ManifestSlideText`
- Pure function, so unit tests are straightforward

### Extract logic per slot

All classification is by slot name string:

1. **title**: find slots where `slot.as_str() == "title"`; for each fragment `f` in them:
   - If `f.kind()` is `Heading`, take `f.plain_text()` (which is the same `text` field as `heading_text()`)
   - Otherwise ignore
   - Join multiples with `"\n"`

2. **body**: find slots where `slot.as_str() == "body"`; for each fragment inside:
   - If `f.kind()` is `Paragraph` or `List`, return the **plain text stripped of inline notation** from `f.markdown()`
   - Text kind is essentially unused (in existing code, `text` is empty), so ignore
   - Do not include `alt` text for Image (v1, prefer simplicity)
   - Join multiples with `"\n"`

3. **code**: find slots where `slot.as_str() == "code"`; for each fragment inside:
   - If `f.kind()` is `Code`, take `f.code_text()` (newline-preserving raw source)
   - Multiple code blocks join with `"\n\n"` (single blank line)

### Stripping inline notation from Paragraph / List

`SourceFragment::paragraph(...)` and `SourceFragment::list(...)` are constructed with `markdown: raw` and `text: ""` (confirmed in the parser; `domain.rs` line 521-541). So `plain_text()` returns an empty string. Because `plain_text()` is a field intended for Text kind, it stays that way.

Plain-text extraction for `body` is implemented anew in `plain.rs` as "run pulldown-cmark's inline parser on the markdown string and pick only `Event::Text`". Parallels the existing `render_heading_inline` (`render.rs`) which serializes heading markdown to HTML — this variant instead pushes only `Event::Text` into a `String`.

- Join list items with `"\n"`
- Normalize soft breaks inside paragraphs to a single space (matches typical Markdown rendering; `\n` is reserved as the fragment separator)
- Code span (`` `foo` ``) picks up `Event::Code` and includes its contents as-is
- Link `[text](url)` only picks up `Event::Text`, so only the link text remains (URL is dropped) — consistent with SEO/crawler intent
- Images produce no `Event::Text`, so they end up empty (alt text is inside `Tag::Image`, but v1 does not pick it up — prefer simplicity. Body-side images are typically already separated into an image slot, so images embedded inside a body slot are rare)

### Serialization: additive, no version bump

- Add `text: ManifestSlideText` to `ManifestSlide`, `serde(rename = "text")`
- `ManifestSlideText { title: String, body: String, code: String }`
- Always emit all 3 keys (keep the keys even if the value is empty string). Reason: consumers do not need branches like `text?.title ?? ''`; the contract is simpler
- **Do not bump `manifest.version`**. Reasons:
  - Existing nullable/optional additions (`sections`, `images`, `aspectRatio`, etc.) did not bump `version` either
  - `text` is purely additive; downstream consumers can ignore `slides[i].text` and everything else works as before
  - decks currently does not read `version` (per Issue body)
- **On manifest deserialize, fall back to `text: {title:"", body:"", code:""}` when `text` is missing** (so `peitho publish` can read manifests written by past versions)
- Emit a TS binding (`bindings/ManifestSlideText.ts`) for the new `ManifestSlideText` struct via `ts-rs`. It rides the existing CI drift check

### `text` name vs `plainText` name

The Issue body leaves room for "not `text` but `plainText` or something", but we adopt `text`. Reasons:

- Same brevity as existing fields (`title`, `slide_count`, `plannedDurationMs`, etc.)
- Consumer code reads naturally as `slide.text.title`
- HTML is also "text", but the neighboring `src` points to HTML — the context makes it clear

## Non-goals

- **Image alt text** is not included in `text.body` (v1). If a decks-side need arises later, consider a separate field `text.images: string[]` or folding it into `text.body`. V1 prefers simplicity
- **Format-specific info** (bullet mark type, code language, link URL, etc.) is not included. This is strictly "plain text so a crawler can read the meaning of the slide", not structured content
- **`manifest.version` bump** (reason: prior section)
- **Integration with `notes.json`**. There is an invariant that notes do not go into `dist/`, so they do not appear in the manifest. This is **not** turned into a `text.notes` field

## Type-safety self-check (CLAUDE.md rule)

- The new function `plain::slide_text` only accepts `CheckedSlide<S>` (a **Checked** phase slide). It is impossible by type to call it at the Parsed/Mapped stage. Matches the existing typestate contract
- Empty string for both (slot doesn't exist) and (slot exists but no fragments of the relevant kind). The two cases are equivalent to "no text", so there is no branch
- Deserialize fallback for the new field is expressed via `#[serde(default)]` + `Default for ManifestSlideText` (guaranteed by the type, not by a runtime patch)

## Test plan (TDD)

Implement in TDD. `plain.rs` unit tests → `manifest.rs` integration tests → binding drift, in that order.

### `plain.rs` unit tests

1. **title slot with single heading** → `title = "Peitho"`
2. **title slot with markdown inline (`# **Bold** heading`)** → `title = "Bold heading"`
3. **title slot missing** → `title = ""`
4. **body slot with two paragraphs** → `body = "First paragraph\nSecond paragraph"`
5. **body slot with a list `- item1\n- item2`** → `body = "item1\nitem2"`
6. **body slot with inline code `` `foo` bar``** → `body = "foo bar"`
7. **body slot with link `[click](url)`** → `body = "click"` (URL not included)
8. **body slot with soft break** → one space in `body`
9. **body slot missing** → `body = ""`
10. **code slot with one code block** → `code = "fn main() {}\n"` (newline preserved)
11. **code slot with two code blocks** → separated by a blank line
12. **code slot missing** → `code = ""`
13. **explicit slot `::: {slot=aside}`** → not included in title / body / code
14. **mixed title+body+code full slide** → all 3 fields as expected

### `manifest.rs` integration tests

1. `build_manifest` embeds `text` in each `ManifestSlide`
2. `manifest_json`'s snapshot contains `"text": {...}`
3. Update the existing `serializes_manifest_schema_exactly` snapshot to include `text`
4. Legacy JSON without the `text` field also deserializes (publish validation)

### bindings

Put `ManifestSlideText.ts` and the exported `text: ManifestSlideText` field of `ManifestSlide.ts` under the CI drift check. Add assertions to the existing `ts_tests::exports_manifest_bindings_with_serde_field_names`.

## Files touched

- `crates/peitho-core/src/plain.rs` (new)
- `crates/peitho-core/src/lib.rs` (add `mod plain;`)
- `crates/peitho-core/src/manifest.rs` (add `ManifestSlideText`, `ManifestSlide::text` field, fill it in `build_manifest`, update tests)
- `bindings/ManifestSlideText.ts` (auto-generated)
- `bindings/ManifestSlide.ts` (auto-regenerated, `text` added)
- `docs/plans/2026-07-05-manifest-slide-text.md` (this document)
- `packages/peitho-present/test/*.test.ts` (6 files; add the newly required `text` field to existing fixtures because the `Manifest` type now requires it. No impact on runtime behavior or shell.js)

No changes on the TypeScript side (`packages/peitho-present/src/`). present does not read `text` from the manifest.

## Gates (from project CLAUDE.md)

```
cargo test --workspace          # 3 times in a row
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/  # contract drift
```

`packages/peitho-present` has no source changes, but run `npm run build` and `npm test` for the drift check.
