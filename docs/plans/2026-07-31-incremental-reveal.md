# Incremental reveal fragments

<!-- constrained-by ../specs/2026-07-31-incremental-reveal-design.md -->

Issue: #290
Date: 2026-07-31
Branch: `incremental-reveal`

Invariants from the approved design:

- `::: {reveal}` dissolves during parse into `Option<RevealSpan>` on `SourceFragment`; no `FragmentKind::Reveal` is introduced.
- `ParsedSlide.step_count` is the only step-count source and is copied through `MappedSlide`, `CheckedSlide`, and `ManifestSlide.revealSteps`.
- Rendered HTML is fully visible; only `packages/peitho-present/src/shell.ts` toggles `data-reveal-hidden`.
- Sync transports absolute `{ index, step }` messages only; index and step never ride separate channel messages.

## Task 1: fenced-div attribute grammar and reveal validation

**Goal**: Accept bare `::: {reveal}` and reject invalid reveal fence shapes with line-numbered `BuildError` help.

**Files**: `crates/peitho-core/src/parser.rs`

**Test**:

```rust
// crates/peitho-core/src/parser.rs #[cfg(test)]
#[test]
fn reveal_fence_validation_errors_are_line_numbered() {
    let err = parse_markdown(
        "# T\n\n::: {reveal=group}\n\nx\n\n:::\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(3));
    assert_eq!(err.message, "reveal fence values are reserved for future syntax");
    assert_eq!(err.help, "use bare `::: {reveal}` for incremental reveal groups");

    let err = parse_markdown(
        "# T\n\n::: {reveal}\n\n:::\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(3));
    assert_eq!(err.message, "empty reveal fence");
    assert_eq!(
        err.help,
        "add content between the opening and closing `:::`, or remove the fence",
    );

    let err = parse_markdown(
        "# T\n\n::: {slot=body reveal}\n\nx\n\n:::\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Parse);
    assert!(err.to_string().contains("accepts only one attribute"));

    let err = parse_markdown(
        "# T\n\n::: {reveal}\n\n::: {slot=body}\n\nx\n\n:::\n\n:::\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Parse);
    assert!(err.to_string().contains("nested"));
}

#[test]
fn reveal_groups_are_allowed_on_draft_and_skip_slides() {
    let skipped = parse_markdown(
        "<!-- {\"skip\":true} -->\n# T\n\n::: {reveal}\n\nx\n\n:::\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap();
    assert!(skipped.parsed_slides()[0].skip);

    let draft = parse_markdown(
        "<!-- {\"draft\":true} -->\n# Draft\n\n::: {reveal}\n\nx\n\n:::\n\n---\n# Live",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap();
    assert_eq!(draft.parsed_slides().len(), 1);
    assert_eq!(draft.parsed_slides()[0].key.as_str(), "live");
}
```

**Implementation**:

- Replace `SlotDivMarker::Open(ExplicitSlot)` with a private `DivMarker::Open(DivOpen)` where `DivOpen` is `Slot(ExplicitSlot)` or `Reveal`.
- Extend `scan_slot_div_markers` to keep the existing line-first scan, fenced-code exclusion, leading-space behavior, and four-colon rejection.
- Change `parse_slot_div_attributes(rest, line)` to return `DivOpen`; accept exactly `{slot=name}` or `{reveal}`.
- Keep existing multi-attribute, nested, unclosed, unmatched-close, and four-colon errors on the same parser paths.
- Add the new `{reveal=value}` and empty reveal-group errors in the marker close handling branch.
- Do not change `parse_page_comment`; use the existing `draft` and `skip` parser tests as the precedent proving those page flags can coexist with reveal groups.

**Verify**:

```sh
cargo test -p peitho-core parser::tests::reveal_fence_validation_errors_are_line_numbered
cargo test -p peitho-core parser::tests::reveal_groups_are_allowed_on_draft_and_skip_slides
```

## Task 2: parse-time RevealSpan annotation and step counting

**Goal**: Dissolve reveal groups into ordinary fragments with `RevealSpan { start, len }` and compute `ParsedSlide.step_count` once during parse.

**Files**: `crates/peitho-core/src/domain.rs`, `crates/peitho-core/src/parser.rs`

**Test**:

```rust
// crates/peitho-core/src/parser.rs #[cfg(test)]
use crate::domain::RevealSpan;

#[test]
fn reveal_group_dissolves_into_fragment_spans_and_counts_list_items() {
    let deck = parse_markdown(
        "# T\n\n::: {reveal}\n\nIntro paragraph.\n\n- one\n- two\n  - child\n- three\n\n```rust\nfn main() {}\n```\n\n:::\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap();

    let slide = &deck.parsed_slides()[0];
    assert_eq!(slide.step_count, 5);
    assert!(!slide.fragments.iter().any(|f| matches!(f.kind(), FragmentKind::SlotGroup { .. })));
    assert_eq!(slide.fragments[0].reveal_span(), None);
    assert_eq!(slide.fragments[1].reveal_span(), Some(RevealSpan { start: 1, len: 1 }));
    assert_eq!(slide.fragments[2].reveal_span(), Some(RevealSpan { start: 2, len: 3 }));
    assert_eq!(slide.fragments[3].reveal_span(), Some(RevealSpan { start: 5, len: 1 }));
}

#[test]
fn reveal_steps_do_not_change_slide_indices_or_sections() {
    let deck = parse_markdown(
        "---\ntime: 2m\n---\n<!-- {\"section\":\"One\",\"time\":\"1m\"} -->\n# One\n\n::: {reveal}\n\n- a\n- b\n\n:::\n\n---\n<!-- {\"section\":\"Two\",\"time\":\"1m\"} -->\n# Two",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap();

    assert_eq!(deck.parsed_slides()[0].index, 0);
    assert_eq!(deck.parsed_slides()[1].index, 1);
    assert_eq!(deck.parsed_slides()[0].step_count, 2);
    assert_eq!(deck.settings().sections()[0].start(), 0);
    assert_eq!(deck.settings().sections()[0].end(), 0);
    assert_eq!(deck.settings().sections()[1].start(), 1);
    assert_eq!(deck.settings().sections()[1].end(), 1);
}
```

**Implementation**:

- Add `pub struct RevealSpan { pub start: usize, pub len: usize }` in `domain.rs` with `Debug`, `Clone`, `Copy`, `PartialEq`, and `Eq`.
- Add `reveal_span: Option<RevealSpan>` to `SourceFragment<S>`, initialize it to `None` in every constructor, copy it in `try_map_image_src_inner`, and expose `pub fn reveal_span(&self) -> Option<RevealSpan>`.
- Add `pub(crate) fn with_reveal_span(mut self, span: RevealSpan) -> Self` on `SourceFragment<RawImagePath>`.
- Track `next_reveal_step: usize` in `parse_slide`, starting at `1`, and set `step_count` to `next_reveal_step - 1`.
- Store reveal-group children in the same stack frame shape as slot groups, but on close push each child directly to the outer fragment list after assigning its span; never construct `FragmentKind::SlotGroup` for reveal.
- Implement `fn reveal_span_len(fragment: &SourceFragment) -> usize` in `parser.rs`: return `1` for non-list fragments; for `FragmentKind::List`, run `Parser::new_ext(fragment.markdown(), Options::ENABLE_OLD_FOOTNOTES)` and count `Event::Start(Tag::Item)` while list depth is `1`.

**Verify**:

```sh
cargo test -p peitho-core parser::tests::reveal_group_dissolves_into_fragment_spans_and_counts_list_items
cargo test -p peitho-core parser::tests::reveal_steps_do_not_change_slide_indices_or_sections
```

## Task 3: phase threading from Parsed to Checked

**Goal**: Copy `step_count` through `ParsedSlide`, `MappedSlide`, and `CheckedSlide` exactly like `skip`, while leaving mapping and `Accepts` validation unchanged.

**Files**: `crates/peitho-core/src/phase.rs`, `crates/peitho-core/src/mapping.rs`, `crates/peitho-core/src/check.rs`

**Test**:

```rust
// crates/peitho-core/src/mapping.rs #[cfg(test)]
#[test]
fn mapping_carries_reveal_step_count_and_cross_slot_spans() {
    let layout = parse_layout(
        "title-body-code",
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="1..*"></slot><slot name="code" accepts="code" arity="1"></slot></section>"#,
    )
    .unwrap();
    let mapped = map_by_convention(
        parse_markdown(
            "# T\n\n::: {reveal}\n\nBody paragraph.\n\n```rust\nfn main() {}\n```\n\n:::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap(),
        &layout,
    )
    .unwrap();
    let slide = &mapped.mapped_slides()[0];
    let body = SlotName::new("body").unwrap();
    let code = SlotName::new("code").unwrap();

    assert_eq!(slide.step_count, 2);
    assert_eq!(slide.slots[&body].fragments()[0].reveal_span().unwrap().start, 1);
    assert_eq!(slide.slots[&code].fragments()[0].reveal_span().unwrap().start, 2);
}

// crates/peitho-core/src/check.rs #[cfg(test)]
#[test]
fn check_deck_carries_reveal_step_count_to_checked_slide() {
    let layout = parse_layout(
        "title-body",
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="1..*"></slot></section>"#,
    )
    .unwrap();
    let checked = check_deck(
        map_by_convention(
            parse_markdown(
                "# T\n\n::: {reveal}\n\n- one\n- two\n\n:::\n",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(checked.checked_slides()[0].step_count(), 2);
}
```

**Implementation**:

- Add `pub step_count: usize` to `ParsedSlide`.
- Add `pub(crate) step_count: usize` to `MappedSlide`.
- Add `step_count: usize` to `CheckedSlide<S>`, add a `step_count()` accessor, and add a constructor argument immediately after `skip` or immediately before `page_number_hidden`.
- In `map_slide`, set `step_count: slide.step_count`.
- In `check_deck`, pass `slide.step_count` into `CheckedSlide::new`.
- In `resolve_image_paths`, destructure and copy `step_count` when rebuilding `CheckedSlide`.
- Update in-crate test constructors with `step_count: 0`; do not add reveal-specific branches to `check_slide` or `accepts_fragment`.

**Verify**:

```sh
cargo test -p peitho-core mapping::tests::mapping_carries_reveal_step_count_and_cross_slot_spans
cargo test -p peitho-core check::tests::check_deck_carries_reveal_step_count_to_checked_slide
```

## Task 4: manifest revealSteps and ts-rs binding

**Goal**: Surface checked slide step counts as additive `ManifestSlide.revealSteps` and regenerate the TypeScript binding.

**Files**: `crates/peitho-core/src/manifest.rs`, `bindings/ManifestSlide.ts`

**Test**:

```rust
// crates/peitho-core/src/manifest.rs #[cfg(test)]
#[test]
fn build_manifest_serializes_reveal_steps_from_checked_deck() {
    let manifest = build_manifest(
        &checked_deck(
            "# T\n\n::: {reveal}\n\n- one\n- two\n\n:::\n",
            title_body_layout(),
        ),
        &[],
    );
    let json = manifest_json(&manifest).unwrap();

    assert_eq!(manifest.slides()[0].reveal_steps(), 2);
    assert!(json.contains(r#""revealSteps": 2"#));
}

#[test]
fn deserializes_manifest_missing_reveal_steps_as_zero() {
    let json = concat!(
        "{\n",
        "  \"version\": 1,\n",
        "  \"peithoVersion\": \"0.1.0\",\n",
        "  \"title\": \"Deck\",\n",
        "  \"slideCount\": 1,\n",
        "  \"plannedDurationMs\": null,\n",
        "  \"aspectRatio\": \"16:9\",\n",
        "  \"canvasWidth\": 1280,\n",
        "  \"canvasHeight\": 720,\n",
        "  \"sections\": [],\n",
        "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}],\n",
        "  \"images\": []\n",
        "}\n"
    );

    let manifest: Manifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.slides()[0].reveal_steps(), 0);
}

// crates/peitho-core/src/manifest.rs ts_tests
assert!(slide.contains("revealSteps: number"));
```

**Implementation**:

- Add `#[serde(rename = "revealSteps", default)] pub(crate) reveal_steps: usize` to `ManifestSlide`.
- Add `pub fn reveal_steps(&self) -> usize`.
- In `build_manifest`, set `reveal_steps: slide.step_count()`.
- Update `manifest_slide_text_and_slide_accessors`, `serializes_manifest_schema_exactly`, and literal `ManifestSlide` construction sites with `reveal_steps`.
- Update `exports_manifest_bindings_with_serde_field_names` to assert `revealSteps: number`.
- Regenerate `bindings/ManifestSlide.ts` with the existing ts-rs export test.

**Verify**:

```sh
cargo test -p peitho-core manifest::tests::build_manifest_serializes_reveal_steps_from_checked_deck
cargo test -p peitho-core manifest::tests::deserializes_manifest_missing_reveal_steps_as_zero
cargo test -p peitho-core manifest::ts_tests::exports_manifest_bindings_with_serde_field_names
```

## Task 5: rendered reveal attributes without hidden state

**Goal**: Stamp reveal attributes in rendered HTML while keeping PDF, preview, lint, and dist output fully visible.

**Files**: `crates/peitho-core/src/render.rs`

**Test**:

```rust
// crates/peitho-core/src/render.rs #[cfg(test)]
#[test]
fn render_reveal_stamps_blocks_lists_code_and_section_total() {
    let rendered = render_checked_deck_with_layout(
        "# T\n\n::: {reveal}\n\n## Sub\n\nBody paragraph.\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n\n:::\n",
        parse_layout(
            "title-body-code",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="1..*"></slot><slot name="code" accepts="code" arity="1"></slot></section>"#,
        )
        .unwrap(),
    );
    let html = rendered.slides()[0].html();

    assert!(html.contains(r#"data-reveal-steps="5""#), "{html}");
    assert!(html.contains(r#"<h2 data-reveal-step="1">Sub</h2>"#), "{html}");
    assert!(html.contains(r#"<p data-reveal-step="2">Body paragraph.</p>"#), "{html}");
    assert!(html.contains(r#"<li data-reveal-step="3">one</li>"#), "{html}");
    assert!(html.contains(r#"<li data-reveal-step="4">two</li>"#), "{html}");
    assert!(html.contains(r#"<pre class="slot-code" data-reveal-step="5"><code"#), "{html}");
    assert!(!html.contains("data-reveal-hidden"), "{html}");

    let plain = render_checked_deck("# Plain\n\nBody");
    let plain_html = plain.slides()[0].html();
    assert!(
        !plain_html.contains("data-reveal-steps") && !plain_html.contains("data-reveal-step"),
        "{plain_html}"
    );
}

#[test]
fn pdf_document_keeps_reveal_steps_in_final_state() {
    let rendered = render_checked_deck("# T\n\n::: {reveal}\n\nBody\n\n:::\n");
    let html = render_pdf_document(&rendered);

    assert!(html.contains(r#"data-reveal-step="1""#), "{html}");
    assert!(!html.contains("data-reveal-hidden"), "{html}");
}
```

**Implementation**:

- Pass `slide.step_count()` into `render_slide` and set `data-reveal-steps` on the `<section>` in the existing `HtmlRewriter` handler that already sets `data-slide-key` only when `slide.step_count() > 0`.
- Keep the current `render_block_slot` batching for fragments with `reveal_span() == None`.
- When `render_block_slot` sees a revealed fragment, flush the current markdown run, render that single fragment, and append reveal attributes before resuming the next non-revealed run.
- Add `render_revealed_fragment(body, fragment, span, breaks, footnote_numbers)` whose first operation is an exhaustive `match fragment.kind()` with explicit arms for `FragmentKind::Heading { .. }`, `FragmentKind::Paragraph`, `FragmentKind::Text`, `FragmentKind::Code`, `FragmentKind::Math { .. }`, `FragmentKind::Footnotes { .. }`, `FragmentKind::Image { .. }`, `FragmentKind::List`, and `FragmentKind::SlotGroup { .. }`; do not use `_ =>`.
- In the `Heading` and `Paragraph` arms, parse with `Options::ENABLE_OLD_FOOTNOTES` and replace the first top-level `Event::Start(Tag::Heading { .. })` or `Event::Start(Tag::Paragraph)` with `Event::Html` containing the same opening tag plus `data-reveal-step="{span.start}"`; pass the existing end event through.
- In the `List` arm, track list depth while transforming events; replace only `Event::Start(Tag::Item)` at list depth `1` with `<li data-reveal-step="{span.start + top_level_item_index}">`; keep nested list items untouched.
- In the `Math { .. }` and `Footnotes { .. }` arms, set `data-reveal-step` on `.peitho-math` and `.peitho-footnotes`.
- In the `Code` arm, render one `<pre class="{class_name}" data-reveal-step="{span.start}"><code>{highlighted_body}</code></pre>` per revealed fragment; preserve the existing single-`pre` batching when no code fragment has a reveal span.
- In the `Image { .. }` arm, set `data-reveal-step` on the emitted `<img>`; in the inline slot path, set it on the slot `<span>`.
- In the `Text` and `SlotGroup { .. }` arms, return a render error or use `unreachable!("revealed Text/SlotGroup fragments are not renderable")`; never silently skip a revealed fragment.
- The exhaustive match is the guard against future `FragmentKind` variants; adding a variant must make this function fail to compile until reveal behavior is decided.

**Verify**:

```sh
cargo test -p peitho-core render::tests::render_reveal_stamps_blocks_lists_code_and_section_total
cargo test -p peitho-core render::tests::pdf_document_keeps_reveal_steps_in_final_state
```

## Task 6: server sync stores absolute index and step

**Goal**: Make `/sync` fold and replay `{ index, step }` as one atomic absolute state.

**Files**: `crates/peitho/src/server.rs`

**Test**:

```rust
// crates/peitho/src/server.rs #[cfg(test)]
#[test]
fn sync_hub_stores_index_and_step_atomically() {
    let hub = SyncHub::default();
    let session = hub.snapshot().session;

    let message = SyncMessage::Index(SyncIndexMessage { index: 2, step: 3 });
    let seq = hub.broadcast_sync_message(&message);

    assert_eq!(seq, 1);
    assert_eq!(
        hub.wait_after(0, Duration::from_secs(1)).unwrap(),
        SyncPoll {
            snapshot: SyncSnapshot {
                seq: 1,
                index: Some(2),
                step: Some(3),
                swapped: false,
                timer: None,
                generation: 0,
                session,
            },
            message: Some(r#"{"index":2,"step":3}"#.to_owned()),
        },
    );
}

#[test]
fn sync_response_body_includes_step() {
    let body = sync_response_body(
        SyncSnapshot {
            seq: 4,
            index: Some(2),
            step: Some(1),
            swapped: false,
            timer: None,
            generation: 0,
            session: "session-a".to_owned(),
        },
        None,
    );

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["index"], 2);
    assert_eq!(json["step"], 1);
}

#[test]
fn sync_endpoint_accepts_index_step_body() {
    let dir = tempfile::tempdir().unwrap();
    let server = PresentServer::bind(dir.path().to_path_buf(), 0, "present.html").unwrap();

    let post = http_request(&server, "POST", "/sync", r#"{"index":2,"step":1}"#);
    let get = http_request(&server, "GET", "/sync", "");

    assert_eq!(post.status, 200);
    assert_eq!(serde_json::from_str::<Value>(&get.body).unwrap()["index"], 2);
    assert_eq!(serde_json::from_str::<Value>(&get.body).unwrap()["step"], 1);
}
```

**Implementation**:

- Add `step: Option<usize>` to `SyncState` and `SyncSnapshot`.
- Add required `step: usize` to `SyncIndexMessage`; keep `#[serde(deny_unknown_fields)]`.
- In `broadcast_sync_message`, update `state.index` and `state.step` together in the `SyncMessage::Index` arm.
- In `wait_after` and `snapshot`, copy `state.step`.
- In `sync_response_body`, add `step: Option<usize>` to `SyncResponseBody` so handshake and poll responses both include the field.
- Update existing server tests and HTTP route tests from `{"index":2}` to `{"index":2,"step":0}`.

**Verify**:

```sh
cargo test -p peitho server::tests::sync_hub_stores_index_and_step_atomically
cargo test -p peitho server::tests::sync_response_body_includes_step
cargo test -p peitho server::tests::sync_endpoint_accepts_index_step_body
```

## Task 7: present shell step state, hiding, and events

**Goal**: Make `PresentShell` own current reveal step state and step-aware navigation without adding keyboard shortcuts.

**Files**: `packages/peitho-present/src/shell.ts`, `packages/peitho-present/src/index.ts`, `packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts`, `packages/peitho-present/test/generated.test.ts`

**Test**:

```ts
// packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts
it("walks reveal steps before changing slides and hides future step elements", async () => {
  const responseManifest = manifestWithSlides([
    { key: "intro", revealSteps: 2 },
    { key: "end" }
  ]);
  const root = document.createElement("main");
  const changes: unknown[] = [];
  const steps: unknown[] = [];
  listenWindow("peitho:slidechange", (event) => changes.push((event as CustomEvent).detail));
  listenWindow("peitho:stepchange", (event) => steps.push((event as CustomEvent).detail));

  const shell = await mountForTest({
    root,
    fetcher: vi.fn(async (url: string) => {
      if (url === "manifest.json") return okJson(responseManifest);
      if (url === "peitho.css") return okText("");
      if (url.includes("intro")) {
        return okText('<section><p data-reveal-step="1">A</p><p data-reveal-step="2">B</p></section>');
      }
      if (url.includes("end")) return okText("<section>End</section>");
      return { ok: false, status: 404, text: async () => "" } as Response;
    }) as unknown as typeof fetch,
    window
  });
  const intro = root.querySelector<HTMLElement>('[data-slide-index="0"]')!;
  const markers = intro.shadowRoot!.querySelectorAll<HTMLElement>("[data-reveal-step]");

  expect(shell.currentIndex).toBe(0);
  expect(shell.currentStep).toBe(0);
  expect([...markers].map((el) => el.hasAttribute("data-reveal-hidden"))).toEqual([true, true]);

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(shell.currentIndex).toBe(0);
  expect(shell.currentStep).toBe(1);
  expect([...markers].map((el) => el.hasAttribute("data-reveal-hidden"))).toEqual([false, true]);

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(shell.currentIndex).toBe(1);
  expect(shell.currentStep).toBe(0);
  expect(changes).toEqual([
    { key: "intro", index: 0, total: 2, previousIndex: null, step: 0, stepCount: 2 },
    { key: "end", index: 1, total: 2, previousIndex: 0, step: 0, stepCount: 0 }
  ]);
  expect(steps).toEqual([
    { index: 0, step: 1, stepCount: 2 },
    { index: 0, step: 2, stepCount: 2 }
  ]);
});

it("direct navigation lands fully revealed and prev enters previous slide fully revealed", async () => {
  const responseManifest = manifestWithSlides([
    { key: "intro", revealSteps: 2 },
    { key: "skip", skip: true, revealSteps: 3 },
    { key: "end", revealSteps: 1 }
  ]);
  const root = document.createElement("main");
  const shell = await mountForTest({ root, fetcher: fetchForManifest(responseManifest), window });

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { key: "intro" } } }));
  expect(shell.currentIndex).toBe(0);
  expect(shell.currentStep).toBe(2);

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 2, step: 0 } } }));
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
  expect(shell.currentIndex).toBe(0);
  expect(shell.currentStep).toBe(2);
});

// packages/peitho-present/test/generated.test.ts
const manifest: Manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 1,
  plannedDurationMs: null,
  aspectRatio: "16:9",
  canvasWidth: 1280,
  canvasHeight: 720,
  sections: [],
  slides: [
    {
      index: 0,
      key: "intro",
      src: "slides/000-intro.html",
      hasNotes: false,
      skip: false,
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    }
  ],
  images: []
};
expect(manifest.slides[0].revealSteps).toBe(0);
```

**Implementation**:

- Change `NavigateTarget` to allow `{ index: number; step?: number }`; keep `{ key: string }`, `first`, `last`, `next`, and `prev`.
- Add `StepChangeDetail = { index: number; step: number; stepCount: number }`, export it from `index.ts`, and add `currentStep: number` to `PresentShell`.
- Add `stepCountFor(index: number): number` that reads `this.slides[index].meta.revealSteps ?? 0` and clamps invalid values to `0`.
- Change `resolveTarget` to return `{ index: number; step: number }`.
- Implement sequential navigation in `resolveSequentialTarget`: `next` increments step while `currentStep < stepCount`; otherwise it returns the next non-skipped slide at step `0`; `prev` decrements step while `currentStep > 0`; otherwise it returns the previous non-skipped slide at that slide's full step count.
- Make direct `{ index }`, `{ key }`, `first`, and `last` resolve to `stepCountFor(index)` when `step` is omitted; clamp provided `step` into `0..=stepCount`.
- Call `show(initialSlideIndex(pending.map((view) => view.meta)) ?? 0, 0)` on load.
- Change `show(index, step)` identity guard to compare both `currentIndex` and `currentStep`.
- Inject shell-only CSS into each slide shadow root after deck CSS: `[data-reveal-hidden]{visibility:hidden}`.
- Add `applyRevealState(host, step)` that toggles `data-reveal-hidden` on `[data-reveal-step]` elements whose numeric step exceeds the current step.
- Dispatch `peitho:slidechange` only when the index changes, with detail `{ key, index, total, previousIndex, step, stepCount }`.
- Dispatch `peitho:stepchange` only when the index is unchanged and the step changes, with detail `{ index, step, stepCount }`.
- Do not edit `packages/peitho-present/src/keyboard.ts` or add keyboard shortcuts.

**Verify**:

```sh
cd packages/peitho-present && npx vitest run test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts
cd packages/peitho-present && npx vitest run test/generated.test.ts
```

## Task 8: sync.ts post and replay absolute step state

**Goal**: Make client sync post and replay `{ index, step }` as a single absolute navigation target.

**Files**: `packages/peitho-present/src/sync.ts`, `packages/peitho-present/test/sync.test.ts`

**Test**:

```ts
// packages/peitho-present/test/sync.test.ts
it("exports strict index step sync message guard", () => {
  expect(isIndexSyncMessage({ index: 1, step: 0 })).toBe(true);
  expect(isIndexSyncMessage({ index: 1 })).toBe(false);
  expect(isIndexSyncMessage({ index: 1, step: Number.NaN })).toBe(false);
});

it("server sync channel replays handshake index and step together", async () => {
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") return Promise.resolve(okJson({ seq: 4, message: null, index: 2, step: 1 }));
    if (url === "/sync?seq=4") return new Promise<Response>(() => undefined);
    throw new Error(`unexpected sync url: ${url}`);
  }) as typeof fetch;
  const channel = serverSyncChannelFactory({ fetcher })("peitho-sync");
  const received: unknown[] = [];
  channel.onmessage = (event) => received.push(event.data);

  await vi.waitFor(() => expect(received).toEqual([{ index: 2, step: 1 }, { synced: true }]));
  channel.close();
});

it("posts local slidechange and stepchange index step to peitho-sync", () => {
  const channel = mockChannel();
  const bus = new EventTarget();
  const cleanup = installSyncBridge(window, () => channel, bus);
  cleanups.push(cleanup);

  bus.dispatchEvent(new CustomEvent("peitho:slidechange", { detail: { index: 1, step: 0 } }));
  bus.dispatchEvent(new CustomEvent("peitho:stepchange", { detail: { index: 1, step: 2 } }));

  expect(channel.sent).toEqual([{ index: 1, step: 0 }, { index: 1, step: 2 }]);
});

it("dispatches sync replay as one navigate target with index and step", () => {
  const channel = mockChannel();
  const bus = new EventTarget();
  const navigations: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) => navigations.push((event as CustomEvent).detail));
  cleanups.push(installSyncBridge(window, () => channel, bus));

  channel.onmessage?.({ data: { index: 2, step: 1 } });

  expect(navigations).toEqual([{ to: { index: 2, step: 1 } }]);
});
```

**Implementation**:

- Change `SyncMessage` from `{ index: number }` to `{ index: number; step: number }`.
- Change `isIndexSyncMessage` to require finite numeric `index` and finite numeric `step`.
- Add `step?: unknown` to `ServerSyncPollResponse`.
- In `deliverReplayState`, emit exactly one `{ index: body.index, step: body.step }` replay when `isIndexSyncMessage(body)` is true.
- In `installSyncBridge`, post `{ index, step }` from both `peitho:slidechange` and `peitho:stepchange`; reject invalid details with `console.error("Invalid peitho navigation state event")`.
- In the channel `onmessage` handler, dispatch `new CustomEvent("peitho:navigate", { detail: { to: { index: data.index, step: data.step } } })`.
- Update existing sync tests that post or expect `{ index: N }` to use `{ index: N, step: 0 }`.

**Verify**:

```sh
cd packages/peitho-present && npx vitest run test/sync.test.ts
```

## Task 9: remote step-aware absolute navigation

**Goal**: Make the remote compute `prev` and `next` locally from `revealSteps` plus `skip`, then post absolute `{ index, step }`.

**Files**: `packages/peitho-present/src/remote.ts`, `packages/peitho-present/test/remote.test.ts`

**Test**:

```ts
// packages/peitho-present/test/remote.test.ts
it("remote resolves reveal steps locally and posts absolute index step targets", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([
      { key: "intro", revealSteps: 2 },
      { key: "skip", skip: true, revealSteps: 4 },
      { key: "end", revealSteps: 1 }
    ])
  );

  button(root, "next").click();
  button(root, "next").click();
  button(root, "next").click();
  channel.deliver({ index: 2, step: 0 });
  button(root, "prev").click();

  expect(channel.sent).toEqual([
    { index: 0, step: 1 },
    { index: 0, step: 2 },
    { index: 2, step: 0 },
    { index: 0, step: 2 }
  ]);
});

it("remote preview mirrors the current reveal step", async () => {
  const previewNavigations: unknown[] = [];
  const { channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro", revealSteps: 2 }]),
    mockChannel(),
    { mountPresentShell: mockMountPresentShell(previewNavigations) }
  );

  channel.deliver({ index: 0, step: 1 });

  expect(previewNavigations.at(-1)).toEqual({ to: { index: 0, step: 1 } });
});
```

**Implementation**:

- Add `revealSteps: number` to `RemoteSlide` and populate it from `slide.revealSteps ?? 0`.
- Replace remote index-only state with a private `RemotePosition = { index: number; step: number } | null`.
- Replace `getCurrentIndex` and `setCurrentIndex` in `RemoteSyncBridgeOptions` with `getCurrentPosition()` and `setCurrentPosition(position: RemotePosition)`.
- Implement `resolveRemoteTarget(slides, currentPosition, to)` returning `{ index, step }`: `next` increments within the current slide until `step == revealSteps`; otherwise it returns the next non-skipped slide at step `0`; `prev` decrements within the current slide until `step == 0`; otherwise it returns the previous non-skipped slide at that slide's full reveal step count.
- Keep initial remote state at `{ index: initialSlideIndex(slides), step: 0 }` when a slide exists.
- On sync replay, call `setCurrentPosition({ index: data.index, step: data.step })`.
- On remote button click, post the returned target and optimistically set that same position.
- Update button enablement to use `resolveRemoteTarget` with the current position.
- Update `syncPreview` to dispatch `{ to: { index, step } }`.
- Keep the public `RemoteView.currentIndex` getter behavior by returning `currentPosition?.index ?? null`.

**Verify**:

```sh
cd packages/peitho-present && npx vitest run test/remote.test.ts
```

## Task 10: presenter current pane mirrors step and next pane stays final

**Goal**: Let the presenter current slide follow live reveal step state while the next-slide pane remains a fully revealed direct preview.

**Files**: `packages/peitho-present/src/presenter.ts`, `packages/peitho-present/test/presenter.test.ts`

**Test**:

```ts
// packages/peitho-present/test/presenter.test.ts
it("presenter current pane follows sync step while next pane stays final state", async () => {
  const responseManifest: Manifest = {
    ...manifest,
    slides: [
      { ...manifest.slides[0], revealSteps: 2 },
      { ...manifest.slides[1], revealSteps: 1 }
    ]
  };
  const root = document.createElement("main");
  const { channel, factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: vi.fn(async (url: string) => {
      if (url === "manifest.json") return okJson(responseManifest);
      if (url === "peitho.css") return okText("");
      if (url === "slides/000-intro.html") {
        return okText('<section><p data-reveal-step="1">A</p><p data-reveal-step="2">B</p></section>');
      }
      if (url === "slides/001-details.html") {
        return okText('<section><p data-reveal-step="1">Next</p></section>');
      }
      return { ok: false, status: 404, text: async () => "" } as Response;
    }) as typeof fetch,
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  channel.onmessage?.({ data: { index: 0, step: 1 } });

  const currentSecond = root
    .querySelector<HTMLElement>('[data-peitho-presenter="current"] [data-slide-index="0"]')!
    .shadowRoot!
    .querySelector<HTMLElement>('[data-reveal-step="2"]')!;
  const nextFirst = root
    .querySelector<HTMLElement>('[data-peitho-presenter="preview"] [data-slide-index="1"]')!
    .shadowRoot!
    .querySelector<HTMLElement>('[data-reveal-step="1"]')!;
  expect(currentSecond.hasAttribute("data-reveal-hidden")).toBe(true);
  expect(nextFirst.hasAttribute("data-reveal-hidden")).toBe(false);
});
```

**Implementation**:

- Update `SlideChangeDetail` usage in `presenter.ts` to tolerate the new `step` and `stepCount` fields without changing notes, section, agenda, or rehearsal behavior.
- In the synthetic first-slide call to `updateFromSlide`, pass `step: mainShell.currentStep` and `stepCount: firstSlide.revealSteps ?? 0`.
- Keep next-preview navigation as `new CustomEvent("peitho:navigate", { detail: { to: { index: nextIndex } } })`; omitted step intentionally lands fully revealed through the shell direct-jump rule.
- Do not add an `onStepChange` presenter chrome refresh unless a visible step counter is added in the same patch.

**Verify**:

```sh
cd packages/peitho-present && npx vitest run test/presenter.test.ts
```

## Task 11: user-facing reveal syntax docs

**Goal**: Document `::: {reveal}` where the repo already documents deck fenced-div syntax.

**Files**: `README.md`, `site/content/guide/writing-decks.md`, `CLAUDE.md`

**Test**:

```sh
rg -n '::: \{reveal\}' README.md site/content/guide/writing-decks.md
rg -n 'final state|peitho present' README.md site/content/guide/writing-decks.md
rg -n 'Incremental reveal \(2026-07-31, Issue #290\)' CLAUDE.md
```

**Implementation**:

- In `README.md`, add a short `### Incremental reveal` subsection after `### Explicit slots`.
- In `site/content/guide/writing-decks.md`, add `## Incremental reveal` after `## Explicit slot syntax`.
- In `CLAUDE.md`, add one invariant-list bullet named `Incremental reveal (2026-07-31, Issue #290)` stating that `::: {reveal}` dissolves at parse into `RevealSpan` annotations with no `FragmentKind` riding the pipeline; step counting has one parse-time source and surfaces as `ManifestSlide.revealSteps`; rendered HTML is fully visible by default and only the present shell toggles `data-reveal-hidden`, so PDF, preview, lint, and dist show final state by construction; sync uses atomic absolute `{"index":N,"step":M}`; direct jumps land fully revealed; `prev` enters the previous slide fully revealed; design records are `docs/specs/2026-07-31-incremental-reveal-design.md` and `docs/plans/2026-07-31-incremental-reveal.md`.
- Document this exact syntax:
  - `::: {reveal}` marks a group of blocks that reveal in `peitho present`.
  - Content outside the group is visible at step `0`.
  - Each direct child block is one step.
  - A list contributes one step per top-level list item; nested items appear with the parent item.
  - `peitho preview`, PDF export, lint, and published output show final state.
  - `{reveal=value}`, empty groups, nested fences, and multi-attribute fences are build errors.

**Verify**:

```sh
rg -n '::: \{reveal\}' README.md site/content/guide/writing-decks.md
rg -n 'final state|peitho present' README.md site/content/guide/writing-decks.md
rg -n 'Incremental reveal \(2026-07-31, Issue #290\)' CLAUDE.md
```

## Repo gates

```sh
cargo test --workspace          # run 3 times in a row
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
git diff --exit-code packages/peitho-present/dist/remote.js
```

Final E2E note: verify present step navigation in a real Chrome before claiming fixed. Use a fixed `--port`, run `curl -X POST http://127.0.0.1:<port>/sync -H 'Content-Type: application/json' -d '{"index":0,"step":1}'`, and capture the present window with `screencapture`; confirm step 1 is visible, later steps remain hidden, and direct navigation shows the final state.
