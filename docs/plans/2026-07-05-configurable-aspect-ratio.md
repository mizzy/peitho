# Configurable slide aspect ratio (Issue #23)

## Summary

Move the slide canvas size from a fixed 1280×720 (16:9) to something selectable via the frontmatter `aspect_ratio` key. v1 accepts two values: `16:9` (default) and `4:3`.

## Motivation

- Presentation needs on 4:3 screens and older projectors
- Issue #109 (PDF export) presupposes the canvas size, so this wants to land as a prerequisite
- Currently `1280` and `720` are **scattered across 6+ locations** (TS constants, CSS, embedded HTML/JS in Rust, tests); there should be a single source of truth

## Non-goals

- Specifying the logical resolution itself (e.g. `resolution: 1920x1080`) is **out of scope**. That is handled by #109 (PDF export) under the `resolution` key ([see the #109 comment](https://github.com/mizzy/peitho/issues/109#issuecomment-4885813888))
- Aspect ratios other than `16:9` / `4:3` (`16:10`, `21:9`, etc.) are not accepted in v1. Additions are a future non-breaking extension

## Design decisions

### Key name: `aspect_ratio`

`aspect_ratio` rather than `aspect`. Reasons:
- Aligns with the CSS `aspect-ratio` property and the Reveal.js / Slidev ecosystem
- `aspect` alone leans toward "side / viewpoint" and is ambiguous as a settings key
- Two words that close in on meaning, matching the existing keys (`time`, `layouts`, `css`, `syntaxes`)

### Value: `W:H` string, restricted to `16:9` and `4:3`

- Only `"16:9"` or `"4:3"` are legal values
- Anything else is a **line-numbered build error** (same style as existing frontmatter validation)
- Unspecified defaults to `16:9` (the current 1280×720)

Chosen to preserve these invariants:
- Silent path is forbidden → invalid values must error
- "Accept but ignore" is also forbidden → do not create variants without a consumer

The logical resolution is a fixed internal mapping (keeps the TS/CSS/Rust triple in sync trivially):
- `16:9` → 1280 × 720
- `4:3` → 960 × 720 (keep height at 720. This avoids re-tuning the base.css font sizes that assume a 720px canvas)

**Why hold height at 720**: base.css is designed around `font-size: 56px` etc. on a 720px-tall canvas. Varying only width keeps existing themes readable as-is on 4:3. If a user finds "4:3 fonts too small," they can address it via custom CSS.

### Type: `AspectRatio` enum (Rust)

Introduce `AspectRatio` in `peitho-core`. Changed from a newtype to an enum during review. Reasons:
- The invariant "only `16:9` / `4:3` are legal" is expressed by the type itself
- serde `Deserialize` expresses the wire label (`"16:9"` / `"4:3"`) ↔ variant correspondence
- `FromStr` is implemented once in `domain.rs`; the frontmatter parser delegates to it
- Expose `pub fn width(self) -> u32` / `pub fn height(self) -> u32`. Consumers read values through these
- Default is `16:9` (1280 × 720)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectRatio {
    #[serde(rename = "16:9")]
    Ratio16To9,
    #[serde(rename = "4:3")]
    Ratio4To3,
}
```

Implement `FromStr` on `AspectRatio` and have the parser delegate via `value.parse::<AspectRatio>()`. The parser converts other values into `BuildError` with line numbers.

### Add `aspect_ratio` to DeckSettings

Add `aspect_ratio: AspectRatio` to `DeckSettings::new`. It rides on `Deck<P>` through every phase. Treated the same as the existing `planned_time` / `sections` / `layouts` / `css` / `syntaxes`.

### Single source of truth

**Rust side (`AspectRatio`) is the single source.** Three propagation paths:

1. **Emit into manifest.json** → `aspectRatio` is a semantic string (`"16:9"` / `"4:3"`); `canvasWidth` / `canvasHeight` are numeric values derived on the Rust side from `AspectRatio`. `shell.ts` passes the numeric fields into `installCanvasScaler`, so the TS side does not duplicate the label→pixel mapping
2. **Turn `themes/base.css` into CSS custom properties**: `width: 1280px` → `width: var(--peitho-canvas-width, 1280px)`, `height: 720px` → `var(--peitho-canvas-height, 720px)`. `shell.ts` injects `--peitho-canvas-width` / `--peitho-canvas-height` / `--peitho-canvas-aspect` onto the shell root
3. **Embedded HTML/JS in `crates/peitho-core/src/render.rs`**: `render_distribution_index` / present / presenter templates derive width / height / CSS aspect (`16 / 9` or `4 / 3`) from `Deck<Rendered>`'s `AspectRatio`. The presenter's `.stage` / `.slide-pane` / `.next-preview` use `var(--peitho-canvas-aspect)`

### Manifest schema extension

The wire form is camelCase and carries the semantic label plus derived numeric dimensions:

```json
{
  "aspectRatio": "16:9",
  "canvasWidth": 1280,
  "canvasHeight": 720,
  ...
}
```

`aspectRatio` is a semantic string (`"16:9" | "4:3"`). Rust-internal `Manifest` carries only `aspect_ratio: AspectRatio`; the `canvas_width()` / `canvas_height()` accessors derive from `aspect_ratio`. serde goes through a private `ManifestWire`. On Deserialize, `aspectRatio` is authoritative and any `canvasWidth` / `canvasHeight` in the JSON are accept-and-drop (implementation-wise: read via `#[serde(default)]` and discard when converting to `Manifest`). On Serialize, `ManifestWire::from(&Manifest)` fills the numerics from `AspectRatio::width()` / `height()`.

`bindings/Manifest.ts` also exposes `aspectRatio`, `canvasWidth`, `canvasHeight` (via ts-rs generation). The existing CI drift check covers this.

## Scope of changes (implementation scope handed to Codex)

Rust (peitho-core):
- [ ] `domain.rs` or new file: add `AspectRatio` enum
- [ ] `phase.rs::DeckSettings`: add `aspect_ratio: AspectRatio` field, add argument to `new()` (update all existing callers), add `aspect_ratio()` accessor
- [ ] `parser.rs::DeckFrontmatter`: add `aspect_ratio: Option<String>` field → validate → convert to `AspectRatio` → pass into `DeckSettings::new`
- [ ] `parser.rs::frontmatter_key_lines`: add `"aspect_ratio"`
- [ ] Invalid values (`"16:10"` etc.) become line-numbered build errors (same style as the existing invalid-time error)
- [ ] `manifest.rs`: add `aspect_ratio: AspectRatio` on `Manifest`; `canvasWidth` / `canvasHeight` are derived on serialization via the private `ManifestWire`
- [ ] `render.rs::render_distribution_index` / present / presenter templates: take `aspect_ratio: AspectRatio` in the signature and drive 1280/720/16:9 in the embedded HTML/JS/CSS from that value

Rust (peitho crate):
- [ ] `crates/peitho/tests/build.rs`: test the existing `assert!(...contains("const CANVAS_WIDTH = 1280"))` family against both the default (unspecified) 16:9 and 4:3
- [ ] `crates/peitho/tests/build.rs::base_theme_targets_fixed_canvas_size`: test the CSS custom property injection after the change
- [ ] `crates/peitho/src/main.rs`: same assert updates

TS (peitho-present):
- [ ] `packages/peitho-present/src/canvas.ts`: `installCanvasScaler` takes `canvasWidth`/`canvasHeight` as required options. Delete the `?? CANVAS_WIDTH` / `?? CANVAS_HEIGHT` fallbacks and the exported constants
- [ ] `packages/peitho-present/src/shell.ts`: read `canvasWidth` / `canvasHeight` from the manifest and pass them into `installCanvasScaler`; inject `--peitho-canvas-width` / `--peitho-canvas-height` / `--peitho-canvas-aspect` on the shell root
- [ ] `bindings/Manifest.ts`: auto-generated by ts-rs → commit

CSS (themes/base.css):
- [ ] `.peitho-slide { width: 1280px; height: 720px }` → `width: var(--peitho-canvas-width, 1280px); height: var(--peitho-canvas-height, 720px);` (fallback is 16:9)

Examples:
- [ ] Add `aspect-ratio-4-3/` under `examples/` (doubles as smoke test and documentation)

Docs:
- [ ] The frontmatter key enumeration in `CLAUDE.md` should mention `aspect_ratio`, but Codex does not edit `CLAUDE.md` on this branch. Report as a memo-worthy invariant
- [ ] This plan file

## Test plan (TDD order)

Instruct Codex on Red → Green → Refactor. Order:

1. **AspectRatio enum unit tests** (`peitho-core/src/domain.rs`): `Ratio16To9.width() == 1280`, `Ratio4To3.height() == 720`, `Default::default() == Ratio16To9`, `FromStr` accepts `"16:9"` / `"4:3"`
2. **Frontmatter parser tests** (`peitho-core/src/parser.rs`):
   - `aspect_ratio: 16:9` → `DeckSettings::aspect_ratio() == Ratio16To9`
   - `aspect_ratio: 4:3` → `DeckSettings::aspect_ratio() == Ratio4To3`
   - Unspecified → default 16:9
   - `aspect_ratio:` → line-numbered build error (`aspect_ratio has no value`)
   - `aspect_ratio: 16:10` → line-numbered build error (`error.line == matching line`)
   - `aspect_ratio: 1920x1080` → line-numbered build error. Error message only enumerates accepted values (`use one of: 16:9, 4:3`). Do not hint at `resolution:` at all (until #109 lands, hinting is a weak commitment)
3. **Manifest output tests** (`peitho-core/src/manifest.rs`): `aspectRatio`, `canvasWidth`, `canvasHeight` are emitted with correct values. A contradictory wire (`"aspectRatio":"4:3","canvasWidth":1280`) deserializes to `canvas_width() == 960`
4. **Render output tests** (`peitho-core/src/render.rs`): `render_distribution_index(4:3)` HTML/JS contains `CANVAS_WIDTH = 960`
5. **TS canvas.ts tests** (vitest): `calculateCanvasFit` honors `canvasWidth`/`canvasHeight` (check existing tests, add if missing)
6. **CSS custom-property tests** (`peitho/tests/build.rs`): `.peitho-slide` carries the CSS variable fallback
7. **E2E**: `peitho build examples/aspect-ratio-4-3/`, verify `dist/index.html` has CANVAS_WIDTH 960 and manifest.json has `aspectRatio` `"4:3"`, `canvasWidth` `960`

## Root-cause self-check

- **Silent path**: every invalid value is a line-numbered build error. `AspectRatio` is an enum, so no external crate can construct anything but a legal variant → OK
- **Long-term view**: to add `16:10` in the future, update the enum variant, serde label, `width()`/`height()`/`css_aspect_value()`/`FromStr`. The parser delegates to `FromStr`, so the parser-side label list does not grow → OK
- **Type safety**: `AspectRatio` is an enum, so there is no path where raw `(u32, u32)` tuples are mistakenly compared. `Manifest` internals also do not store canvas width/height; they derive from `AspectRatio` → OK
- **Single source of truth**: Rust-side `AspectRatio` is the truth; it propagates to TS/CSS through the manifest. Escapes the "rewrite 1280 in three places" pattern → OK

## Verification gates

CLAUDE.md required gates:
- `cargo test --workspace` (3 consecutive runs)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/`
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js`

E2E:
- `peitho build examples/aspect-ratio-4-3/`, open `dist/index.html` in a real browser, and verify the 4:3 slides render correctly (mandatory given past "full black screen" incidents)

## Shipped divergence / adjustments during review (2026-07-05)

1. `AspectRatio` shipped as an enum, not a newtype. Only `Ratio16To9` / `Ratio4To3` are legal values, and serde rename (`"16:9"` / `"4:3"`) and `FromStr` are centralized in `domain.rs`. The frontmatter parser holds no label list and delegates to `AspectRatio` parsing.
2. The Manifest wire form is not a `{width,height}` object but `"aspectRatio": "16:9"` plus derived numeric fields (`"canvasWidth": 1280`, `"canvasHeight": 720`). Rust-internal `Manifest` holds only `AspectRatio`; a private `ManifestWire` handles the JSON shape for serialization/deserialization.
3. Manifest deserialize is accept-and-drop. Even if `canvasWidth` / `canvasHeight` appear in the JSON, `aspectRatio` is authoritative. A contradictory JSON (`"aspectRatio":"4:3","canvasWidth":1280`) deserializes to `canvas_width() == 960`.
4. Removed the `CANVAS_WIDTH` / `CANVAS_HEIGHT` exports and the `?? CANVAS_WIDTH` / `?? CANVAS_HEIGHT` fallbacks from `packages/peitho-present/src/canvas.ts`. `installCanvasScaler` takes `canvasWidth` / `canvasHeight` as required options — no convention-only seam that silently falls back to 16:9 when a caller forgets to pass the dimensions.
5. The presenter view is also aspect-ratio aware. `render.rs` embeds `--peitho-canvas-aspect` on `:root` in the standalone / present / presenter templates, and the presenter's `.stage` / `.slide-pane` / `.next-preview` use `var(--peitho-canvas-aspect)`. `shell.ts` also sets the same CSS variable on the shell root.
