# PDF reveal-step pages (Issue #723)

Date: 2026-10-20

## File map

| File | Implemented change |
| --- | --- |
| `packages/peitho-present/src/pdfStepPages.ts` | Add the single browser-side page expander. It validates ordered `{key, revealSteps}` metadata embedded by Rust, deep-clones step pages, copies synchronous canvas bitmaps, and delegates reveal and emphasis state to `applyStepState`. |
| `packages/peitho-present/test/pdfStepPages.test.ts` | Cover page order, N+1 pages, cumulative reveal hiding, exact-match emphasis, unchanged plain slides, inherited page-number attributes, wrap-count and key-order mismatches, invalid metadata, forged DOM counts, and canvas copying in jsdom. |
| `packages/peitho-present/src/viewer.ts` | Re-export `expandRevealStepPages` from the existing `PeithoViewer` IIFE surface. |
| `packages/peitho-present/test/viewer.test.ts` | Add the expander to the exact viewer export-surface assertion. |
| `packages/peitho-present/dist/viewer.js` | Rebuild the committed viewer IIFE with the new export. |
| `crates/peitho-core/src/domain.rs` | Retain the checked reveal-step count on each `RenderedSlide` so browser expansion never trusts mutable DOM metadata. |
| `crates/peitho-core/src/notes.rs` | Update the direct `RenderedSlide` fixtures for the retained reveal-step count. |
| `crates/peitho-core/src/render.rs` | In `PublicReveal::Steps` only, serialize ordered `{key, revealSteps}` records with script-safe escaping and insert the guarded viewer bootstrap after parsed-slide announcement and before `pdf_flatten.js`; keep omitted and explicit `final` on the same HTML path, keep Rust from duplicating slides, and narrow two lint assertions so the embedded viewer's expander strings do not look like PDF wrappers. |
| `crates/peitho-core/src/pdf_flatten.js` | Skip box-shadow targets whose computed visibility is not `visible` or whose effective opacity below the slide is zero; preserve partial effective opacity on appended outer-shadow images. This correction also affects omitted/final PDFs. |
| `crates/peitho/src/cdp.rs` | Read `data-peitho-pdf-step-pages-error` after flatten readiness and fail before `Page.printToPDF`, sharing one labeled status evaluator and validator with the shadow-mounted check. |
| `crates/peitho/src/main.rs` | Rename and update the cross-mode output regression so omitted equals `final`, `steps` changes PDF HTML, and manifest, slides, and lint stay mode-independent. |
| `crates/peitho/tests/export_pdf.rs` | Add ignored Chrome E2Es for four-page step expansion, invisible box-shadow suppression, and zero-effective-opacity ancestor suppression, plus `pdf_page_object_count` and `pdf_image_object_count`. |
| `README.md` | State that final reveal is the default and `public_reveal: steps` affects both the published viewer and PDF export. |
| `site/content/guide/frontmatter.md` | Document that `public_reveal: steps` also expands PDF pages and that repeated step pages retain the logical slide number and total. |
| `site/content/guide/writing-decks.md` | Update incremental reveal and stepped emphasis output descriptions for PDF steps mode. |
| `site/content/guide/layouts.md` | Explain which DOM, canvas, and script-held state reaches cloned step pages and the final page. |
| `site/content/examples/incremental-reveal.md` | Explain that this steps-mode example publishes every reveal position to the viewer and PDF. |
| `site/content/examples/code-emphasis.md` | Explain that this steps-mode example publishes every exact-match emphasis position to the viewer and PDF. |
| `CLAUDE.md` | Update the typed `public_reveal` consumers, Rust-owned PDF metadata, incremental-reveal and code-emphasis invariants, layout-script ordering, and box-shadow flattening pitfall. |

No parser, phase, manifest schema, binding, slide renderer, lint renderer,
distribution bootstrap, `stepState.ts`, or `stepnav.ts` change is required.
`PublicReveal` and `DeckSettings::public_reveal()` already provide the typed
gate; `RenderedSlide` now preserves the already-computed count for the PDF
bootstrap.

There is one concrete consistency issue in the requested design. A strict
comparison against the bytes at commit `05e46b7` cannot coexist with
re-exporting the function from `viewer.ts` and rebuilding `dist/viewer.js`:
that bundle is embedded in PDF and lint HTML and in the distribution index.
This plan follows the explicit test oracle requested for `main.rs`: omitted and
explicit `final` PDF HTML must be byte-identical to each other, `steps` PDF
HTML must differ, and manifest, slide fragments, and lint HTML must be
identical across the three modes. “Unchanged” below means behavior and
mode-independence, not identity to the pre-#723 embedded bundle bytes.

## Task 1: Add the browser page expander with jsdom coverage

### Goal

Create one DOM function that expands every stepped PDF slide into positions
0 through N while leaving zero-step slides in place. Rust supplies the ordered
slide keys and counts; mutable `data-reveal-steps` attributes are not an
authority. The function must reject count, key, structure, and numeric metadata
errors. Each clone receives the source canvases' synchronous bitmaps, and the
final original wrap receives step N state because step N can be an exact-match
code-emphasis group even though all reveal nodes are visible there.

### Files

- `packages/peitho-present/src/pdfStepPages.ts`
- `packages/peitho-present/test/pdfStepPages.test.ts`

### Test (concrete code)

Create `test/pdfStepPages.test.ts` with these tests:

```ts
import { expect, it, vi } from "vitest";
import { expandRevealStepPages } from "../src/pdfStepPages";

it("expands stepped wraps in order and preserves logical page numbers", () => {
  document.body.innerHTML = `
    <div id="stepped" class="peitho-slide-wrap">
      <section class="peitho-slide"
        data-slide-key="stepped"
        data-reveal-steps="2"
        data-peitho-page-number="1"
        data-peitho-page-total="2">
        <p data-reveal-step="1">first</p>
        <p data-reveal-step="2">second</p>
        <span data-emphasis-step="1">one</span>
        <span data-emphasis-step="2">two</span>
      </section>
    </div>
    <div id="plain" class="peitho-slide-wrap">
      <section class="peitho-slide"
        data-slide-key="plain"
        data-peitho-page-number="2"
        data-peitho-page-total="2">
        <p>plain</p>
      </section>
    </div>`;
  const originalStepped = document.querySelector<HTMLElement>("#stepped")!;
  const originalPlain = document.querySelector<HTMLElement>("#plain")!;

  expandRevealStepPages(document, [
    { key: "stepped", revealSteps: 2 },
    { key: "plain", revealSteps: 0 }
  ]);

  const wraps = Array.from(
    document.querySelectorAll<HTMLElement>(".peitho-slide-wrap")
  );
  expect(wraps).toHaveLength(4);
  expect(wraps[2]).toBe(originalStepped);
  expect(wraps[3]).toBe(originalPlain);
  expect(
    wraps.map((wrap) =>
      wrap.querySelector<HTMLElement>("section.peitho-slide")!.dataset
        .peithoPageNumber
    )
  ).toEqual(["1", "1", "1", "2"]);
  expect(
    wraps.map((wrap) =>
      wrap.querySelector<HTMLElement>("section.peitho-slide")!.dataset
        .peithoPageTotal
    )
  ).toEqual(["2", "2", "2", "2"]);

  const steppedPages = wraps.slice(0, 3);
  expect(
    steppedPages.map((page) =>
      Array.from(page.querySelectorAll<HTMLElement>("[data-reveal-step]")).map(
        (node) => node.hasAttribute("data-reveal-hidden")
      )
    )
  ).toEqual([
    [true, true],
    [false, true],
    [false, false]
  ]);
  expect(
    steppedPages.map((page) =>
      Array.from(page.querySelectorAll<HTMLElement>("[data-emphasis-step]")).map(
        (node) => node.hasAttribute("data-emphasis-active")
      )
    )
  ).toEqual([
    [false, false],
    [true, false],
    [false, true]
  ]);
});

it("ignores forged DOM step counts when Rust metadata says zero steps", () => {
  document.body.innerHTML = `
    <div id="plain" class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="plain" data-reveal-steps="3"></section>
    </div>`;
  const plain = document.querySelector<HTMLElement>("#plain")!;

  expandRevealStepPages(document, [{ key: "plain", revealSteps: 0 }]);

  expect(document.querySelectorAll(".peitho-slide-wrap")).toHaveLength(1);
  expect(document.querySelector("#plain")).toBe(plain);
});

it("rejects count and key mismatches", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="authored"></section>
    </div>`;
  expect(() => expandRevealStepPages(document, [])).toThrow(
    "PDF slide wrap count 1 does not match Rust slide metadata count 0"
  );
  expect(() =>
    expandRevealStepPages(document, [{ key: "expected", revealSteps: 0 }])
  ).toThrow(
    'PDF slide wrap 1 has data-slide-key "authored"; Rust metadata expects "expected"'
  );
});

it("rejects invalid Rust revealSteps with the slide key", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="bad"></section>
    </div>`;
  expect(() =>
    expandRevealStepPages(document, [{ key: "bad", revealSteps: 2.8 }])
  ).toThrow(
    'Slide "bad" has invalid revealSteps 2.8; expected a non-negative safe integer'
  );
});

it("copies source canvas pixels into clone canvases", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="canvas">
        <canvas width="320" height="180"></canvas>
      </section>
    </div>`;
  const source = document.querySelector<HTMLCanvasElement>("canvas")!;
  const drawImage = vi.fn();
  const owners: HTMLCanvasElement[] = [];
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = vi.fn(function (
    this: HTMLCanvasElement
  ) {
    owners.push(this);
    return { drawImage };
  }) as unknown as typeof HTMLCanvasElement.prototype.getContext;
  try {
    expandRevealStepPages(document, [{ key: "canvas", revealSteps: 1 }]);
  } finally {
    HTMLCanvasElement.prototype.getContext = original;
  }
  expect(owners).toEqual([
    document.querySelectorAll<HTMLCanvasElement>("canvas")[0]
  ]);
  expect(owners).not.toContain(source);
  expect(drawImage).toHaveBeenCalledWith(source, 0, 0);
});
```

The complete test file also pins a missing slide section, both directions of a
wrap-count mismatch, negative and unsafe-integer metadata, zero-sized canvas
skips, unavailable 2D contexts, and contextual `drawImage` failures. The third
stepped wrap in the first test is the original node, so its active second
emphasis group proves that the original receives `applyStepState(original, N)`.

### Implementation (concrete code)

Create `src/pdfStepPages.ts` with this implementation:

```ts
import { applyStepState } from "./stepState";

export interface PdfStepPageSlide {
  readonly key: string;
  readonly revealSteps: number;
}

function validatedRevealStepCount(slide: PdfStepPageSlide): number {
  if (!Number.isSafeInteger(slide.revealSteps) || slide.revealSteps < 0) {
    throw new Error(
      `Slide ${JSON.stringify(slide.key)} has invalid revealSteps ${String(slide.revealSteps)}; expected a non-negative safe integer`
    );
  }
  return slide.revealSteps;
}

function copyCanvasBitmaps(
  source: ParentNode,
  copy: ParentNode,
  key: string
): void {
  const sourceCanvases = Array.from(
    source.querySelectorAll<HTMLCanvasElement>("canvas")
  );
  const copyCanvases = Array.from(
    copy.querySelectorAll<HTMLCanvasElement>("canvas")
  );
  for (const [index, sourceCanvas] of sourceCanvases.entries()) {
    if (sourceCanvas.width === 0 || sourceCanvas.height === 0) continue;
    const context = copyCanvases[index]?.getContext("2d") ?? null;
    if (context === null) {
      throw new Error(
        `Unable to copy canvas ${index + 1} for slide ${JSON.stringify(key)}: 2d context unavailable`
      );
    }
    try {
      context.drawImage(sourceCanvas, 0, 0);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      throw new Error(
        `Unable to copy canvas ${index + 1} for slide ${JSON.stringify(key)}: drawImage failed: ${message}`,
        { cause: error }
      );
    }
  }
}

export function expandRevealStepPages(
  doc: Document,
  slides: ReadonlyArray<PdfStepPageSlide>
): void {
  const wraps = Array.from(
    doc.querySelectorAll<HTMLElement>(".peitho-slide-wrap")
  );
  if (wraps.length !== slides.length) {
    throw new Error(
      `PDF slide wrap count ${wraps.length} does not match Rust slide metadata count ${slides.length}`
    );
  }

  for (const [index, wrap] of wraps.entries()) {
    const slide = wrap.querySelector<HTMLElement>("section.peitho-slide");
    if (slide === null) {
      throw new Error(
        `PDF slide wrap ${index + 1} does not contain section.peitho-slide`
      );
    }

    const metadata = slides[index];
    const actualKey = slide.dataset.slideKey;
    if (actualKey !== metadata.key) {
      throw new Error(
        `PDF slide wrap ${index + 1} has data-slide-key ${JSON.stringify(actualKey ?? "<missing>")}; Rust metadata expects ${JSON.stringify(metadata.key)}`
      );
    }
    const stepCount = validatedRevealStepCount(metadata);
    if (stepCount === 0) continue;

    for (let step = 0; step < stepCount; step += 1) {
      const copy = wrap.cloneNode(true) as HTMLElement;
      copyCanvasBitmaps(wrap, copy, metadata.key);
      // Other script-set property values and later async changes reach only the final page.
      applyStepState(copy, step);
      wrap.before(copy);
    }
    applyStepState(wrap, stepCount);
  }
}
```

Capture the original wraps before inserting any clones so the loop never
expands a clone. Keep every cloned `<script>` node in the DOM and do not call
`executeInlineScripts`; `cloneNode(true)` does not execute cloned scripts.
Canvas bitmap copying is the one explicit property-state bridge. Other
script-held properties and later asynchronous changes reach only the original
fully revealed page.

### Verification command

```sh
cd packages/peitho-present && npx vitest run test/pdfStepPages.test.ts && npm run typecheck
```

## Task 2: Export the expander through the committed viewer bundle

### Goal

Expose the page expander to Rust's existing embedded `PeithoViewer` runtime
without adding a second runtime or copying the algorithm into an inline Rust
string.

### Files

- `packages/peitho-present/src/viewer.ts`
- `packages/peitho-present/test/viewer.test.ts`
- `packages/peitho-present/dist/viewer.js`

### Test (concrete code)

Replace the exact export-surface expectation in `test/viewer.test.ts` with:

```ts
it("exports exactly the distribution viewer runtime surface", () => {
  expect(Object.keys(viewer).sort()).toEqual([
    "announceParsedSlides",
    "announceShadowMounted",
    "applyStepState",
    "createStepStateStyle",
    "createViewerNavigator",
    "dropDisconnectedShadowMounted",
    "executeInlineScripts",
    "expandRevealStepPages",
    "keyBelongsToTarget",
    "pointerBelongsToTarget",
    "readViewerPosition",
    "shadowMountedBacklog",
    "writeViewerPosition"
  ]);
});
```

### Implementation (concrete code)

Add this re-export to `src/viewer.ts`:

```ts
export { expandRevealStepPages } from "./pdfStepPages";
```

Run the package build after the source and tests pass. Retain the generated
change to `dist/viewer.js`; `dist/shell.js`, `dist/preview.js`, and
`dist/remote.js` must rebuild without byte changes.

### Verification command

```sh
cd packages/peitho-present && npm run build && npx vitest run test/pdfStepPages.test.ts test/viewer.test.ts && npm run typecheck
git diff --check -- packages/peitho-present/src/pdfStepPages.ts packages/peitho-present/src/viewer.ts packages/peitho-present/test/pdfStepPages.test.ts packages/peitho-present/test/viewer.test.ts packages/peitho-present/dist/viewer.js
```

## Task 3: Wire steps-mode PDF expansion between announcement and flattening

### Goal

Make `render_pdf_document` emit one additional guarded browser bootstrap only
for `PublicReveal::Steps`. Parsed layout scripts must see exactly one section
per logical slide, the expander must run after that announcement, and
`pdf_flatten.js` must start only after all page clones and step attributes are
present.

### Files

- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/notes.rs`
- `crates/peitho-core/src/render.rs`

### Test (concrete code)

Add these renderer tests:

```rust
#[test]
fn pdf_final_and_omitted_modes_are_byte_identical() {
    const BODY: &str = "# T\n\n::: {reveal}\n\nBody\n\n:::\n";
    let omitted = render_checked_deck_with_layout(BODY, title_body_layout());
    let final_mode = render_checked_deck_with_layout(
        &format!("---\npublic_reveal: final\n---\n{BODY}"),
        title_body_layout(),
    );

    let omitted_html = render_pdf_document(&omitted);
    let final_html = render_pdf_document(&final_mode);

    assert_eq!(omitted_html, final_html);
    assert!(!final_html.contains("PeithoViewer.expandRevealStepPages("));
    assert!(!final_html.contains("data-peitho-pdf-step-pages-error"));
}

#[test]
fn pdf_steps_mode_expands_after_announcement_and_before_flattening() {
    let rendered = render_checked_deck_with_layout(
        "---\npublic_reveal: steps\n---\n# T\n\n::: {reveal}\n\nBody\n\n:::\n",
        title_body_layout(),
    );

    let html = render_pdf_document(&rendered);
    let last_slide_index = html.rfind(r#"data-slide-key="t""#).unwrap();
    let announce_index = html.find("PeithoViewer.announceParsedSlides(").unwrap();
    let style_index = html
        .find("document.head.appendChild(PeithoViewer.createStepStateStyle(document));")
        .unwrap();
    let expand_index = html
        .find("PeithoViewer.expandRevealStepPages(document, ")
        .unwrap();
    let flatten_index = html.find(PDF_FLATTEN_JS).unwrap();
    let expansion_script =
        embedded_script_body(&html, "PeithoViewer.expandRevealStepPages(document, ");
    let expansion_bootstrap = expansion_script
        .split_once(DISTRIBUTION_VIEWER_JS)
        .unwrap()
        .1;

    assert_eq!(
        html.matches(r#"<div class="peitho-slide-wrap">"#).count(),
        1,
    );
    assert!(last_slide_index < announce_index);
    assert!(announce_index < style_index);
    assert!(style_index < expand_index);
    assert!(expand_index < flatten_index);
    assert_eq!(html.matches(DISTRIBUTION_VIEWER_JS).count(), 3);
    assert!(expansion_bootstrap.contains("data-peitho-pdf-step-pages-error"));
    assert!(expansion_bootstrap.contains("throw error;"));
    assert!(!expansion_bootstrap.contains("executeInlineScripts"));
    assert!(expansion_bootstrap.contains(
        r#"PeithoViewer.expandRevealStepPages(document, [{"key":"t","revealSteps":1}]);"#
    ));
}
```

Keep the existing default/final test that asserts reveal markup has no
`data-reveal-hidden` in the server-generated body. The new steps test's one
wrap assertion pins browser expansion and prevents a later Rust duplication
from reintroducing layout-script execution or announcement ambiguity.

### Implementation (concrete code)

Retain the checked count when constructing each rendered slide:

```rust
let reveal_steps = slide.step_count();
let html = render_slide(
    slide.key(),
    slide.slots(),
    slide.layout(),
    breaks,
    SlideRenderAttributes {
        page_number,
        page_total,
        reveal_steps,
    },
    DeckRenderContext {
        highlighter,
        edit_annotations,
        layout_assets,
    },
)?;
let notes = slide.notes().map(|s| s.to_owned());
slides.push(RenderedSlide::new(
    slide.index(),
    slide.key().clone(),
    reveal_steps,
    html,
    notes,
));
```

Add a private, script-safe metadata serializer beside the announcement
helpers:

```rust
#[derive(serde::Serialize)]
struct PdfStepPageSlide<'a> {
    key: &'a str,
    #[serde(rename = "revealSteps")]
    reveal_steps: usize,
}

fn pdf_step_pages_script(slides: &[RenderedSlide]) -> String {
    pdf_step_pages_script_from_entries(
        slides
            .iter()
            .map(|slide| (slide.key().as_str(), slide.reveal_steps())),
    )
}

fn pdf_step_pages_script_from_entries<'a>(
    slides: impl IntoIterator<Item = (&'a str, usize)>,
) -> String {
    let slides = slides
        .into_iter()
        .map(|(key, reveal_steps)| PdfStepPageSlide { key, reveal_steps })
        .collect::<Vec<_>>();
    let slides_json = serde_json::to_string(&slides)
        .expect("slide keys and reveal-step counts serialize as JSON")
        .replace('<', "\\u003c");
    let body = format!(r#"    try {{
      document.head.appendChild(PeithoViewer.createStepStateStyle(document));
      PeithoViewer.expandRevealStepPages(document, {slides_json});
    }} catch (error) {{
      const message = error instanceof Error ? error.message : String(error);
      document.documentElement.setAttribute("data-peitho-pdf-step-pages-error", message);
      throw error;
    }}
"#);
    embedded_distribution_viewer_script(&body)
}
```

Test `pdf_step_pages_script_from_entries` with
`"quoted\"</script><!--"` and count `3`; parse the embedded literal back to
`[{"key":"quoted\"</script><!--","revealSteps":3}]`, while asserting that
the script body contains `\u003c/script>` and no raw closing script tag.

In `render_pdf_document`, derive the optional script from the typed enum:

```rust
let step_pages_script = match settings.public_reveal() {
    PublicReveal::Final => String::new(),
    PublicReveal::Steps => pdf_step_pages_script(deck.slides()),
};
```

Change only the body interpolation at the PDF seam:

```rust
{slides}{announce_script}{step_pages_script}  <script>
{pdf_flatten_js}
  </script>
```

An empty `step_pages_script` leaves the omitted/final formatting path
identical. Do not pass `PublicReveal` to lint, duplicate slide HTML in Rust,
re-run layout scripts, change page-number attributes, or move the existing
announce and flatten scripts.

### Verification command

```sh
cargo test -p peitho-core pdf_final_and_omitted_modes_are_byte_identical
cargo test -p peitho-core pdf_steps_mode_expands_after_announcement_and_before_flattening
cargo test -p peitho-core render_pdf_document
cargo fmt --all --check
```

## Task 4: Fail CDP export when page expansion reports an error

### Goal

Prevent a failed expander from silently producing a final-only or partially
expanded PDF. Preserve the existing shadow-mounted error priority and perform
both checks before `Page.printToPDF` writes the output.

### Files

- `crates/peitho/src/cdp.rs`
- `crates/peitho/src/main.rs`

### Test (concrete code)

Add this unit test beside `shadow_mounted_error_evaluation_fails_with_the_recorded_message`:

```rust
#[test]
fn pdf_step_pages_error_evaluation_fails_with_the_recorded_message() {
    assert_eq!(
        PDF_STEP_PAGES_ERROR_EXPRESSION,
        "document.documentElement?.getAttribute('data-peitho-pdf-step-pages-error') ?? null"
    );
    validate_pdf_status_evaluation(
        &json!({
            "result": { "type": "object", "subtype": "null", "value": null }
        }),
        "PDF reveal-step page expansion",
    )
    .unwrap();

    let err = validate_pdf_status_evaluation(
        &json!({
            "result": {
                "type": "string",
                "value": "failed to clone reveal page 1"
            }
        }),
        "PDF reveal-step page expansion",
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "PDF reveal-step page expansion failed: failed to clone reveal page 1"
    );
}
```

### Implementation (concrete code)

Add the expression next to the two current PDF expressions:

```rust
const PDF_STEP_PAGES_ERROR_EXPRESSION: &str =
    "document.documentElement?.getAttribute('data-peitho-pdf-step-pages-error') ?? null";
```

Use one labeled `CdpClient` method for both PDF status attributes:

```rust
pub fn ensure_pdf_status_succeeded(
    &mut self,
    expression: &str,
    label: &str,
    deadline: Instant,
) -> miette::Result<()> {
    let result = self.call(
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "returnByValue": true
        }),
        deadline,
    )?;
    validate_pdf_status_evaluation(&result, label)
}
```

Replace the two surface-specific validators with one labeled validator while
keeping every resulting error string unchanged:

```rust
fn validate_pdf_status_evaluation(result: &Value, label: &str) -> miette::Result<()> {
    if let Some(exception) = result.get("exceptionDetails") {
        return Err(miette::miette!(
            "Chrome failed to read {label} status: {exception}"
        ));
    }
    let remote_object = result.get("result").ok_or_else(|| {
        miette::miette!("Chrome Runtime.evaluate response omitted its result object")
    })?;
    match remote_object.get("value") {
        Some(Value::String(message)) => Err(miette::miette!("{label} failed: {message}")),
        Some(Value::Null) => Ok(()),
        Some(value) => Err(miette::miette!(
            "Chrome returned unexpected {label} status: {value}"
        )),
        None if remote_object.get("subtype").and_then(Value::as_str) == Some("null") => Ok(()),
        None => Err(miette::miette!(
            "Chrome Runtime.evaluate response omitted the {label} status"
        )),
    }
}
```

In `run_chrome_print_with_timeout`, keep the checks in this order:

```rust
cdp::wait_for_pdf_flattening(&mut client, deadline)?;
client.ensure_pdf_status_succeeded(
    cdp::SHADOW_MOUNTED_ERROR_EXPRESSION,
    "PDF slide initialization",
    deadline,
)?;
client.ensure_pdf_status_succeeded(
    cdp::PDF_STEP_PAGES_ERROR_EXPRESSION,
    "PDF reveal-step page expansion",
    deadline,
)?;

let pdf = client.page_print_to_pdf(deadline)?;
```

The shadow-mounted check stays first so the existing duplicate-section E2E
continues to report its original error when both attributes are present.

### Verification command

```sh
cargo test -p peitho cdp::tests::pdf_step_pages_error_evaluation_fails_with_the_recorded_message
cargo test -p peitho cdp::tests::shadow_mounted_error_evaluation_fails_with_the_recorded_message
cargo fmt --all --check
```

## Task 5: Update the cross-mode artifact regression

### Goal

Pin the intended `public_reveal` blast radius: omitted and explicit `final`
produce identical PDF HTML, `steps` adds the PDF bootstrap, and manifest,
slide fragments, and lint remain identical across all modes.

### Files

- `crates/peitho/src/main.rs`

### Test (concrete code)

Rename and replace `public_reveal_only_changes_distribution_index` with:

````rust
#[test]
fn public_reveal_steps_changes_distribution_index_and_pdf_only() {
    const BODY: &str = r#"# Intro

::: {reveal}

- First item
- Second item

:::

```rust {1|2}
let first = 1;
let second = 2;
```
"#;

    let omitted = stable_public_outputs(BODY);
    let final_mode = stable_public_outputs(&format!("---\npublic_reveal: final\n---\n{BODY}"));
    let steps_mode = stable_public_outputs(&format!("---\npublic_reveal: steps\n---\n{BODY}"));

    assert_eq!(omitted.manifest, final_mode.manifest);
    assert_eq!(omitted.slides, final_mode.slides);
    assert_eq!(final_mode.manifest, steps_mode.manifest);
    assert_eq!(final_mode.slides, steps_mode.slides);
    assert_eq!(omitted.pdf, final_mode.pdf);
    assert_ne!(final_mode.pdf, steps_mode.pdf);
    assert!(!final_mode
        .pdf
        .contains("PeithoViewer.expandRevealStepPages("));
    assert!(steps_mode
        .pdf
        .contains("PeithoViewer.expandRevealStepPages(document, ["));
    assert_eq!(omitted.lint, final_mode.lint);
    assert_eq!(final_mode.lint, steps_mode.lint);
}
````

### Implementation (concrete code)

No production branch belongs in `main.rs`. Keep `StablePublicOutputs` and
`stable_public_outputs` unchanged; only replace the test name and PDF
assertions with the code above. Distribution config coverage remains in
`render.rs`, where Issue #722 already proves that the index selects final or
stepped navigation.

### Verification command

```sh
cargo test -p peitho public_reveal_steps_changes_distribution_index_and_pdf_only
git diff --check -- crates/peitho/src/main.rs
```

## Task 6: Add the ignored Chrome PDF page-count E2E

### Goal

Prove through the real export command and CDP printing path that a two-step
slide produces three PDF pages, a plain slide produces one, and the CLI still
reports two logical slides.

### Files

- `crates/peitho/tests/export_pdf.rs`

### Test (concrete code)

Add this ignored test:

```rust
#[test]
#[ignore]
// Chrome-backed E2E test; run explicitly with cargo test -- --ignored.
fn export_pdf_expands_public_reveal_steps_into_pages() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping export_pdf_expands_public_reveal_steps_into_pages: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("out.pdf");
    fs::write(
        &deck,
        r#"---
public_reveal: steps
---

# Stepped

::: {reveal}

- First item
- Second item

:::

---

# Plain
"#,
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .args(["export", "pdf"])
        .arg(&deck)
        .args(["-o"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("exported 2 slide(s)"));

    let bytes = fs::read(&out).unwrap();
    assert!(bytes.len() > 4);
    assert_eq!(&bytes[..5], b"%PDF-");
    assert_eq!(pdf_page_object_count(&bytes), 4);
}
```

Add this helper beside `pdf_bytes_contain`:

```rust
fn pdf_page_object_count(bytes: &[u8]) -> usize {
    const PAGE_TYPE_TOKENS: [&[u8]; 2] = [b"/Type /Page", b"/Type/Page"];

    PAGE_TYPE_TOKENS
        .iter()
        .map(|token| {
            bytes
                .windows(token.len() + 1)
                .filter(|window| window.starts_with(token) && window[token.len()] != b's')
                .count()
        })
        .sum()
}
```

The byte after `Page` excludes the `/Pages` tree object. Supporting both token
spellings matches the existing PDF tests' handling of Chrome's optional PDF
name whitespace.

Review testing added two more ignored Chrome cases. The
`export_pdf_skips_hidden_box_shadows_without_emitting_images` fixture gives a
hidden heading a box shadow and asserts zero image XObjects.
`export_pdf_skips_box_shadows_under_zero_opacity_ancestor` places a shadowed
child inside `.transparent-shadow-parent { opacity: 0; }` in a custom layout
and also asserts zero image XObjects. Both retain the normal `%PDF-` and font
assertions. Count image objects with the two Chrome spellings:

```rust
fn pdf_image_object_count(bytes: &[u8]) -> usize {
    const IMAGE_SUBTYPE_TOKENS: [&[u8]; 2] = [b"/Subtype /Image", b"/Subtype/Image"];

    IMAGE_SUBTYPE_TOKENS
        .iter()
        .map(|token| {
            bytes
                .windows(token.len())
                .filter(|window| *window == *token)
                .count()
        })
        .sum()
}
```

### Implementation (concrete code)

This task adds test code only. Do not add a PDF parser dependency, change the
CLI's logical slide count, or make the test non-ignored. The Chrome-enabled
runner supplies `PEITHO_CHROME_PATH`; a missing browser retains the existing
skip behavior used by this test file.

### Verification command

```sh
cargo test -p peitho --test export_pdf export_pdf_expands_public_reveal_steps_into_pages -- --ignored --exact --nocapture
```

## Task 7: Update author documentation and repository invariants

### Goal

Describe the PDF opt-in accurately, remove every statement that PDF always
shows only the final reveal state, and record the ordering and no-script-rerun
constraints for future changes.

### Files

- `site/content/guide/frontmatter.md`
- `site/content/guide/writing-decks.md`
- `site/content/guide/layouts.md`
- `site/content/examples/incremental-reveal.md`
- `site/content/examples/code-emphasis.md`
- `README.md`
- `CLAUDE.md`

### Test (concrete code)

Use these documentation contract checks after editing:

```sh
rg -q 'PDF exports N \+ 1 pages' site/content/guide/frontmatter.md
rg -q 'keeps its own slide number' site/content/guide/frontmatter.md
rg -q 'logical slide total' site/content/guide/frontmatter.md
rg -q 'PDF export emits one page per position' site/content/guide/writing-decks.md
rg -q 'PDFs with `public_reveal: steps`' site/content/guide/writing-decks.md
rg -q 'PDF exports that set `public_reveal: steps`' site/content/guide/writing-decks.md
rg -q 'data-peitho-pdf-step-pages-error' CLAUDE.md
rg -q 'cloneNode\(true\)' CLAUDE.md
rg -q 'before `pdf_flatten.js`' CLAUDE.md
```

### Implementation (concrete code)

In `frontmatter.md`, change the table purpose to:

```markdown
| `public_reveal` | Published distribution and PDF reveal behavior: `final` (default) or `steps`. |
```

Replace the current “Published reveal steps” section with this contract:

````markdown
## Public reveal steps

`public_reveal` controls the viewer written by `peitho build` and reveal-page
expansion in `peitho export pdf`. The default is `final`: the published viewer
moves one whole slide at a time, and PDF writes one fully revealed page per
slide. Use `steps` to make the viewer walk incremental reveal and stepped
code-emphasis states:

```yaml
public_reveal: steps
```

In `steps` mode, the viewer walks positions 0 through N for a slide with N
reveal steps, while PDF exports N + 1 pages over those same positions. A slide
without steps still exports one page. Every expanded PDF page keeps its own
slide number and the logical slide total, so three pages for slide 2 still
display `2`, or `2 / total`, rather than being renumbered as PDF pages.

Only `final` and `steps` are accepted. `peitho present` keeps its normal step
navigation in either mode. Preview and lint remain fully revealed.
````

In `writing-decks.md`, make the incremental-reveal output paragraph say:

```markdown
Published distributions and PDF export show the final state by default. With
`public_reveal: steps`, the published viewer walks the same step sequence and
PDF export emits one page per position, step 0 through step N. A slide with N
steps therefore contributes N + 1 PDF pages; a slide without steps contributes
one. Repeated PDF pages keep the slide's own page number and the logical slide
total. Preview and lint always show the final state.
```

Update the stepped-emphasis table's two stepped rows so their “Where it
appears” cells read:

```markdown
| `{\|3}` | one | `peitho present`, distributions with `public_reveal: steps`, and PDFs with `public_reveal: steps` |
| `{2-4\|6-8}` | one per group | `peitho present`, distributions with `public_reveal: steps`, and PDFs with `public_reveal: steps` |
```

Replace the paragraph that says stepped emphasis is absent from every PDF with:

```markdown
Static emphasis says "these lines are the important ones" — a property of the
content, so it ships with the deck. Stepped emphasis is a pointer that follows
your narration. `peitho preview`, lint, and default/final published and PDF
output leave that pointer inactive. PDFs with `public_reveal: steps` emit each
exact-match emphasis state in the shared step sequence.
```

Replace the later styling sentence with:

```markdown
Stepped emphasis is styled by `peitho present`, by distributions that set
`public_reveal: steps`, and by PDF exports that set `public_reveal: steps`.
```

Make these precise invariant changes in `CLAUDE.md`:

- In the frontmatter bullet, record the two typed consumer paths as
  `DeckFrontmatter` -> `DeckSettings` -> `emit_distribution` ->
  `render_distribution_index` and `DeckFrontmatter` -> `DeckSettings` ->
  `render_pdf_document`. State that no other renderer reads `public_reveal`.
- In the layout-scripts bullet, state that steps-mode PDF embeds a third copy
  of the viewer runtime after the exactly-once parsed-slide announcement,
  receives ordered `{key,revealSteps}` records from Rust, rejects wrap-count
  and key-order mismatches, ignores DOM `data-reveal-steps` as an authority,
  uses `cloneNode(true)`, never calls `executeInlineScripts`, and completes
  before `pdf_flatten.js`.
- In the incremental-reveal bullet, state that omitted/final emits one final
  page per slide, while steps emits positions 0 through N, preserves logical
  page-number attributes, and reports expansion failures through
  `data-peitho-pdf-step-pages-error` before CDP printing.
- In the code-emphasis bullet, add steps-mode PDF as an `applyStepState`
  consumer and retain equality rather than cumulative activation.

In `layouts.md`, record that step pages are cloned immediately after the
parsed-slide announcement. DOM and synchronous canvas bitmaps present then
reach every clone; later drawing, including drawing before `load`, and other
script-held state reach only the original final page. Update `README.md` and
both example pages so final is clearly the default and `public_reveal: steps`
enables both viewer navigation and PDF positions.

### Verification command

```sh
make demo-site
rg -q 'PDF exports N \+ 1 pages' site/content/guide/frontmatter.md
rg -q 'keeps its own slide number' site/content/guide/frontmatter.md
rg -q 'logical slide total' site/content/guide/frontmatter.md
rg -q 'PDF export emits one page per position' site/content/guide/writing-decks.md
rg -q 'PDFs with `public_reveal: steps`' site/content/guide/writing-decks.md
rg -q 'PDF exports that set `public_reveal: steps`' site/content/guide/writing-decks.md
rg -q 'data-peitho-pdf-step-pages-error' CLAUDE.md
rg -q 'cloneNode\(true\)' CLAUDE.md
rg -q 'before `pdf_flatten.js`' CLAUDE.md
git diff --check -- site/content/guide/frontmatter.md site/content/guide/writing-decks.md CLAUDE.md
```

## Changes after review

Four review rounds tightened the accepted design without changing its central
browser-expansion architecture:

- Rust became the sole authority for PDF keys and reveal-step counts.
  `RenderedSlide` retains the checked count, `render_pdf_document` embeds an
  ordered, script-safe `[{key,revealSteps}]` literal, and the browser throws on
  count, key, structure, or numeric mismatch instead of silently skipping.
- Cloned canvases receive source bitmaps by paired DOM order. Zero-size sources
  are skipped; missing 2D contexts and `drawImage` failures carry the slide key
  and canvas index. Other property state and later asynchronous work remain on
  the original final page.
- `pdf_flatten.js` now skips invisible and zero-effective-opacity shadow
  targets and applies effective opacity below `.peitho-slide` to appended outer
  shadow images. Inset shadows keep their existing path. This is a correctness
  fix for omitted/final PDFs as well as steps PDFs, preventing ghost shadows
  and double opacity.
- Chrome coverage grew to three ignored tests: page-count expansion, hidden
  shadow suppression, and suppression under a zero-opacity ancestor. The test
  file owns both `pdf_page_object_count` and `pdf_image_object_count`.
- CDP uses one `(expression, label)` evaluator and one labeled validator for
  slide initialization and reveal-step expansion, preserving all existing
  error strings. Renderer lint assertions were narrowed to actual wrapper CSS
  or markup because the rebuilt viewer bundle legitimately contains expander
  selectors.
- Documentation corrections include `README.md`, `layouts.md`, both reveal
  example pages, the frontmatter and writing guides, and the `CLAUDE.md`
  box-shadow and Rust-metadata invariants.

## Task 8: Run the full gates and Chrome acceptance test

### Goal

Pass every repository gate, prove generated contracts and unrelated bundles
have no drift, and run the ignored real-Chrome page-count test before commit.

### Files

No file is intentionally edited in this task.

### Test (concrete code)

Run the complete gate list from `CLAUDE.md`. Invoke every displayed line from
the repository root in its own shell. The first command must succeed three
consecutive times:

```sh
cargo test --workspace          # run 3 times in a row (past test-race incidents)
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/  # contract drift
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js  # embedded shell drift (after npm run build)
git diff --exit-code packages/peitho-present/dist/preview.js  # embedded preview drift (after npm run build)
git diff --exit-code packages/peitho-present/dist/remote.js  # embedded remote drift (after npm run build)
git diff --exit-code packages/peitho-present/dist/viewer.js  # embedded viewer drift (after npm run build)
```

Then run the Chrome acceptance test from the repository root:

```sh
cargo test -p peitho --test export_pdf export_pdf_expands_public_reveal_steps_into_pages -- --ignored --exact --nocapture
```

### Implementation (concrete code)

Make no production or documentation change in this task. If a command fails,
add the regression to the test file that owns the behavior, fix the owning
implementation, rebuild `dist/viewer.js`, and restart this task at the first
workspace test run. Do not weaken a gate, remove the ignored E2E assertion, or
commit a generated binding change.

### Verification command

```sh
git diff --check
git status --short
```

## Completion

Issue #723 is complete only when steps-mode PDF uses the shared DOM state to
print N+1 pages, layout scripts still run once, expansion failures stop CDP
printing, logical page
numbers remain unchanged, omitted/final mode isolation holds, the ignored
Chrome test reports four pages for the two-slide fixture, documentation is
current, `dist/viewer.js` is rebuilt, and every final gate passes.
