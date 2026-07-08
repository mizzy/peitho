# Markdown image support plan (Issue #117 / #106)

## Decisions

- The `path` in `![alt](path)` is **relative from the deck file only**. Remote URLs, absolute paths, and URL-scheme-like values are line-numbered build errors
- Allowed image extensions in the MVP are **`.png` / `.jpg` / `.jpeg` / `.gif` / `.webp`**. Case-insensitive; lowercased before comparison. No extension, `.exe`, `.md`, `.svg` are line-numbered build errors at the parser stage
- Images are assigned **only to slots with `accepts="image"`**. Layouts with no image-accepting slot, or with multiple such that convention cannot resolve uniquely, are build errors
- Dist layout is `dist/assets/<hash>-<basename>.<ext>`. Deduplicate by content hash; identical content is copied once
- #106 is resolved at the same time. Put `FragmentKind::Image` / `Accepts::Image` on the actual parser → mapping → check → render path
- `Accepts::Text` is untouched in this PR. Comment on #106: "Image is resolved, Text remains"

## Invariants

- Images are Markdown-side content. Sizing, placement, and cropping sit on the layout HTML/CSS side
- Do not silently swallow unknown / unsupported / mixed structures with `_ => {}`. Undefined cases become line + help errors
- Preserve the `Parsed → Mapped → Checked → Rendered` type boundaries. The image src that reaches `Rendered` is only `ResolvedImagePath`, and no type path lets `RawImagePath` generate `<img src>` directly

## Target code

- `crates/peitho-core/src/domain.rs`
  - Change `FragmentKind` so it can hold `Image { alt: String, src: S }`; add `RawImagePath` / `ResolvedImagePath` / `ResolvedImageAsset`
  - Add `SourceFragment::image(line, alt, RawImagePath)` and an `image()` accessor
  - Drop `Copy` from `FragmentKind`. `kind()` returns a reference; `default_accepts()` / `removal_noun()` / `Display` work regardless of the image payload
- `crates/peitho-core/src/parser.rs`
  - Remove `Tag::Image { .. }` from `unsupported_tag()` / `unsupported_tag_name()`
  - Give `OpenBlock::Paragraph` inline state; only paragraphs that are a single image get converted to `SourceFragment::image`
  - Call `RawImagePath::new(raw, line)`, and turn URL / absolute path / non-allowed extension into line-numbered parser-stage errors
  - Detect `Tag::Image` before the whole-item ignore for `list_depth > 0`; images inside lists are errors for now
- `crates/peitho-core/src/mapping.rs`
  - Change `map_slide()` to a form that can fail on image slot selection (`Result<MappedSlide>`)
  - `FragmentKind::Image { .. }` does not fall through to `body`; assign only to a slot where `accepts == Accepts::Image`
- `crates/peitho-core/src/check.rs`
  - Pin the existing `(Accepts::Image, FragmentKind::Image)` arm with a reachable test
  - Exhaustively update all matches to accommodate the payloadful `FragmentKind`
- `crates/peitho-core/src/error.rs`
  - Add `ErrorKind::Asset` so resolve-stage I/O failures don't mix with parser / layout / manifest
- `crates/peitho-core/src/render.rs`
  - `render_deck()` accepts only `Deck<Checked<ResolvedImagePath>>`
  - `render_slot()` / `render_image_fragment()` use only `ResolvedImagePath`'s `html_src()`, HTML-attribute-escape alt, and emit `<img>`
- `crates/peitho-core/src/manifest.rs`
  - Add `ManifestImage { src }` and `images: Vec<ManifestImage>`; use `#[serde(default)]` so existing manifest publish validation is not broken
  - `build_manifest()` either takes a list of resolved image assets or derives `images` from a resolved Checked deck
- `crates/peitho-core/src/lib.rs`
  - Publicly expose `RawImagePath` / `ResolvedImagePath` / `ResolvedImageAsset` / `ManifestImage` / image resolution functions used by the CLI
- `crates/peitho/src/main.rs`
  - Insert a `resolve_image_paths()` call in `build_artifacts()`
  - The CLI-side resolver resolves real files relative to the deck parent and determines hash and `assets/...`
  - `emit_distribution()` / `emit_present_cache()` recreates `assets/` and copies images
  - `validate_publish_dist()` / `read_publish_manifest()` verify existence and dist-relative-ness of `manifest.images[*].src`
- `crates/peitho/tests/{build.rs,publish.rs}`, unit tests in `crates/peitho-core/src/*`, `examples/`, `bindings/`
  - Add an image-bearing example, regenerate bindings, add publish-missing detection

## Type design

`RawImagePath` is the value written in Markdown. On construction it allows only "local relative path".

```rust
pub struct RawImagePath(String);
impl RawImagePath {
    pub const ALLOWED_EXTENSIONS: &'static [&'static str] = &["png", "jpg", "jpeg", "gif", "webp"];
    pub fn new(raw: impl Into<String>, line: usize) -> Result<Self>;
    pub fn as_str(&self) -> &str;
    pub fn extension(&self) -> &str;
}
```

Enforce the extension check at the parser stage. Reason: leaving `foo.exe` / `notes.md` to be copied by the resolver and dispatched by Chrome's MIME interpretation gives a silent path where HTML generation succeeds. SVG might get its own policy later; the MVP explicitly rejects `.svg`.

`ResolvedImagePath` exposes only the dist-relative src that is safe to write into HTML. The source path lives on the copy-side asset.

```rust
pub struct ResolvedImagePath(String); // "assets/<hash>-<basename>.<ext>"
pub struct ResolvedImageAsset {
    pub source_abs: PathBuf,
    pub dist_rel: ResolvedImagePath,
}
```

`SourceFragment` / `FragmentKind` are made generic over the image src type.

```rust
pub enum FragmentKind<S = RawImagePath> {
    Heading { level: u8 },
    Paragraph,
    Text,
    Code,
    Image { alt: String, src: S },
    List,
}
```

Core boundary:

```rust
pub fn resolve_image_paths<R>(
    deck: Deck<Checked<RawImagePath>>,
    resolver: R,
) -> Result<(Deck<Checked<ResolvedImagePath>>, Vec<ResolvedImageAsset>)>
where
    R: FnMut(ImageRequest<'_>) -> Result<ResolvedImageAsset>;

pub fn render_deck(deck: Deck<Checked<ResolvedImagePath>>) -> Result<Deck<Rendered>>;
```

This means `render_image_fragment()`, which generates `<img src>`, can only accept `ResolvedImagePath`. The CLI injects the resolver, but core itself has no filesystem or copy side effects.

## Error routing

Every layer returns `peitho_core::BuildError`; the CLI unifies to `miette` diagnostics via the existing `core(result)`.

- parser stage: URL / absolute / non-allowed extension, mixed paragraph, list-inner image → `ErrorKind::Parse`
- mapping stage: no image slot, multiple image slots where convention cannot resolve, structural match fail/ambiguous → `ErrorKind::Layout`
- resolve stage: file missing, permission denied, read failure during hashing → new `ErrorKind::Asset`

The resolver's type is fixed to `BuildError`.

```rust
pub struct ImageRequest<'a> {
    pub raw: &'a RawImagePath,
    pub line: usize,
}

pub fn resolve_image_paths<R>(
    deck: Deck<Checked<RawImagePath>>,
    resolver: R,
) -> Result<(Deck<Checked<ResolvedImagePath>>, Vec<ResolvedImageAsset>)>
where
    R: FnMut(ImageRequest<'_>) -> crate::Result<ResolvedImageAsset>;
```

The CLI resolver converts `io::Error` from `fs::metadata` / `fs::read` / `fs::canonicalize` into `BuildError::new(ErrorKind::Asset, Some(request.line), ..., ...)` in place. `resolve_image_paths()` traverses checked slides, so it appends slide number / key to any `BuildError` returned by the resolver. `io::Error` and `miette::Report` never cross the core boundary.

## Parser event handling

Change only pulldown-cmark 0.13's body parser (`parser_options`). Keep frontmatter detection and slide-split grammar separated as before; don't mix metadata-block settings into the split side.

Standalone image paragraph:

```text
Start(Paragraph)
Start(Tag::Image { dest_url, title, id, .. })
Text("alt")
End(TagEnd::Image)
End(Paragraph)
```

Turn this into `SourceFragment::image(paragraph_start_line, alt, RawImagePath::new(dest_url, line))`. Don't create a Paragraph fragment.

Text-only paragraph:

```text
Start(Paragraph)
Text(...)
End(Paragraph)
```

Same as today, `SourceFragment::paragraph`.

Mixed paragraph:

```text
Start(Paragraph)
Text("before ")
Start(Tag::Image { .. })
Text("alt")
End(TagEnd::Image)
Text(" after")
End(Paragraph)
```

For now, error as `unsupported construct 'mixed image paragraph'`. Line is the earlier of paragraph start or image start; help is "split into image-only paragraphs". Multiple images in one paragraph is also an error; if multiple images are needed, split into separate paragraphs and produce multiple Image fragments.

Alt collection collapses `Text` / `Code` / breaks into plain text. If an unsupported tag arrives inside an image, error out — don't let the outer `Event::Start(Tag::Emphasis | Tag::Strong | Tag::Link)` existing ignore arms accidentally swallow it.

Image inside a list:

```text
Start(List)
Start(Item)
Start(Paragraph)
Start(Tag::Image { .. })
...
```

Current list processing does not look at inner events for `list_depth > 0` and keeps the source Markdown as `SourceFragment::list`. Allowing images here would let the later `html::push_html` generate a raw-path `<img>`, so for now explicitly error as `unsupported construct 'image inside list'`.

## Why replace with Image rather than Paragraph

If we handled it as paragraph-inner inline, the fragment would stay as `FragmentKind::Paragraph` and flow into `blocks/body`, so the author-intent "image slot only" cannot be type-checked. Replacing the standalone-image paragraph with `FragmentKind::Image { alt, src }` lets mapping/check inspect the `Accepts::Image` contract directly. Mixed paragraphs are explicit errors until the inline-image design is decided.

## dispatch / mapping fallout

The public functions `dispatch_by_convention()` and `map_by_convention()` already return `Result<Deck<Mapped>>` today, so signatures on the CLI (`crates/peitho/src/main.rs::build_artifacts`) and external export (`crates/peitho-core/src/lib.rs`) side are unchanged. What changes is only the return of the private `map_slide()` to `Result<MappedSlide>` and its callers.

Impact surface:

- `crates/peitho-core/src/mapping.rs::dispatch_slide`
  - explicit layout override: `map_slide(&slide, layout)?`. An image-slot error is a definitive error for the specified layout; no fallback to other layouts
  - single layout: `map_slide(&slide, layout)?`. As before, returns the shortest error against that layout
  - multi layout structural probe: try `map_slide()` per layout; treat `map_slide` errors or `check_slide` errors as rejections for that layout
- `crates/peitho-core/src/mapping.rs` tests
  - Existing dispatch tests can keep the `unwrap()` shape at the public function. Add structural-match tests with images
- `crates/peitho-core/src/check.rs` / `render.rs` / `manifest.rs` tests
  - `map_by_convention(...).unwrap()` call sites are signature-compatible. The main fallout is match-arm updates from `FragmentKind` payloadification
- `crates/peitho/src/main.rs::build_artifacts`
  - `core(peitho_core::dispatch_by_convention(parsed, &layouts))?` stays as-is. New mapping errors surface through miette via the existing route

Image impact on structural match:

- Slide with image + one layout with an image slot + multiple without → only the image layout matches
- Slide with image + multiple layouts with an image slot → ambiguous layout error
- Slide with image + zero layouts with an image slot → no layout matches
- Slide without images → title/body/code/list decision unchanged
- With an explicit layout, don't use the structural probe; if the specified layout has no image slot, layout error right there

## Asset side-effect phase

Shared pipeline for build/present:

```text
read markdown
parse_markdown
dispatch_by_convention
check_deck
resolve_image_paths(core traversal + CLI resolver)
build_manifest(with images)
build_theme_css
render_deck(resolved only)
emit_distribution / emit_present_cache(copy assets)
```

CLI resolver responsibilities:

- `input.parent()` becomes the deck-base directory
- Resolve `deck_dir.join(raw.as_str())` to a real file; if missing, line-numbered build error for the raw path
- Extensions are already enforced at the parser, so the resolver does not re-check by default. But `ResolvedImagePath::new` accepts only dist-relative paths under `assets/`
- Hash the file contents, return `assets/<hash>-<basename>.<ext>`
- Deduplicate `ResolvedImageAsset` per hash in a `BTreeMap` etc. If the content is identical, reuse the first dist name even when basenames differ

Emit-side responsibilities:

- Like `write_slide_fragments()`, recreate `assets/` each time so no stale image survives
- Both `emit_distribution()` and `emit_present_cache()` call `copy_image_assets(out_or_cache, &artifacts.image_assets)`
- Add `image_assets: Vec<ResolvedImageAsset>` to `BuildArtifacts`

## TDD task list

| Red test | Green production change | Silent-drop countermeasure |
|---|---|---|
| `parses_standalone_image_paragraph_as_image_fragment` | Add `RawImagePath` / `FragmentKind::Image { alt, src }` / `SourceFragment::image` in `domain.rs`, image-only paragraph handling in `parser.rs` | Add a dedicated match right after removing `Tag::Image` from `unsupported_tag`; don't fall through to `_` |
| `rejects_remote_image_url_with_line` / `rejects_absolute_image_path_with_line` | `RawImagePath::new` rejects scheme, `//`, absolute/root/prefix components | Don't leave the URL as Paragraph markdown; parse error |
| `rejects_image_without_supported_extension` / `rejects_svg_until_policy_is_decided` | `RawImagePath::new` enforces the allowed extensions `.png/.jpg/.jpeg/.gif/.webp` | Don't defer non-image files to Chrome-failure by copying to assets |
| `rejects_text_and_image_mixed_in_one_paragraph` / `rejects_two_images_in_one_paragraph_until_inline_design_exists` | Give `OpenBlock::Paragraph` inline state (`Empty/TextOnly/PendingImage/SingleImage/Mixed`) | Don't paragraphify mixed and let it flow into `blocks` |
| `rejects_image_inside_list_before_markdown_rerender` | In `parser.rs`, detect `Tag::Image` before the `list_depth > 0` ignore and error | Don't let the later list-markdown re-render generate a raw `<img src>` |
| `maps_image_to_unique_image_accepting_slot` | `mapping.rs::map_slide` picks the unique `Accepts::Image` slot from the layout contract | Remove `FragmentKind::Image` from the `body` arm |
| `rejects_image_when_layout_has_no_image_slot` / `rejects_image_when_multiple_image_slots_are_ambiguous` | Make `dispatch_slide` / `map_slide` `Result`-typed; 0-match / multi-match become line-numbered Layout errors | Don't turn it into wrong ResidualContent like "missing body" |
| `dispatch_selects_layout_with_image_slot_as_unique_structural_match` / `dispatch_rejects_two_image_layout_matches` | Include `map_slide` errors as rejections in the multi-layout probe | Don't drop image-slot requirement from structural-match uniqueness decision |
| `check_accepts_image_fragment_in_image_slot` | Update the existing Image arm of `check.rs::accepts_fragment` to be payload-aware | Assert that `Accepts::Blocks` also does not accept image |
| `render_deck_requires_resolved_image_paths` | Compile_fail doctest that `render_deck` cannot be called from `Deck<Checked<RawImagePath>>`; add `resolve_image_paths` | Don't let a raw path flow through the render function's type parameter |
| `renders_image_with_resolved_src_and_escaped_alt` | Add an image branch in `render.rs::render_slot`, add `render_image_fragment(&FragmentKind<ResolvedImagePath>)` | Don't rely on Markdown re-rendering; don't expose `RawImagePath` accessor to render side |
| `build_copies_markdown_image_to_dist_assets` | Inject CLI resolver in `main.rs::build_artifacts`; call `copy_image_assets` in `emit_distribution` | Also assert the original `images/foo.png` does not remain inside HTML |
| `build_fails_for_missing_markdown_image_with_line_and_help` / `build_fails_for_unreadable_markdown_image_with_line_and_help` | CLI resolver converts I/O error to `BuildError(ErrorKind::Asset)`; core attaches slide context | Don't defer missing/permission errors to copy-time panic or publish-time drop |
| `build_deduplicates_images_by_content_hash` | Resolver keeps a content-hash map and returns the same `ResolvedImagePath` for identical contents | Don't double-copy when basenames differ |
| `manifest_serializes_images_array` / `deserializes_manifest_missing_images_as_empty` | Add `ManifestImage` / `images` in `manifest.rs`; update bindings test | Don't break legacy manifest publish validation; enumerate all images in image-bearing manifests |
| `publish_rejects_missing_manifest_image_reference` / `publish_rejects_manifest_image_reference_outside_dist` | Add `validate_manifest_image_refs`; share the slide src validation helper | Don't pass a missing-dist to the publish command |
| `present_cache_copies_markdown_images` | `emit_present_cache` also calls `copy_image_assets` | Close the route where only `peitho present` 404s on images |
| `feature_tour_or_markdown_image_example_builds` | Add an image slot layout, PNG fixture, and deck to `examples/` | Assert no raw path in example HTML/manifest and that assets exist |

## manifest / publish

`manifest.json` is additive.

```json
{
  "images": [
    { "src": "assets/4f8c...-diagram.png" }
  ]
}
```

- `images` always serializes; deserialize is `#[serde(default)]`
- Publish validation checks `images[*].src` by the same rules as `slides[*].src`: empty string, absolute, `..`, root/prefix component are errors
- `dist/assets/` itself is not required for decks without images. If `images` is non-empty, existence of each src is required

## bindings / shell / gates

- Adding `images` to `Manifest` and adding `ManifestImage` means regenerating and committing `bindings/Manifest.ts` and the new `bindings/ManifestImage.ts`
- `FragmentKind` is not currently a TS export target, but confirm the bindings test doesn't break under domain genericization
- The present shell reads manifest types, so `packages/peitho-present`'s typecheck must pass. If runtime doesn't touch images, `shell.js` diff should be zero, but the drift gate is required

Gate:

```text
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
```

For the image-bearing example, run `peitho build examples/<image-example>/deck.md` and inspect `dist/assets/`, slide HTML, and manifest.

## Undecided

- Whether mixed paragraphs (`text ![alt](x.png) text`) will one day be handled as Paragraph inline, or designed together with a slot-specification notation
- Whether SVG will one day be allowed as an opaque `<img>` asset, or if sanitize/reject continues. MVP: `.svg` is off the allow list and explicitly rejected
- Whether image-size hints (`![alt](x.png){width=...}` etc.) sit on the Markdown side, or are pushed entirely into CSS
- Notation to pick a specific slot from Markdown when a layout has multiple `accepts="image"` slots
- How far to plainify Markdown decoration inside alt
- Whether to close the TOCTOU where an image file changes between hash calculation and emit copy, via retaining asset bytes or an open-file-handle design
- Implementation approach for dynamic-watching referenced images under `--watch`. At minimum, add a test that guarantees an image change does not go silently stale before deciding
