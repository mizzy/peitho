# PDF export: flatten box-shadow at print time (Issue #150)

<!-- derived-from ./2026-07-06-pdf-gradient-flatten.md -->
<!-- constrained-by ../../CLAUDE.md -->

## Summary

PDFs produced by `peitho export pdf`, when opened in macOS Preview.app / Quick Look / `sips`, show hard black rectangles around `box-shadow` elements. They do not appear in browser display, poppler, or Chrome's PDF viewer. The root cause is that Chrome `--print-to-pdf` emits blurred `box-shadow` as **black-fill + `/SMask /S /Luminosity` ExtGState + a Form XObject with a DeviceGray mask image**, and Quartz-family renderers miscomposite the Luminosity SMask. Like Issue #142's gradient flatten, a script inside the PDF-export-only `pdf.html` **rasterizes only box-shadows into RGBA PNGs and replaces them**. Body text and normal DOM stay vector.

## Measured facts (2026-07-06, no need to re-investigate)

- Removing `box-shadow` from the deck CSS eliminates the black rectangles in Preview.app. HTML display never shows them
- Chrome PDF emits blurred shadows as `/S /Luminosity`, and only Quartz-family renderer APIs paint them fully black up to the BBox. poppler / Chrome's PDF viewer render them correctly
- Rasterizing the shadow into an RGBA PNG on canvas and laying it behind the element as an `<img>`, then having Chrome print, results in a correct soft shadow even in Quartz
- When RGBA PNGs are PDF-ized, the image XObject itself sometimes carries an `/SMask`. The test forbids not `/SMask` in general but the `/S /Luminosity` that causes Quartz misrendering
- The existing `--virtual-time-budget=10000` was already confirmed to converge `pdf_flatten.js`'s async work before print during the gradient work

## Design decisions

- **Extend the existing flatten**. Target is only `crates/peitho-core/src/pdf_flatten.js`. It is a PDF-export-only JS embedded into `pdf.html` by `render_pdf_document` via `include_str!`; not part of the npm package or TS bundle
- **Execution order is gradient → shadow**. Inset shadow prepends into the head layer of `background-image`, so it rewrites background after gradient flatten. Top-level awaits `waitForStableLayout()` once, then calls `flattenBoxShadows()` after `flattenGradients()`
- **Parse computed `boxShadow` in a small custom parser**. Chrome computed form example: `rgba(0, 0, 0, 0.45) 0px 18px 48px 0px`. Comma splitting respects parenthesis depth. Categorize `inset`; support only px lengths. Handles commas inside `rgba()`, multiple shadows, negative offset/spread. Any unsupported value causes per-element `console.error` and leaves the original `box-shadow` in place
- **Multiple shadows preserve CSS stacking order**. In CSS, list head is frontmost. In canvas, later draws are on top, so when drawing into the same canvas, draw the shadow list back to front
- **Outer shadow becomes an absolutely-positioned PNG child of the slide**. Find the target element's `.peitho-slide`, use `slideRect.width / slide.offsetWidth` as scale, and derive padding-box local coordinates as `(elementRect - slideRect) / scale - slide.clientLeft/clientTop`. `.peitho-slide`'s `transform: scale(...)` is a stacking context and the containing block for absolute positioning, so insert an `<img>` with `position:absolute; z-index:-1; pointer-events:none` directly under the slide, and set the element's `box-shadow` to `none !important`
- **Accept the approximation limits of outer shadow**. A real CSS `box-shadow` paints inside the element's paint position; the replacement PNG becomes a back layer directly under the slide. This matches for typical card shadows, but with sibling elements that have opaque backgrounds or complex z-index, the stacking with siblings may change. Prioritize eliminating the Preview.app black rectangles; treat such cases as fallback candidates or known-approximation differences
- **Inset shadow becomes the topmost background layer PNG**. Inset shadow paints on top of background and beneath content, so prepend as `background-image: url(data), <old>`, and equally prepend the head layer for `background-size/position/repeat/origin/clip`. When `background-attachment: fixed`, non-`normal` `background-blend-mode`, or values that can't be safely list-manipulated appear, `console.error` and leave the original in place
- **Shadow drawing is canvas API only**. Outer shadows use `ctx.shadowColor/shadowBlur/shadowOffset*` plus the "place the rectangle far away and let only the shadow land inside the canvas" trick. `shadowBlur` / `shadowOffsetX/Y` are unaffected by canvas CTM, so do not depend on `context.scale(SCALE, SCALE)`; explicitly multiply rectangle coordinates, radius, blur, offset, and farAway by `SCALE`. `border-radius` is reflected via `roundRect` only when computed corner radius is a single-px value. Spread is emulated by inflate + radius adjust. Canvas margin reserves `blur * 2 + Math.abs(offset) + Math.max(0, spread)` per axis. `SCALE = 2` and canvas upper bound match the existing gradient flatten
- **Per-element all-or-nothing**. If any of outer/inset, or any one of multiple shadows, is unsupported, do not rewrite the DOM for that element. Build all PNGs before applying, then perform layer insertion / background prepend / `box-shadow:none` atomically. On mid-flight failure, remove nodes that were already inserted
- **Skip conditions are explicit-log fallbacks**. `getClientRects().length !== 1`, `transform !== "none"` in the ancestor chain from the target up to (excluding) `.peitho-slide`, `.peitho-slide` not found, zero size, non-px border radius, canvas upper bound exceeded → `console.error("peitho pdf shadow flatten:", describeElement(...), reason)` and leave as-is

## Non-goals

- `text-shadow` and `filter: drop-shadow(...)` are not covered by Issue #150. Both are siblings of the Quartz PDF problem but require rasterizing font glyphs or arbitrary-alpha silhouettes — a different problem from rectangular box-shadow
- No full-page rasterization or `--rasterize` option. Keep vector text
- Do not aim for full CSS Shadows Level 4 syntax compatibility. Target only computed shadows with px resolutions that can be safely replaced in PDF export

## Implementation tasks (TDD)

### Task 1: add red E2E

- Target: `crates/peitho/tests/export_pdf.rs`
- Modeled on the existing `export_pdf_flattens_gradient_backgrounds_to_images`, add a Chrome-required `#[ignore]` test first and confirm red
- Test name: `export_pdf_flattens_box_shadows_without_luminosity_smask_to_images`
- Minimum CSS in deck-adjacent `css/shadow.css`. Raw HTML blocks turn into `unsupported html` in `process_html_chunk`, so use CSS-only against Markdown-derived DOM:

```css
.peitho-slide {
  background: #f8fafc;
  color: #111827;
}
.peitho-slide h1 {
  display: inline-block;
  padding: 24px 48px;
  border-radius: 28px;
  background: white;
  box-shadow: rgba(0, 0, 0, 0.45) 0px 18px 48px 0px;
}
```

- Deck body is plain Markdown only:

```markdown
# Vector Text

Box shadow PDF export
```

- Reason for pre-implementation red: currently Chrome emits `h1`'s `box-shadow` as Luminosity SMask, so the following asserts fail. Having the `h1` itself be the shadow target also means an implementation that mistakenly rasterizes the whole element will lose the `/Font` canary and get caught

```rust
assert!(!pdf_bytes_contain(&bytes, b"/S /Luminosity"));
assert!(
    pdf_bytes_contain(&bytes, b"/Subtype /Image")
        || pdf_bytes_contain(&bytes, b"/Subtype/Image")
);
assert!(pdf_bytes_contain(&bytes, b"/Font"));
```

- Note: do not write `assert!(!pdf_bytes_contain(&bytes, b"/SMask"))`. It would also forbid the alpha mask of RGBA image XObjects

### Task 2: reshape `pdf_flatten.js` top-level into two-stage flatten

- Target: `crates/peitho-core/src/pdf_flatten.js`
- Move `waitForStableLayout()` up to top-level orchestration; make the existing gradient handling an internal function that assumes waiting is done
- Code fragment shape:

```js
async function flattenPdfArtifacts() {
  await waitForStableLayout();
  var gradientCount = await flattenGradients();
  var shadowCount = await flattenBoxShadows();
  document.documentElement.setAttribute("data-peitho-pdf-flattened", String(gradientCount + shadowCount));
  document.documentElement.setAttribute("data-peitho-pdf-shadow-flattened", String(shadowCount));
}
```

- Keep the existing `try/catch` policy; set attributes even on top-level failure
- Target: `crates/peitho-core/src/render.rs`
  - Broaden the existing unit test to be the equivalent of `pdf_document_embeds_pdf_flattening_script_after_slides`
  - Assertion examples: `assert!(html.contains("flattenGradients")); assert!(html.contains("flattenBoxShadows"));`
  - Keep the test that `PDF_FLATTEN_JS` does not contain `</script`

### Task 3: `box-shadow` parser and target collection

- Target: `crates/peitho-core/src/pdf_flatten.js`
- Small helpers to add:

```js
function splitCssList(value) { /* comma split with parentheses depth */ }
function parseSignedPixel(value) { /* /^-?\d+(\.\d+)?px$/ */ }
function parseShadowList(boxShadow) { /* [{ inset, color, offsetX, offsetY, blur, spread }] */ }
function parseCornerRadius(value) { /* "12px" only; reject "12px 8px" and non-px */ }
```

- Parser policy:
  - `none` or empty string is skipped
  - Strip `inset` keyword
  - Assume Chrome computed form; treat the leading `rgb(...)` / `rgba(...)` as the color
  - Rest is `offsetX offsetY blur? spread?`. Offsets are required; blur/spread default to 0 when omitted
  - Blur cannot be negative. Spread can be negative
  - If any part fails to parse, skip the element as a whole
- `.peitho-slide` resolution, skip conditions, coordinates:

```js
var slide = element.closest(".peitho-slide");
if (!slide) throw new Error("element is outside .peitho-slide");
if (hasTransformBeforeSlide(element, slide)) throw new Error("transformed element ancestor");
if (element.getClientRects().length !== 1) throw new Error("fragmented element");

var slideRect = slide.getBoundingClientRect();
var elementRect = element.getBoundingClientRect();
var scale = slideRect.width / slide.offsetWidth;
var localX = (elementRect.left - slideRect.left) / scale - slide.clientLeft;
var localY = (elementRect.top - slideRect.top) / scale - slide.clientTop;
var width = elementRect.width / scale;
var height = elementRect.height / scale;
if (width <= 0 || height <= 0) throw new Error("zero-sized element");
```

- `hasTransformBeforeSlide` walks from the target upward and stops at `.peitho-slide`. The slide's own print-time `transform: scale(...)` is allowed, but a transform on an intermediate ancestor would warp the local coordinates and is skipped:

```js
function hasTransformBeforeSlide(element, slide) {
  for (var node = element; node && node !== slide; node = node.parentElement) {
    if (getComputedStyle(node).transform !== "none") return true;
  }
  return false;
}
```

### Task 4: replace outer shadow with a PNG layer

- Target: `crates/peitho-core/src/pdf_flatten.js`
- Draw only outer shadows into one transparent PNG. Canvas size is `(width + padLeft + padRight) * SCALE` / `(height + padTop + padBottom) * SCALE`
- Drawing order and spread/radius adjustment. Because `shadowBlur` / `shadowOffsetX/Y` are not CTM-scaled, do not use `context.scale(SCALE, SCALE)`; convert to canvas coordinates explicitly:

```js
var s = SCALE;
outerShadows.slice().reverse().forEach(function (shadow) {
  var inflated = scaleRect(inflateRect(baseRect, shadow.spread), s);
  var radius = scaleRadius(adjustRadius(cornerRadii, shadow.spread), s);
  var far = farAway * s;
  context.shadowColor = shadow.color;
  context.shadowBlur = shadow.blur * s;
  context.shadowOffsetX = shadow.offsetX * s + far;
  context.shadowOffsetY = shadow.offsetY * s;
  fillRoundedRect(context, inflated.x - far, inflated.y, inflated.width, inflated.height, radius);
});
```

- For Chrome environments without `roundRect`, use a small path helper when `ctx.roundRect` is missing
- Application fragment:

```js
var image = document.createElement("img");
image.src = dataUrl;
await image.decode();
image.setAttribute("data-peitho-pdf-shadow", "outer");
Object.assign(image.style, {
  position: "absolute",
  left: (target.localX - padLeft) + "px",
  top: (target.localY - padTop) + "px",
  width: cssWidth + "px",
  height: cssHeight + "px",
  zIndex: "-1",
  pointerEvents: "none",
  maxWidth: "none"
});
target.slide.appendChild(image);
```

- Do `await image.decode()` before inserting into the DOM. Reusing the existing `loadImage` helper is fine too — either way, image decode completion is made deterministic against print timing under virtual time
  - **[Deprecated 2026-07-06 — Issue #155]** This `decode()` recommendation was found to hang silently under Linux headless Chrome + `--virtual-time-budget` (neither resolves nor rejects), and was unified to `loadImage` (wait on load event). Do not reintroduce `decode()`. Details: `docs/plans/2026-07-06-pdf-flatten-linux-decode-hang.md`
- Because it is all-or-nothing, do not set `box-shadow:none` at this point yet

### Task 5: replace inset shadow with the head background layer

- Target: `crates/peitho-core/src/pdf_flatten.js`
- Draw only inset shadows into a transparent PNG sized to the element's border box. Do not depend on `context.scale(SCALE, SCALE)` here either — explicitly multiply clip path, ring path, radius, blur, offset by `SCALE`
- Drawing model is the same as CSS: "the outside of the box drops a shadow inward". Do not flip the sign of offset. Procedure:
  - Clip with a border-box rounded rectangle
  - Build an even-odd hollow path of "huge outer rectangle + inner rounded-hole deflated by spread"
  - Set `ctx.shadowColor/shadowBlur/shadowOffsetX/Y` with the same-signed offset and fill
  - Only the shadow cast by the hollow ring enters the clip, becoming the topmost background PNG
- Drawing fragment:

```js
var s = SCALE;
clipRoundedRect(context, scaleRect(borderBox, s), scaleRadius(cornerRadii, s));
insetShadows.slice().reverse().forEach(function (shadow) {
  var inner = scaleRect(deflateRect(borderBox, shadow.spread), s);
  var innerRadius = scaleRadius(adjustRadius(cornerRadii, -shadow.spread), s);
  context.shadowColor = shadow.color;
  context.shadowBlur = shadow.blur * s;
  context.shadowOffsetX = shadow.offsetX * s;
  context.shadowOffsetY = shadow.offsetY * s;
  context.beginPath();
  context.rect(-huge * s, -huge * s, (width + huge * 2) * s, (height + huge * 2) * s);
  appendRoundedRectPath(context, inner, innerRadius);
  context.fill("evenodd");
});
```

- Background list manipulation snapshots existing computed values first. Safety conditions:
  - `backgroundAttachment` does not contain `fixed`
  - `backgroundBlendMode` is `normal` on all layers
  - `splitCssList` can split `backgroundImage/Size/Position/Repeat/Origin/Clip`
- Prepend fragment:

```js
setImportant(style, "background-image", 'url("' + dataUrl + '"), ' + old.backgroundImage);
setImportant(style, "background-size", width + "px " + height + "px, " + old.backgroundSize);
setImportant(style, "background-position", "0 0, " + old.backgroundPosition);
setImportant(style, "background-repeat", "no-repeat, " + old.backgroundRepeat);
setImportant(style, "background-origin", "border-box, " + old.backgroundOrigin);
setImportant(style, "background-clip", "border-box, " + old.backgroundClip);
```

- When `old.backgroundImage === "none"`, use a single layer with no tail
- Only after all outer/inset applications succeed:

```js
setImportant(target.element.style, "box-shadow", "none");
```

### Task 6: documentation update

- Target: `CLAUDE.md`
- Add one line to Pitfalls:

```markdown
- **PDF export flattens box-shadow at print time**: Chrome emits blurred CSS box-shadows as `/S /Luminosity` soft masks that Quartz renders as hard black rectangles, so `pdf_flatten.js` rasterizes supported shadows to RGBA PNGs for PDF export (measured 2026-07-06). Design record: `docs/plans/2026-07-06-pdf-shadow-flatten.md`
```

- Treat this file as the design record for Issue #150. No README update needed

## Verification steps (at verify time)

1. Confirm red: `PEITHO_CHROME_PATH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" cargo test -p peitho --test export_pdf export_pdf_flattens_box_shadows_without_luminosity_smask_to_images -- --ignored`
2. Post-green E2E: `cargo test -p peitho --test export_pdf -- --ignored`
3. All gates:

```bash
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/measure.js
```

4. Quartz visual A/B check. Instead of poppler, split pages with `pdfseparate` and Quartz-render with `sips`:

```bash
mkdir -p /tmp/peitho-shadow-ab
peitho export pdf path/to/shadow-deck.md -o /tmp/peitho-shadow-ab/before.pdf
# After implementation:
cargo run -p peitho -- export pdf path/to/shadow-deck.md -o /tmp/peitho-shadow-ab/after.pdf
pdfseparate /tmp/peitho-shadow-ab/before.pdf /tmp/peitho-shadow-ab/before-%02d.pdf
pdfseparate /tmp/peitho-shadow-ab/after.pdf /tmp/peitho-shadow-ab/after-%02d.pdf
sips -s format png /tmp/peitho-shadow-ab/before-01.pdf --out /tmp/peitho-shadow-ab/before-01-quartz.png
sips -s format png /tmp/peitho-shadow-ab/after-01.pdf --out /tmp/peitho-shadow-ab/after-01-quartz.png
```

5. PDF byte check:

```bash
grep -a "/S /Luminosity" /tmp/peitho-shadow-ab/after.pdf
grep -a "/Subtype */Image\\|/Subtype/Image" /tmp/peitho-shadow-ab/after.pdf
```

Expected: `after` has 0 hits for `/S /Luminosity`, an image XObject exists, and Quartz PNG loses the black rectangles while keeping soft shadows only.
