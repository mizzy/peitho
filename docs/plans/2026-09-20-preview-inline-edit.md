# Preview: edit inline Markdown blocks in single mode

Date: 2026-09-20
Branch: `preview-slide-edit`

<!-- derived-from ../specs/2026-09-20-preview-inline-edit-design.md -->

## Spec deviations

1. The edge-case claim that an image can be edited "inside a paragraph" is
   infeasible with the current grammar. The design makes that claim at
   `docs/specs/2026-09-20-preview-inline-edit-design.md:232-236`, but the parser
   rejects an image in an open list/table/blockquote container and rejects text
   mixed with an image at `crates/peitho-core/src/parser.rs:2474-2516`; a sole
   image paragraph becomes `FragmentKind::Image` at
   `crates/peitho-core/src/parser.rs:2653-2665`. Existing regression tests also
   pin mixed-image rejection at `crates/peitho-core/src/parser.rs:5611-5635`
   and list/table rejection at `crates/peitho-core/src/parser.rs:5700-5727`.
   Therefore a pure image fragment gets no `EditableSpan`, and a mixed inline
   image still fails parsing before this feature runs. Footnote references,
   inline code, and links inside an otherwise editable paragraph remain
   editable as their verbatim Markdown source.
2. The requested `/sync` binding cannot literally "gain" `buildError` because
   no generated `/sync` response type exists. Rust currently declares a local
   `SyncResponseBody` inside `sync_response_body` at
   `crates/peitho/src/server.rs:1486-1515`, while TypeScript independently
   declares `ServerSyncPollResponse` at
   `packages/peitho-present/src/sync.ts:48-58`. The plan first creates the
   authoritative `peitho_core::sync::{SyncResponse, SyncTimerSnapshot}` types,
   in accordance with the Rust-contract invariant at `CLAUDE.md:41`, then
   generates `bindings/SyncResponse.ts` and
   `bindings/SyncTimerSnapshot.ts`. This does not add an annotation payload:
   editable source still travels only in preview HTML attributes.

## File map

| Change | Exact path | One responsibility | Depends on | Task(s) |
| --- | --- | --- | --- | --- |
| Modify | `crates/peitho-core/src/domain.rs` | Define and carry opaque edit provenance. | Existing `SourceSpan` and `SourceFragment`. | 1 |
| Modify | `crates/peitho-core/src/phase.rs` | Collect a parsed slide's authorized spans in source order. | `domain.rs::EditableSpan`. | 1 |
| Modify | `crates/peitho-core/src/parser.rs` | Capture only byte-verbatim editable inline ranges. | `domain.rs`; renderer Markdown options. | 1 |
| Modify | `crates/peitho-core/src/render.rs` | Own preview-sensitive HTML emission. | Tasks 1 and 9; `domain.rs` provenance. | 2, 10 |
| Create | `crates/peitho-core/src/slide_edit.rs` | Splice a block and enforce the structural postcondition. | Task 1; parser and phase APIs. | 4 |
| Create | `crates/peitho-core/src/sync.rs` | Own the serialized and generated `/sync` GET contract. | Existing serde/ts-rs export convention. | 7 |
| Modify | `crates/peitho-core/src/lib.rs` | Expose the new core modules and public types. | `render.rs`, `slide_edit.rs`, `sync.rs`. | 2, 4, 7 |
| Modify | `crates/peitho-core/tests/code_images.rs` | Keep direct code-image renders annotation-free. | Task 2 render signature. | 2 |
| Create | `crates/peitho/src/snapshots/peitho__tests__edit_annotations_off_example_slide_hashes.snap` | Freeze every example deck's current slide HTML. | Task 2 deterministic build harness. | 2 |
| Modify | `crates/peitho/src/server.rs` | Own preview server coordination state. | Tasks 4 and 7 core APIs. | 5-7 |
| Modify | `crates/peitho/src/main.rs` | Orchestrate preview application services. | Tasks 2, 4, 6, and 7 APIs. | 2, 3, 5, 6, 8 |
| Modify | `crates/peitho/tests/build.rs` | Prove build output remains annotation-free. | Task 3 CLI wiring. | 3 |
| Modify | `crates/peitho/tests/publish.rs` | Reject each preview attribute from distributions. | Task 3 publish validator. | 3 |
| Create | `bindings/SyncResponse.ts` | Commit the generated absolute sync response. | `peitho-core/src/sync.rs`. | 7 |
| Create | `bindings/SyncTimerSnapshot.ts` | Commit the generated sync timer member. | `peitho-core/src/sync.rs`. | 7 |
| Modify | `packages/peitho-present/src/sync.ts` | Validate and forward absolute `buildError` state. | Task 7 generated bindings. | 7 |
| Modify | `packages/peitho-present/src/preview.ts` | Own preview shell interaction state. | Tasks 3, 6, and 7 server/render contracts. | 8-10 |
| Modify | `packages/peitho-present/test/generated.test.ts` | Type-check the generated sync contract. | Task 7 bindings. | 7 |
| Modify | `packages/peitho-present/test/sync.test.ts` | Prove absolute build-error replay. | Task 7 sync client. | 7 |
| Modify | `packages/peitho-present/test/preview.test.ts` | Regress the complete preview interaction lifecycle. | Tasks 7-10 preview and sync clients. | 7-10 |
| Modify | `packages/peitho-present/dist/shell.js` | Commit the presenter bundle rebuilt from shared sync code. | Task 10 package build. | 10 |
| Modify | `packages/peitho-present/dist/preview.js` | Commit the rebuilt preview bundle. | Task 10 package build. | 10 |
| Modify | `packages/peitho-present/dist/remote.js` | Commit the remote bundle rebuilt from shared sync code. | Task 10 package build. | 10 |
| Modify | `CLAUDE.md` | Record the repository-level edit invariants. | Tasks 1-10 final behavior. | 11 |
| Modify | `site/content/guide/cli.md` | Document the author-facing preview workflow. | Tasks 1-10 final behavior. | 11 |
| Create (ignored) | `target/preview-inline-edit-e2e/deck.md` | Anchor the include-only browser fixture. | Task 12 checklist. | 12 |
| Create (ignored) | `target/preview-inline-edit-e2e/included.md` | Hold every browser-edited slide byte. | Task 12 top deck. | 12 |
| Create (ignored) | `target/preview-inline-edit-e2e/css/base.css` | Mirror the production base theme for browser proof. | `themes/base.css`. | 12 |
| Create (ignored) | `target/preview-inline-edit-e2e/css/overrides.css` | Trigger and recover the keyed-CSS failure. | Task 12 included keys. | 12 |

## Goal

In `peitho preview` single mode, make a parser-approved rendered paragraph,
heading, tight list item, or table cell swap in place to its exact inline
Markdown bytes. Commit through `POST /slide-edit`, re-read and reparse the deck,
reject stale or structural rewrites, translate the exact block span to its
origin file, and let the normal watcher rebuild. Markdown remains the only
content model; no HTML-to-Markdown conversion is introduced. Notes and block
edits serialize through one deck-writer mutex and one origin-write function.

## Verified existing seams

- `SourceFragment` is moved unchanged through mapping/checking and is rebuilt
  only by `try_map_image_src_inner`; copying new provenance fields there is
  sufficient for Parsed -> Mapped -> Checked propagation. The phase structs do
  not need parallel span fields.
- `source_slice` currently calls `trim()` at
  `crates/peitho-core/src/parser.rs:3552-3554`. Span capture must calculate the
  same leading/trailing trim offsets before attaching absolute provenance; it
  cannot infer them later from the stored `String`.
- Container fragments retain the original list, blockquote, or table Markdown
  and the renderer reparses it with `BODY_MARKDOWN_OPTIONS`. Offset iteration
  and `BodyMarkdownSource.range` already meet at
  `crates/peitho-core/src/render.rs:1027-1065`; the annotation planner can map
  joined offsets back to one source fragment without text matching.
- Revealed roots and revealed list items currently manufacture opening tags at
  `crates/peitho-core/src/render.rs:853-985`. Edit and reveal attributes must be
  merged at one opening-tag helper so neither path overwrites the other.
- `write_preview_note` already contains the complete LineMap -> origin read ->
  drift check -> CRLF conversion -> atomic write tail at
  `crates/peitho/src/main.rs:2133-2192`. That exact tail is the extraction point;
  the slide-edit path must not grow a second copy.
- `PresentServer` currently owns `Arc<Mutex<NotesWriter>>` at
  `crates/peitho/src/server.rs:419-440`, and `respond_notes_post` locks it at
  `crates/peitho/src/server.rs:1195-1200`. Replacing that field with one
  `Arc<Mutex<DeckWriter>>` gives both write routes the required ordering seam.
- Preview already centralizes navigation in `commitTransition`, saves on
  `pagehide`, and destroys shell listeners. The edit state belongs in that
  controller; slide HTML remains unaware of the shell, preserving the section
  16 event boundary.

## Tasks

### Task group 1: Parser `EditableSpan` capture

#### Task 1: Prove and encode the verbatim editable-span invariant

**Goal.** Make the parser the sole producer of opaque editable ranges, prove
that each range is exactly a substring of the Markdown the renderer reparses,
and carry those ranges through Checked without making transformed content
editable.

All requested editable block contexts can uphold the invariant with
pulldown-cmark 0.13 offsets: paragraph, ATX heading, setext heading, tight item,
loose-item paragraph, nested parent/child item, table header/data cell,
blockquote paragraph, explicit slot child, reveal child, CJK content, and an
included-file block. None of those contexts needs exclusion. An empty inline
run has no non-empty first/last event range and therefore gets no span.

**Files.**

- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/phase.rs`
- `crates/peitho-core/src/parser.rs`

**Test (Red).** Add
`parser::tests::editable_spans_are_verbatim_renderer_inputs_in_all_supported_contexts`
as the first new test. Its table must contain every context below and assert
both the expected source slices and the invariant for every emitted span:

```rust
if let Some(fragment_span) = fragment.source_span() {
    for editable in fragment.editable_spans() {
        let span = editable.source_span();
        let local = span.start - fragment_span.start;
        let renderer_markdown = &fragment.markdown()[local..local + (span.end - span.start)];
        assert_eq!(&combined[span.start..span.end], renderer_markdown, "{case_name}");
    }
} else {
    assert!(fragment.editable_spans().is_empty(), "{case_name}");
}
```

The table asserts these exact cases rather than merely counting spans:

- paragraph: `Plain *paragraph* with \`code\` and [link](https://example.com).`;
- ATX heading: `ATX **heading**`, excluding `## ` and closing ` ##`;
- setext heading: `Setext _heading_`, excluding the underline;
- tight and nested list source `- parent *one*\n  - child\n    - grandchild`:
  `parent *one*`, `child`, and `grandchild`, excluding markers and child lists;
- loose list source `- loose a\n\n- loose b`: paragraph spans `loose a` and
  `loose b`, with no tight-item span on either `<li>`;
- table source `| Name | Value |\n| --- | --- |\n| 日本 | **二** |`: `Name`,
  `Value`, `日本`, and `**二**`, excluding pipes and cell padding;
- blockquote source `> quoted *one*\n> quoted two`: the contiguous paragraph
  slice `quoted *one*\n> quoted two`, retaining its internal continuation marker;
- `::: {slot=body}\n\nslot *text*\n\n:::` and
  `::: {reveal}\n\nreveal **text**\n\n:::`: spans `slot *text*` and
  `reveal **text**` on real children, never on a synthetic wrapper;
- CJK: `日本語の **文章** です` with byte, not character, offsets.

Add `parser::tests::included_editable_span_uses_combined_source_coordinates`.
Expand a top deck containing `<!-- {"include":"shared.md"} -->`, parse the
expanded string, assert the span slices `共有 **本文**`, and assert
`LineMap::translate_span` names `shared.md`. Prefix the top deck with a UTF-8
BOM and assert every recorded offset indexes the include-expanded, BOM-stripped
string rather than the origin bytes.

Add `parser::tests::non_verbatim_or_non_text_fragments_have_no_editable_span`.
It asserts no spans for code, pure images, the `Footnotes` fragment, page
settings, empty inline blocks, and the synthetic `SlotGroup` wrapper. Raw HTML
already fails parsing at `parser.rs:6030-6042`, so retain that rejection rather
than inventing a non-editable fragment. Also keep the existing mixed-image
parse failure explicit:

```rust
assert!(image_fragment.editable_spans().is_empty());
assert!(parse_markdown("# T\n\nprefix ![x](x.png)", &highlighter).is_err());
```

Finally, map, check, and resolve a source-backed fixture and assert
`editable_spans_survive_parsed_mapped_and_checked` by comparing the exact
`EditableSpan` values before and after each phase. These tests fail first
because `EditableSpan`, fragment source provenance, and their accessors do not
exist.

**Implementation (Green).** Add a public opaque span with only a crate-visible
constructor and kind:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditableSpan {
    source: SourceSpan,
    kind: EditableBlockKind,
}

impl EditableSpan {
    pub(crate) fn new(source: SourceSpan, kind: EditableBlockKind) -> Self;
    pub fn source_span(self) -> SourceSpan;
    pub(crate) fn kind(self) -> EditableBlockKind;
}
```

Use the closed crate-private enum `EditableBlockKind::{Paragraph, Heading,
TightListItem, TableCell}`. Add `source_span: Option<SourceSpan>` and
`editable_spans: Vec<EditableSpan>` to `SourceFragment`; public constructors
initialize them to `None`/empty, parser-only `with_source_provenance` attaches
them, and `try_map_image_src_inner` copies both fields recursively. Add
`ParsedSlide::editable_spans() -> Vec<EditableSpan>` as the sole recursive
collector across `SlotGroup` children.

Replace `source_slice` at fragment construction sites with a helper returning
the trimmed text and its exact absolute `SourceSpan`. Reparse that exact text
with `BODY_MARKDOWN_OPTIONS`, walk offset events with a stack, and collect the
first-through-last inline range for headings, paragraphs, direct inline runs of
tight items, and table cells. Convert local offsets only after this guard:

```rust
if combined.get(fragment_span.start..fragment_span.end) == Some(fragment.markdown()) {
    fragment = fragment.with_source_provenance(fragment_span, editable_spans);
}
```

Paragraphs inside loose lists and blockquotes are identified by their
`Start(Paragraph)` events. A direct inline run under `Start(Item)` is a tight
item; stop it before the first nested block. Slot children keep their own
provenance; reveal groups dissolve without changing it. Do not attach
provenance to parser-synthesized or transformed fragments.

**Verification.**

```sh
cargo test -p peitho-core editable_spans_are_verbatim_renderer_inputs_in_all_supported_contexts
cargo test -p peitho-core included_editable_span_uses_combined_source_coordinates
cargo test -p peitho-core non_verbatim_or_non_text_fragments_have_no_editable_span
cargo test -p peitho-core editable_spans_survive_parsed_mapped_and_checked
test "$(rg -l 'EditableSpan::new' crates/peitho-core/src --glob '*.rs')" = "crates/peitho-core/src/parser.rs"
! rg -q 'EditableSpan' bindings
```

### Task group 2: Preview-only render annotations

#### Task 2: Render parser-authorized annotations and pin `Off` bytes

**Goal.** Make annotation choice explicit, emit source metadata only when a
renderer event exactly matches a parser-authorized span, support every rendered
element shape, and freeze every example deck's rendered slide HTML under `Off`.

**Files.**

- `crates/peitho-core/src/render.rs`
- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/tests/code_images.rs`
- `crates/peitho/src/main.rs`
- `crates/peitho/src/snapshots/peitho__tests__edit_annotations_off_example_slide_hashes.snap`

**Test (Red).** Before changing rendering, add the `main.rs` unit test
`edit_annotations_off_example_slide_hashes`. It discovers every
`examples/*/deck.md`, recursively copies each example beside an isolated temp
cache, runs the real load/transform/map/check/render pipeline with deterministic
test implementations of `SvgRunner`, `EmbedRenderer`, and `OEmbedFetcher`,
sorts every rendered slide by paths such as
`minimal/slides/000-arch-1.html`, and snapshots a compact
SHA-256 index:

```rust
let line = format!("{}  {}\n", hex_digest(&bytes), relative_slide.display());
digest_index.push_str(&line);
insta::assert_snapshot!("edit_annotations_off_example_slide_hashes", digest_index);
```

Review and commit that snapshot from the pre-feature renderer. Then add failing
renderer tests which refer to the not-yet-defined `EditAnnotations::On` and
`EditAnnotations::Off`:

- `edit_annotations_on_marks_paragraphs_atx_setext_and_blockquotes`;
- `edit_annotations_on_marks_tight_li_but_loose_li_paragraph`;
- `edit_annotations_on_marks_nested_items_and_table_cells`;
- `edit_annotations_on_marks_slot_and_reveal_children_without_losing_reveal`;
- `edit_annotations_on_wraps_each_accepts_inline_heading_fragment`;
- `edit_annotations_escape_markdown_attribute_once`;
- `edit_annotations_encode_crlf_and_lf_for_dom_attribute_round_trip`;
- `edit_annotations_off_emits_no_attributes_or_title_wrapper`.

The essential assertions use the parser's range rather than guessed numbers:

```rust
let span = parsed.parsed_slides()[0].editable_spans()[0].source_span();
assert!(html.contains(&format!(r#"data-peitho-src="{}-{}""#, span.start, span.end)));
assert!(html.contains(r#"data-peitho-md="A &quot;quote&quot; &amp; **mark**""#));
assert!(html.contains(r#"data-peitho-md="first&#13;&#10;second""#));
assert!(!off_html.contains("data-peitho-src"));
assert!(!off_html.contains("data-peitho-md"));
```

Assert attributes directly on `<p>`, `<h1>` through `<h6>`, `<th>`, `<td>`,
and tight `<li>`. Assert loose `<li>` is unannotated while its `<p>` is
annotated. Assert an `Accepts::Inline` title slot retains its existing outer
`.slot-title` span and gains exactly one inner annotation span per heading
fragment only under `On`. The tests fail first because the enum, argument, and
attributes do not exist.

**Implementation (Green).** Add the explicit option and require it at the
render boundary:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAnnotations { Off, On }

pub fn render_deck(
    deck: Deck<Checked<ResolvedImagePath>>,
    highlighter: &Highlighter,
    theme_css: String,
    edit_annotations: EditAnnotations,
) -> Result<Deck<Rendered>>;
```

Extend `BodyMarkdownFragment` with the fragment's absolute source span and
authorized editable spans. Build one annotation plan from the same
`into_offset_iter()` events that will be rendered. For a candidate block,
calculate its absolute inline range with
`fragment_span.start + (event_offset - BodyMarkdownSource.range.start)` and
emit metadata only when both range and `EditableBlockKind` exactly match an
existing `EditableSpan`. Slice `data-peitho-md` from the source-backed fragment
and escape it with one dedicated double-quoted-attribute encoder. Encode `&`,
`"`, and `<` once, encode carriage return as `&#13;`, and encode line feed as
`&#10;`; numeric line-ending references prevent the HTML parser from
normalizing CRLF before `getAttribute()` returns the drift-guard `old` value.

Refactor revealed root/list opening tags and ordinary Markdown starts through
one opening-tag helper that merges `data-reveal-step` with optional edit
attributes. Under `Off`, preserve the existing event stream and exact opening
tag bytes. For inline title slots, change the heading helper to receive the
whole `SourceFragment`; only `On` adds the per-fragment wrapper.

Every direct renderer call in core tests and `crates/peitho-core/tests/code_images.rs`
passes `EditAnnotations::Off`. Update the compile-fail doctest in `lib.rs` to
pass `Off` as well, so it still fails on `RawImagePath` rather than argument
count. Update the existing `build_artifacts` renderer
call in `crates/peitho/src/main.rs` to pass `Off` as the compile-safe baseline;
Task 3 splits out the sole `On` path. Re-run the example hash snapshot without
accepting any changed digest. Factor the existing build pipeline behind an
internal generic `build_artifacts_with_services` so the snapshot uses fixed
SVG/PNG/oEmbed payloads and never invokes Graphviz, Chrome, or the network;
production `build_artifacts` keeps the existing CLI services.

**Verification.**

```sh
cargo test -p peitho-core edit_annotations
cargo test -p peitho-core --test code_images
cargo test -p peitho-core --doc
cargo test -p peitho --bin peitho edit_annotations_off_example_slide_hashes
```

#### Task 3: Make preview the only `On` caller and gate contamination

**Goal.** Keep build, build-watch, present, PDF, lint, and every distributed
artifact byte-clean while preview generations receive annotations.

**Files.**

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/build.rs`
- `crates/peitho/tests/publish.rs`

**Test (Red).** Add these failing CLI integration and `main.rs` seam tests:

- `preview_cache_slide_fragments_contain_edit_annotations` asserts both
  attributes in `.peitho/preview-cache/build-0/slides/*.html`;
- `build_distribution_omits_preview_edit_annotations` asserts neither
  attribute in `dist/slides/*.html`;
- `build_watch_distribution_omits_preview_edit_annotations` drives one watch
  rebuild and makes the same assertion on its replacement slide files;
- `non_preview_rendered_documents_omit_preview_edit_annotations` checks the
  present-cache slide, PDF HTML, and lint HTML built from an `Off` deck;
- `publish_rejects_preview_edit_source_span_attribute` injects
  `data-peitho-src="1-4"` into a valid slide and expects failure naming the
  attribute and slide file;
- `publish_rejects_preview_edit_markdown_attribute` independently injects
  `data-peitho-md="raw"` and expects the same class of failure.

Place the preview-cache, watch, and non-preview-emitter seam tests in
`main.rs`; place the build distribution assertion in `tests/build.rs` and the
two contamination cases in `tests/publish.rs`.

```rust
for forbidden in ["data-peitho-src", "data-peitho-md"] {
    assert!(!distribution_slide.contains(forbidden));
    assert!(preview_slide.contains(forbidden));
}
```

The positive preview test and both injected-contamination tests fail first;
the negative output tests initially pass and become regression guards while
the preview path is split from the default renderer.

**Implementation (Green).** Preserve the existing default build seam and add a
preview-specific wrapper:

```rust
fn build_artifacts(input: &Path) -> miette::Result<BuildArtifacts> {
    build_artifacts_with_edit_annotations(input, peitho_core::EditAnnotations::Off)
}

fn build_preview_artifacts(input: &Path) -> miette::Result<BuildArtifacts> {
    build_artifacts_with_edit_annotations(input, peitho_core::EditAnnotations::On)
}
```

Only `emit_initial_preview_root` and `rebuild_preview_once_for_watch` call
`build_preview_artifacts`. Build, build-watch, present, export PDF, and lint
continue through `build_artifacts`, so their rendered deck is `Off` before any
document emitter sees it.

Add `reject_preview_edit_annotations(dist)` to `validate_publish_dist`. Scan
every regular `.html` file under `dist/` as bytes, reject either exact ASCII
needle, and report the relative file and offending attribute. Run this after
the required-file checks and before invoking the user's publish command.

**Verification.**

```sh
cargo test -p peitho --test build build_distribution_omits_preview_edit_annotations
cargo test -p peitho --bin peitho preview_cache_slide_fragments_contain_edit_annotations
cargo test -p peitho --bin peitho build_watch_distribution_omits_preview_edit_annotations
cargo test -p peitho --bin peitho non_preview_rendered_documents_omit_preview_edit_annotations
cargo test -p peitho --test publish publish_rejects_preview_edit_source_span_attribute
cargo test -p peitho --test publish publish_rejects_preview_edit_markdown_attribute
```

### Task group 3: Pure `slide_edit` rewrite and postcondition

#### Task 4: Implement `rewrite_block` with one test per refusal

**Goal.** Splice only an authorized inline byte range and return a candidate
only when reparsing proves that the requested text edit caused no structural
change.

**Files.**

- `crates/peitho-core/src/slide_edit.rs`
- `crates/peitho-core/src/lib.rs`

**Test (Red).** Create `slide_edit::tests` before the module exists. Acceptance
tests cover adding/removing syntax across an emphasis boundary, link and
inline-code paragraph edits, newline-only edge
normalization, byte-identical no-op, CJK bytes, a footnote-reference edit, and
a `breaks: true` paragraph whose internal newline remains one real newline and
one paragraph after reparsing. Put a second slide with explicit layout and note
metadata after that paragraph to prove shifted diagnostic line numbers alone do
not trigger a refusal.
Pin the two key rules separately:

```rust
let derived = rewrite_heading("# Before\n", "After").unwrap();
assert_eq!(parse(&derived).parsed_slides()[0].key.as_str(), "after");

let explicit = rewrite_heading("<!-- {\"key\":\"fixed\"} -->\n# Before\n", "After").unwrap();
assert_eq!(parse(&explicit).parsed_slides()[0].key.as_str(), "fixed");
```

Add one named test for every postcondition refusal:

- `rejects_original_parse_failure`;
- `rejects_candidate_parse_failure_with_the_parser_diagnostic`;
- `rejects_slide_count_change`;
- `rejects_section_change`;
- `rejects_non_target_key_change`;
- `rejects_non_target_key_source_change`;
- `rejects_non_target_layout_request_change`;
- `rejects_non_target_skip_change`;
- `rejects_non_target_page_number_flag_change`;
- `rejects_non_target_notes_change`;
- `rejects_target_notes_change`;
- `rejects_target_layout_request_change`;
- `rejects_target_skip_change`;
- `rejects_target_page_number_flag_change`;
- `rejects_target_fragment_kind_change_inside_slot_group`;
- `rejects_target_heading_level_change`;
- `rejects_target_editable_span_count_change`;
- `rejects_target_reveal_step_count_change`;
- `rejects_target_key_change_when_either_key_source_is_explicit`.

Exercise parse/bounds failures and author-enterable structural text through
`rewrite_block`. Exercise comparator branches that a legal target-range splice
cannot directly produce by parsing explicit before/after fixtures and calling
the private `preserves_deck_for_block_edit(&before, &after, target_index)` from
the module tests; do not synthesize phase values.

Also pin every concrete structural example called out by the design with its
own test: `rejects_empty_block`, `rejects_list_marker_prefix`,
`rejects_heading_marker_prefix`, `rejects_blank_line_block_split`,
`rejects_slide_separator`, `rejects_comment_close`, and `rejects_div_fence`.
Add `rejects_removing_the_last_footnote_reference_with_the_parser_diagnostic`:
the current parser rejects the now-unused definition, and the shell needs no
footnote special case. Each test asserts the specific `BuildError.message`, not
a generic `is_err()`:

```rust
let err = rewrite_block(source, slide, span, "\n- split", &highlighter).unwrap_err();
assert_eq!(err.message, "inline edit would change the edited slide's block structure");
```

Add defense tests for a span not owned by the target slide, an invalid UTF-8
boundary/range, and a stale target slide. Harvest every defense-test token from
a parser fixture—use a different slide or source string to make it invalid—so
no test outside `parser.rs` calls `EditableSpan::new`. The suite fails first
because `slide_edit` does not exist.

**Implementation (Green).** Implement the public pure API with the opaque span
as its capability token:

```rust
pub fn rewrite_block(
    source: &str,
    target: &ParsedSlide,
    span: EditableSpan,
    new_markdown: &str,
    highlighter: &Highlighter,
) -> Result<String>;

fn preserves_deck_for_block_edit(
    before: &Deck<Parsed>,
    after: &Deck<Parsed>,
    target_index: usize,
) -> Result<()>;
```

Reparse `source` with freshly parsed frontmatter before trusting `target`.
Resolve `target.index` in that old deck and require its `key`, `source_index`,
`source_span`, and authorized-span set to match the supplied target; then
require `span` to occur in both target span sets. Validate ordered bounds and UTF-8 boundaries,
and normalize only edge newlines:

```rust
let replacement = new_markdown.trim_matches(|ch| matches!(ch, '\r' | '\n'));
```

Do not trim spaces or internal newlines. Return the original bytes when the
replacement equals the current slice. Otherwise splice once, parse old and new
with their own frontmatter, and call
`preserves_deck_for_block_edit(before, after, target.index)`.

Make the postcondition return a reason-bearing `BuildError`, not `bool`. Compare
slide count and `DeckSettings::sections`; for every non-target slide compare key,
`KeySource` discriminant, semantic layout-request name, skip, page-number flag,
and notes. On the target compare notes, layout-request name, skip, page-number
flag, recursive fragment shape (including `SlotGroup` children and heading
levels), editable block-kind sequence/count, and reveal step count. Permit a
changed target key only when both key sources are `Derived`; an explicit key
must remain identical. Source line numbers are diagnostic positions and are
not semantic equality fields because a permitted internal newline can shift
later lines.

Return the new parser diagnostic directly when the candidate does not parse.
Map every other mismatch to the exact reason asserted above, with the target
span's line and help directing structural edits to the Markdown editor.

**Verification.**

```sh
cargo test -p peitho-core slide_edit::tests
```

### Task group 4: One server deck writer and one origin-write seam

#### Task 5: Extract the shared origin writer and implement fresh slide rewrites

**Goal.** Revalidate browser coordinates against a fresh parse, run the pure
postcondition, and make the existing note writer plus a callable slide-edit
writer use the same LineMap/origin function before changing the server API.

**Files.**

- `crates/peitho/src/server.rs`
- `crates/peitho/src/main.rs`

**Test (Red).** Keep the current note writer entry point compiling, then add:

- `write_preview_slide_edit_reparses_and_rewrites_the_current_block`;
- `write_preview_slide_edit_rejects_missing_key_span_and_old_bytes_as_conflicts`;
- `write_preview_slide_edit_uses_range_when_the_same_old_text_occurs_twice`;
- `write_preview_slide_edit_maps_structural_refusal_to_unprocessable`;
- `write_preview_slide_edit_writes_only_the_included_origin_block`;
- `write_preview_slide_edit_rejects_a_synthetic_or_mixed_origin_block_span`;
- `write_preview_slide_edit_preserves_a_leading_origin_bom`;
- `write_preview_slide_edit_preserves_crlf`;
- `write_preview_slide_edit_does_not_run_code_image_commands`;
- `preview_origin_writer_is_shared_by_note_and_slide_edit_regressions`.

The included-file assertion proves block-span translation rather than a whole
slide write:

```rust
write_preview_slide_edit(
    &deck, &SlideKey::new("shared").unwrap(), start, end,
    "共有 **本文**", "共有 **更新**",
).unwrap();
assert_eq!(fs::read_to_string(&deck).unwrap(), top_source);
assert!(fs::read_to_string(&included).unwrap().contains("共有 **更新**"));
```

Retain and run every existing note include, BOM, CRLF, parse-only, highlighter,
conflict, refusal, I/O, no-op, and watcher-coalescing test. The new tests fail
before a slide-edit function and shared origin seam exist.

**Implementation (Green).** First rename the server error type, without
changing the existing notes-only mutex yet, to the final
`DeckWriteError::{Conflict, Unprocessable, Io}`; update the note writer aliases
and rename the main helpers to `classify_preview_deck_report`,
`preview_deck_conflict`, `preview_deck_drift_conflict`,
`preview_deck_span_conflict`, and `preview_deck_io`. Then extract the old note
writer's filesystem tail into one function:

```rust
fn write_preview_origin_rewrite(
    input: &Path,
    loaded: &LoadedDeckSource,
    scope: PreviewOriginRewriteScope,
    before: &str,
    after: &str,
) -> Result<(), server::DeckWriteError>;
```

`PreviewOriginRewriteScope` is a closed internal enum with
`NoteSlide(SourceSpan)` and `EditableBlock(SourceSpan)`. Here, `before` and
`after` are the complete old and candidate combined sources. The function
returns early when they are identical; verifies bytes outside the selected
scope are unchanged; translates that scope through `LineMap`; reads the origin;
converts the translated line/column span with `origin_span_to_range`; compares
the current origin bytes to the corresponding old combined bytes; derives the
rewritten end with checked signed length arithmetic; preserves a pure-CRLF
origin with `convert_bare_lf_to_crlf`; and calls `write_atomic` once.

Keep the established note behavior that permits `LineMap::translate_span` to
clip synthetic units only at a slide span's outer edges. For
`EditableBlock`, require the returned `OriginSpan.combined` to equal the whole
requested block span; clipping, an internal synthetic unit, or a mixed-file
span is a conflict naming the translated origin file. Translation failure,
disk drift, or invalid checked arithmetic is also `Conflict`; read/write
failures are `Io`.

Make `write_preview_note` call this function with
`PreviewOriginRewriteScope::NoteSlide(slide.source_span)`. The slide-edit path
passes `PreviewOriginRewriteScope::EditableBlock(span.source_span())`. Add the
final callable slide function:

```rust
fn write_preview_slide_edit(
    input: &Path, key: &SlideKey, start: usize, end: usize,
    old: &str, new: &str,
) -> Result<(), server::DeckWriteError>;
```

It performs exactly this fresh sequence:

```rust
let loaded = load_and_expand_deck_source(input)?;
let parsed = loaded.translate(parse_deck(
    loaded.source.as_str(), loaded.frontmatter.clone(), &highlighter,
)).map_err(classify_preview_deck_report)?;
let slide = parsed.parsed_slides().iter().find(|slide| slide.key == *key)
    .ok_or_else(|| preview_deck_drift_conflict(key))?;
let span = slide.editable_spans().into_iter()
    .find(|span| span.source_span() == SourceSpan { start, end })
    .ok_or_else(|| preview_deck_drift_conflict(key))?;
```

Require `combined[start..end] == old`; all key/span/old mismatches use one
"the deck changed on disk; reload and retry" conflict. Normalize the submitted
new text only through `rewrite_block`; translate its diagnostic through
`loaded.translate`, map that reason to 422, and pass the
exact editable span, not `slide.source_span`, to
`write_preview_origin_rewrite`. Neither write path calls the preview rebuild,
root swap, or sync broadcast; the existing filesystem watcher remains the sole
rebuild trigger.

**Verification.**

```sh
cargo test -p peitho --bin peitho write_preview_slide_edit
cargo test -p peitho --bin peitho preview_origin_writer_is_shared_by_note_and_slide_edit_regressions
cargo test -p peitho --bin peitho preview_notes_writer
test "$(rg -c 'write_atomic\(origin_path' crates/peitho/src/main.rs)" -eq 1
```

#### Task 6: Replace `NotesWriter` with one serialized `DeckWriter` and route

**Goal.** Dispatch the two proven main-layer operations through one closure
behind one mutex, with capability-gated endpoints and exact HTTP status mapping.

**Files.**

- `crates/peitho/src/server.rs`
- `crates/peitho/src/main.rs`

**Test (Red).** Convert existing note-route tests to the new request enum, then
add:

- `slide_edit_route_without_deck_writer_returns_404_before_parsing`;
- `slide_edit_route_rejects_content_type_malformed_missing_and_unknown_fields_with_400`;
- `slide_edit_route_passes_exact_request_to_deck_writer`;
- `slide_edit_route_maps_conflict_to_409`;
- `slide_edit_route_maps_unprocessable_to_422`;
- `slide_edit_route_maps_io_to_500`;
- `notes_and_slide_edits_share_one_deck_writer_mutex`;
- `preview_deck_writer_dispatches_note_and_slide_edit_to_the_same_origin_seam`.

The exact request assertion is:

```rust
assert_eq!(captured[0], DeckWrite::SlideEdit {
    key: SlideKey::new("intro").unwrap(),
    start: 120,
    end: 143,
    old: "Peitho is a *fast* tool".to_owned(),
    new: "Peitho is a **very fast** tool".to_owned(),
});
```

For the mixed concurrency test, issue concurrent `/notes` and `/slide-edit`
requests against a writer that increments an atomic in-flight counter, waits
50 ms, then decrements it; assert all responses are 200 and the observed
maximum is exactly 1. These tests fail while the server still owns a
notes-specific closure/mutex and has no slide-edit route.

**Implementation (Green).** Replace the notes-specific type and field with:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckWrite {
    Note { key: SlideKey, text: String },
    SlideEdit { key: SlideKey, start: usize, end: usize, old: String, new: String },
}

pub type DeckWriter = Box<dyn FnMut(DeckWrite) -> Result<(), DeckWriteError> + Send + 'static>;
```

`PresentServer` owns only
`deck_writer: Option<Arc<Mutex<DeckWriter>>>` and exposes only
`with_deck_writer`. Both routes check the capability before content type or
body parsing, parse `application/json` with `deny_unknown_fields`, clone the
same `Arc`, and lock the same mutex. Keep the request DTO private:

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SlideEditRequest { key: SlideKey, start: usize, end: usize, old: String, new: String }
```

Use one response mapper for both routes: `Conflict` -> 409,
`Unprocessable` -> 422, `Io` -> logged 500, and success ->
`{"saved":true}`. Keep malformed JSON, invalid keys/numbers, missing fields,
unknown fields, and wrong content type at 400.

Add the sole preview installer:

```rust
fn preview_deck_writer(input: PathBuf) -> server::DeckWriter {
    Box::new(move |request| match request {
        server::DeckWrite::Note { key, text } => write_preview_note(&input, &key, &text),
        server::DeckWrite::SlideEdit { key, start, end, old, new } =>
            write_preview_slide_edit(&input, &key, start, end, &old, &new),
    })
}
```

Remove `NotesWriter`, `with_notes_writer`, and `preview_notes_writer` rather
than retaining compatibility aliases. `preview()` installs exactly one
`preview_deck_writer`, so notes and slide requests cannot interleave from this
point onward.

**Verification.**

```sh
cargo test -p peitho server::tests::slide_edit_route
cargo test -p peitho server::tests::notes_and_slide_edits_share_one_deck_writer_mutex
cargo test -p peitho server::tests::notes_route
cargo test -p peitho --bin peitho preview_deck_writer
test "$(rg -c 'deck_writer: Option<Arc<Mutex<DeckWriter>>>' crates/peitho/src/server.rs)" -eq 1
! rg -q 'NotesWriter|preview_notes_writer|with_notes_writer' crates/peitho/src/{main,server}.rs
```

### Task group 5: Absolute `/sync` build errors and preview banner

#### Task 7: Establish the generated sync contract and server-owned state

**Goal.** Put the whole JSON GET shape under one Rust-owned contract, add
`buildError` to every JSON GET response, wake long pollers on failures, clear it
atomically with successful generations, and reject it on POST.

**Files.**

- `crates/peitho-core/src/sync.rs`
- `crates/peitho-core/src/lib.rs`
- `crates/peitho/src/server.rs`
- `bindings/SyncResponse.ts`
- `bindings/SyncTimerSnapshot.ts`
- `packages/peitho-present/src/sync.ts`
- `packages/peitho-present/test/generated.test.ts`
- `packages/peitho-present/test/sync.test.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (Red).** Add core serialization/export tests which assert camelCase and
the committed binding:

```rust
assert_eq!(json["buildError"], serde_json::Value::Null);
assert!(sync_ts.contains("buildError: string | null"));
assert!(sync_ts.contains("timer: SyncTimerSnapshot | null"));
```

Add server tests:

- `sync_response_body_always_includes_build_error` for both null and text;
- `sync_hub_build_error_wakes_waiting_poller_without_bumping_generation`;
- `sync_hub_successful_reload_clears_build_error_and_bumps_generation_atomically`;
- `sync_hub_regular_messages_preserve_build_error`;
- `sync_endpoint_rejects_build_error_post_body`.

Add TypeScript tests that type a complete `SyncResponse`, reject malformed
build-error values, and assert handshake plus later polls each forward
`{buildError: string | null}`. Update every complete `/sync` fixture in
`sync.test.ts` and `preview.test.ts` with `buildError: null`; absence is no
longer a valid complete server response. These tests fail because the contract
and state field do not exist.

**Implementation (Green).** Define ts-rs/serde contract types in
`peitho-core`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResponse {
    seq: u64,
    message: Option<serde_json::Value>,
    index: Option<usize>,
    step: Option<usize>,
    swapped: bool,
    generation: u64,
    session: String,
    timer: Option<SyncTimerSnapshot>,
    now_ms: u64,
    build_error: Option<String>,
}
```

Annotate JS integer fields as TypeScript `number`, message as `unknown`, and
nullable fields explicitly. `SyncTimerSnapshot` owns `running`, `elapsed_ms`,
and `at_ms` and derives `Copy` so snapshots retain their current value semantics.
Give `SyncResponse` a constructor used by the server, replace the
server-local `TimerSyncState` with `SyncTimerSnapshot`, and export both
generated files through the existing test/`ts-bindings` convention.

Add `build_error: Option<String>` to `SyncState` and `SyncSnapshot`.
`report_build_error(String)` sets it, increments `seq`, clears `latest`, and
notifies the condvar without changing `generation`. `broadcast_reload()` clears
it and increments `generation` and `seq` while holding the same lock, then
notifies once. Replace the local response struct with `SyncResponse`.

In TypeScript, import `SyncResponse` from `bindings/SyncResponse` and replace
the handwritten server shape with `Partial<SyncResponse>` only at defensive
runtime-validation boundaries. Import generated `SyncTimerSnapshot` as well
and retain the package's exported name with
`export type TimerSyncSnapshot = SyncTimerSnapshot`; this avoids changing the
public barrel in `src/index.ts`. Add:

```ts
export function isBuildErrorSyncMessage(
  value: unknown
): value is { buildError: string | null };
```

Deliver that absolute state from every JSON GET before generation replay.
Do not add it to the `SyncMessage` POST union; existing
`deny_unknown_fields` arms make `{"buildError":"x"}` a 400.

**Verification.**

```sh
cargo test -p peitho-core sync::tests
cargo test -p peitho server::tests::sync_
cd packages/peitho-present && npm test -- test/generated.test.ts test/sync.test.ts && npm run typecheck
git diff --check -- bindings/SyncResponse.ts bindings/SyncTimerSnapshot.ts
```

#### Task 8: Publish watch failures and render the last-good-page banner

**Goal.** Send failed watch diagnostics to the open preview immediately, clear
them on the next successful rebuild, and show them safely above the last good
generation.

**Files.**

- `crates/peitho/src/main.rs`
- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (Red).** Extend the recording reload target tests:

- `preview_watch_rebuild_failure_keeps_root_generation_and_reports_build_error`
  asserts no swap, no generation change, one `BuildError(full_diagnostic)`
  event, and unchanged last-good files;
- `preview_watch_success_after_failure_swaps_broadcasts_and_clears_error`
  asserts the new root is complete before the `Broadcast` event and that the
  server snapshot has `buildError: null` at the bumped generation.

Add preview tests:

- `build_error_message_renders_in_fixed_preformatted_banner`;
- `build_error_uses_text_content_and_cannot_inject_html`;
- `null_build_error_hides_and_clears_banner`;
- `build_error_does_not_replace_or_reload_last_good_slides`;
- `initial_sync_build_error_is_rendered_after_mount`.

```ts
channel.onmessage?.({ data: { buildError: "<b>broken</b>\nline 2" } });
expect(banner.textContent).toBe("<b>broken</b>\nline 2");
expect(banner.querySelector("b")).toBeNull();
expect(reload).not.toHaveBeenCalled();
```

They fail while failed rebuilds are terminal-only and the shell has no banner.

**Implementation (Green).** Extend `PreviewReloadTarget` with
`report_build_error(String)`. On the error arm of
`rebuild_preview_once_for_watch`, keep the existing terminal rendering, compute
the full unstyled headline-and-help text with `plain_diagnostic_text`, and
publish that text after logging. On success, keep ordering as emit complete ->
swap root -> `broadcast_reload`; the latter performs the atomic clear from
Task 7. Do not change `emit_initial_preview_root`: the standalone first-build
error page remains the existing generation-polling page.

Create one shell-owned banner with
`data-peitho-preview="build-error"`, `position: fixed`, a bounded scrollable
height, high z-index, and `white-space: pre-wrap`. It is hidden for null and
clears its `textContent`; for text it sets only `textContent`. Expose
`setBuildError(error: string | null)` and
`requestGenerationReload(generation: number, reload: () => void)` on the
`PreviewShell` interface, then generalize `installPreviewReload` to dispatch
both absolute messages:

```ts
if (isBuildErrorSyncMessage(event.data)) shell.setBuildError(event.data.buildError);
if (isGenerationSyncMessage(event.data)) {
  shell.requestGenerationReload(event.data.generation, reload);
}
```

Replace `fetchGeneration()` with a validated initial sync-state fetch so the
mount applies both `generation` and `buildError`. At this task's boundary,
`requestGenerationReload` returns when the value equals `shell.generation` and
otherwise retains today's immediate save-and-reload behavior; Task 10 adds
edit-aware deferral.

**Verification.**

```sh
cargo test -p peitho --bin peitho preview_watch_rebuild
cd packages/peitho-present && npm test -- test/preview.test.ts -t build_error
```

### Task group 6: Preview shell click-to-edit lifecycle

#### Task 9: Start, edit, cancel, and save an inline source block

**Goal.** Turn only a current single-mode slide's annotated block into a
plaintext editor, preserve links and nested list structure, and POST the exact
source-addressed request.

**Files.**

- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (Red).** Add a fixture whose fetched slide HTML contains an annotated
paragraph, annotated heading-wrapper span, tight annotated `<li>` with a nested
`<ul>`, and an annotated paragraph containing an `<a>`. Add tests:

- `inline_edit_click_uses_composed_path_inside_current_slide_shadow_root`;
- `inline_edit_grid_tile_thumbnail_and_non_current_roots_do_not_start`;
- `inline_edit_link_click_keeps_browser_behavior`;
- `inline_edit_editor_uses_plaintext_only_and_shows_data_peitho_md`;
- `inline_edit_tight_list_wraps_only_leading_inline_nodes`;
- `inline_edit_escape_restores_rendered_nodes_without_posting`;
- `inline_edit_shift_enter_inserts_one_source_newline`;
- `inline_edit_enter_and_blur_post_the_exact_request_once`;
- `inline_edit_ime_composition_and_keycode_229_do_not_commit`;
- `inline_edit_crlf_attribute_round_trips_exact_old_bytes`;
- `inline_edit_failed_save_stays_open_and_reports_json_error`;
- `inline_edit_unchanged_close_restores_without_posting`.

The request assertion is exact:

```ts
expect(JSON.parse(posts[0][1].body as string)).toEqual({
  key: "intro", start: 120, end: 143,
  old: "Peitho is a *fast* tool", new: "Peitho is a **very fast** tool"
});
```

For the CRLF case, parse fixture HTML containing
`data-peitho-md="first&#13;&#10;second"` through jsdom and assert the request's
`old` is exactly `"first\r\nsecond"`, not browser-normalized LF.

Dispatch clicks from inside the shadow tree with `{bubbles: true, composed:
true}` so a test that uses `event.target` instead of `composedPath()` fails.
Assert the link event is not prevented. These tests fail before edit state and
the endpoint client exist.

**Implementation (Green).** Add one nullable `ActiveSlideEdit` containing
`key`, `start`, `end`, `old`, target element, editor element, and the original
nodes needed for lossless cancel. A tile click may start editing only when mode
is `single`, the tile is `slides[currentIndex]`, its `composedPath()` reaches
that host's exact `ShadowRoot`, an element before that boundary has both edit
attributes, and no anchor occurs before the boundary.
While that state is non-null, ignore attempts to start a second editor. Retain
the existing `pointer-events: none` rule on thumbnail hosts so their copied
annotations are inert.

For ordinary blocks, detach and retain all child nodes, set the block's text to
decoded `data-peitho-md`, and set
`contenteditable="plaintext-only"`. For tight `<li>`, retain only nodes before
the first nested block, insert a shell-created inline `<span>` at that point,
and make the span editable; leave nested `<ul>`/`<ol>` nodes untouched. Apply a
visible outline without changing slide HTML source.

Parse `data-peitho-src` only with `^(\d+)-(\d+)$`, require safe integers with
`start < end`, and read `old` through `getAttribute("data-peitho-md")` so HTML
entity decoding happens exactly once. Dirty checks and the submitted `new`
value use only `editor.textContent`; no rendered child HTML is inspected or
converted.

Install editor-local key/blur handlers. Ignore a key when `isComposing` or
`keyCode === 229`. Escape prevents default/propagation and restores retained
nodes. Both Enter variants prevent default/propagation: unshifted Enter
commits, while Shift+Enter inserts a literal `"\n"` at the current
Selection/Range and keeps editing. Blur shares the same serialized commit
promise, preventing Enter-plus-blur duplicate POSTs.

Post JSON to `/slide-edit` with `Content-Type: application/json`. On success,
remove editability and keep the submitted Markdown text visible for the watcher
reload. On a non-2xx response, display its JSON `{error}` string, falling back
to ``slide edit failed (HTTP ${response.status})`` when that shape is absent;
on transport failure display ``failed to save slide edit: ${String(error)}``.
Keep the session and
focusable editor open. Generalize the notes status writer to source-keyed panel
errors so a successful note save cannot erase a slide-edit failure.

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/preview.test.ts -t inline_edit
cd packages/peitho-present && npm run typecheck
```

#### Task 10: Settle transitions, defer reloads, save on page exit, and clean up

**Goal.** Integrate slide edits with every transition/reload lifetime boundary,
so a failed save blocks movement and an external rebuild cannot discard an
open editor.

**Files.**

- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`
- `crates/peitho-core/src/render.rs`
- `packages/peitho-present/dist/shell.js`
- `packages/peitho-present/dist/preview.js`
- `packages/peitho-present/dist/remote.js`

**Test (Red).** Add:

- `commit_transition_saves_slide_edit_before_notes_and_navigation`;
- `failed_slide_edit_blocks_slide_change_and_grid_entry`;
- `pageup_pagedown_from_editor_wait_for_commit_before_navigation`;
- `generation_reload_is_deferred_while_edit_is_open`;
- `successful_commit_releases_deferred_reload`;
- `escape_cancel_releases_deferred_reload`;
- `drift_409_keeps_editor_open_until_escape_releases_reload`;
- `pagehide_posts_dirty_slide_edit_with_keepalive`;
- `pagehide_does_not_post_clean_or_cancelled_slide_edit`;
- `slide_edits_are_never_written_to_session_storage`;
- `destroy_removes_editor_tile_and_pagehide_listeners`;
- `installer_cleanups_remove_keyboard_and_sync_listeners`;
- `destroy_discards_deferred_reload_and_pending_edit_completion`;
- `preview_index_bootstrap_cleans_keyboard_when_mount_fails`;
- `preview_index_bootstrap_disposes_shell_keyboard_and_reload_handles`.

Use controlled promises to prove ordering:

```ts
shell.navigate("next");
expect(slideEditPosts()).toHaveLength(1);
expect(notesPosts()).toHaveLength(0);
expect(shell.currentIndex).toBe(0);
resolveSlideEdit(errorJson(409, "the deck changed on disk; reload and retry"));
await vi.waitFor(() => expect(shell.currentIndex).toBe(0));
```

For deferred reload, deliver a new generation, assert no reload, mutate the
open editor, return 409, assert no reload again, press Escape, then assert one
reload. Every test registers the shell in the existing `shells` array and every
keyboard/reload installation in `cleanups`; `afterEach` must leave neither a
shell nor global listener alive. These tests fail while transitions settle only
notes and reload is unconditional.

**Implementation (Green).** Make `commitTransition` use one ordered settlement
routine:

```ts
private async settleForTransition(): Promise<boolean> {
  if (!(await this.commitSlideEdit(false))) return false;
  while (!this.notesSettled()) {
    if (!(await this.flushNotes())) return false;
  }
  return true;
}
```

Keep the existing transition sequence token so stale async completions cannot
navigate after a later request or `destroy()`. A slide edit is always settled
first. A false result leaves index/mode unchanged and the panel visible.

Store at most one deferred reload callback. `requestGenerationReload` calls
`saveState()` and reloads immediately only when no `ActiveSlideEdit` exists;
otherwise it records the callback. Successful commit, unchanged close, or
Escape cancel clears the active session and invokes the deferred reload once.
A failed commit retains both the session and deferred reload.

On `pagehide`, snapshot a dirty active edit and call the same request encoder
directly with `keepalive: true`; do not put edit content into `PreviewState`.
Continue the existing notes page-exit path, allowing the server's one mutex to
serialize both requests.

Track every tile/editor listener with a cleanup closure and run them from
`destroy`; clear the active edit and deferred reload, and invalidate pending
transition completions without navigating. In `render_preview_index`, retain
the cleanup functions returned by `installPreviewKeyboard` and
`installPreviewReload`, then on page exit destroy
the shell and invoke both once; if mounting throws, invoke every cleanup already
acquired before calling `showError`. Rebuild all package entry points. Because
`sync.ts` is shared, commit the resulting `dist/shell.js`, `dist/preview.js`,
and `dist/remote.js`; do not hand-edit any bundle.

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/preview.test.ts
cd packages/peitho-present && npm run typecheck
cd packages/peitho-present && npm run build
cargo test -p peitho-core preview_index_bootstrap
cargo test -p peitho --bin peitho builtin_preview_shell_matches_committed_bundle
cargo test -p peitho --bin peitho builtin_remote_shell_matches_committed_bundle
git diff --check -- packages/peitho-present/dist/shell.js packages/peitho-present/dist/preview.js packages/peitho-present/dist/remote.js
```

### Task group 7: Documentation and real-Chrome acceptance

#### Task 11: Document the author contract and repository invariants

**Goal.** Explain exactly what can be edited, how commits/cancels/failures
behave, why Markdown remains authoritative, and why both write paths share one
seam.

**Files.**

- `CLAUDE.md`
- `site/content/guide/cli.md`

**Test (Red).** Run these acceptance searches before editing; each new phrase
must be absent and therefore fail:

```sh
rg -q 'inline Markdown source' site/content/guide/cli.md
rg -q 'Shift\+Enter.*newline' site/content/guide/cli.md
rg -q 'included file' site/content/guide/cli.md
rg -q 'one deck-writer mutex' CLAUDE.md
rg -q 'data-peitho-src.*data-peitho-md' CLAUDE.md
```

**Implementation (Green).** Add a dedicated “Editing slide text in preview”
subsection under `peitho preview` in the CLI guide. State this exact user model:

```text
Click a paragraph, heading, tight list item, or table cell in single view to
replace its rendering with its inline Markdown source. Enter or blur saves,
Shift+Enter inserts a newline, and Escape cancels. Markdown is the only source
of truth; Peitho never converts rendered HTML back to Markdown.
```

Document single-mode/current-slide scope, link behavior, inline syntax,
structural refusals, failed-save blocking, last-good build-error banner,
derived heading keys and keyed CSS, CRLF preservation, stale-tab 409 recovery,
and writes to an included file. Explicitly list non-editable code, generated
code images/math/embeds, pure images, footnote definitions, raw HTML, settings,
and layout HTML.

Add one new CLAUDE invariant bullet recording parser-only `EditableSpan`
authority, preview-only annotations, no HTML conversion, fresh-parse drift and
structural guards, absolute `buildError`, and the exact root-cause rule: notes
and slide edits use one deck-writer mutex and one origin-write function.

**Verification.**

```sh
make demo-site
git diff --check -- CLAUDE.md site/content/guide/cli.md
rg -q 'inline Markdown source' site/content/guide/cli.md
rg -q 'one deck-writer mutex' CLAUDE.md
```

#### Task 12: Complete the required real-Chrome acceptance checklist

**Goal.** Exercise renderer metadata, Shadow DOM event paths, Japanese IME,
server writes, include translation, watcher success/failure, and browser link
behavior together before claiming completion.

**Files.** These are reviewer-created ignored fixtures and are not committed:

- `target/preview-inline-edit-e2e/deck.md`
- `target/preview-inline-edit-e2e/included.md`
- `target/preview-inline-edit-e2e/css/base.css`
- `target/preview-inline-edit-e2e/css/overrides.css`

**Test (Red).** Treat the acceptance run as red until every box has recorded
real-Chrome evidence:

- [ ] Paragraph click shows exact `Peitho is a *fast* tool.` Markdown; changing
      `*fast*` to `**very fast**` and pressing Enter writes exactly those bytes
      and reloads at the same slide.
- [ ] Japanese IME conversion changes `編集します` to `編集できます`; the
      composing Enter does not commit, while a later non-composing Enter does.
- [ ] Editing `Derived Heading` to `Renamed Heading` changes its derived key and
      preserves the current preview position after reload.
- [ ] Editing `CSS Guard` changes its derived key, makes the old keyed selector
      fail validation, and shows the full fixed banner over the last good slide.
- [ ] Fixing the selector clears the banner on the successful generation.
- [ ] Every accepted edit changes `included.md`; top-level `deck.md` stays
      byte-identical.
- [ ] Command-clicking the rendered external link opens it in a new tab and
      never enters editing.

**Implementation (Green).** Create the ignored fixture with this exact top
deck:

```markdown
---
css: ./css
---
<!-- {"include":"included.md"} -->
```

Use this exact included deck:

```markdown
# Derived Heading

Peitho is a *fast* tool.

日本語の文章を編集します。

[Open Peitho docs](https://peitho.gosu.ke/)

---

# CSS Guard

Change this heading to trigger the keyed CSS guard.
```

Copy `themes/base.css` byte-for-byte to the fixture's `css/base.css`; create
`css/overrides.css` with:

```css
[data-slide-key="css-guard"] .slot-title { color: #38bdf8; }
```

Record the top deck's checksum, launch preview from the repository root with
browser auto-open disabled, and perform the checklist in real Google Chrome at
`http://localhost:6175/`. For the banner recovery step, change the `CSS Guard`
heading to `Broken CSS Guard`; after confirming the failed rebuild
remains visible, change the selector to
`[data-slide-key="broken-css-guard"]`. Compare the top checksum and inspect
the included file after every successful save. This is reviewer-owned browser
verification; no generated fixture or edited example enters the commit.

**Verification.**

```sh
test "$(git check-ignore target/preview-inline-edit-e2e/deck.md target/preview-inline-edit-e2e/included.md target/preview-inline-edit-e2e/css/base.css target/preview-inline-edit-e2e/css/overrides.css | wc -l | tr -d ' ')" -eq 4
shasum -a 256 target/preview-inline-edit-e2e/deck.md > /tmp/peitho-preview-inline-edit-deck.sha256
# Terminal 1:
cargo run -p peitho -- preview target/preview-inline-edit-e2e/deck.md --port 6175 --no-open
# Terminal 2 (macOS):
open -a "Google Chrome" http://localhost:6175/
shasum -a 256 -c /tmp/peitho-preview-inline-edit-deck.sha256
git status --short -- examples target/preview-inline-edit-e2e
```

## Summary

<!-- derived-from #file-map -->
<!-- derived-from #tasks -->

The dependency order is parser provenance -> preview-only rendering -> pure
structural rewrite -> single serialized server writer -> shared origin write ->
absolute rebuild-error state -> shell lifecycle -> documentation and real
Chrome proof. The design never trusts DOM coordinates on save, never derives
Markdown from HTML, and never creates a second filesystem rewrite seam.

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
