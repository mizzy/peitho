# Lint font-size check implementation plan

Date: 2026-08-01
Status: planned

<!-- derived-from ../specs/2026-08-01-lint-font-size-design.md -->

Source of truth: [Lint font-size check -- design](../specs/2026-08-01-lint-font-size-design.md).
The check rides the existing single Chrome lint measurement pass. Any warning
keeps exit code `1`; a clean run exits `0`.

## Task 1 -- Rename the measurement type

Goal: Rename `SlideOverflow` to `SlideMeasurement` before adding non-overflow
data.

Files: `crates/peitho/src/lint.rs`.

Test:

```rust
let measurements = parse_lint_measurements(&stderr, 1).unwrap();
assert_eq!(measurements[0].slide, 1);
let warnings: Vec<OverflowWarning> = collect_overflow_warnings(&measurements);
assert_eq!(warnings.len(), 1);
```

Implementation:

```rust
struct SlideMeasurement {
    slide: usize,
    #[serde(rename = "contentWidth")]
    content_width: f64,
    #[serde(rename = "contentHeight")]
    content_height: f64,
    #[serde(rename = "boxWidth")]
    box_width: f64,
    #[serde(rename = "boxHeight")]
    box_height: f64,
}

fn parse_lint_measurements(
    chrome_log: &str,
    expected_slide_count: usize,
) -> miette::Result<Vec<SlideMeasurement>>;

fn collect_overflow_warnings(measurements: &[SlideMeasurement]) -> Vec<OverflowWarning>;

fn write_lint_report(
    measurements: &[SlideMeasurement],
    stdout: &mut dyn Write,
) -> miette::Result<i32>;
```

Verification:

```bash
cargo test -p peitho lint
```

## Task 2 -- Deserialize font payload fields

Goal: Add optional `minFontSizePx` and `minFontSample` fields using explicit
camelCase serde names.

Files: `crates/peitho/src/lint.rs`.

Test:

```rust
let payload = encoded(
    r#"[{"slide":1,"contentWidth":1280.4,"contentHeight":762.49,"boxWidth":1280.0,"boxHeight":720.0,"minFontSizePx":18.0,"minFontSample":"Tiny text"}]"#,
);
let measurements = parse_lint_measurements(&console_chunk(1, 1, &payload), 1).unwrap();
assert_eq!(measurements[0].min_font_size_px, Some(18.0));
assert_eq!(measurements[0].min_font_sample.as_deref(), Some("Tiny text"));
```

Implementation:

```rust
struct SlideMeasurement {
    slide: usize,
    #[serde(rename = "contentWidth")]
    content_width: f64,
    #[serde(rename = "contentHeight")]
    content_height: f64,
    #[serde(rename = "boxWidth")]
    box_width: f64,
    #[serde(rename = "boxHeight")]
    box_height: f64,
    #[serde(rename = "minFontSizePx")]
    min_font_size_px: Option<f64>,
    #[serde(rename = "minFontSample")]
    min_font_sample: Option<String>,
}
```

Fixture note:

- `lint_measurement_chunks_reassemble_base64_json_and_validate_slide_count`
  gains `"minFontSizePx":18.0,"minFontSample":"Tiny text"` and asserts `Some`.
- The slide-count mismatch JSON inside
  `lint_measurement_chunk_errors_are_distinct_and_actionable` gains
  `"minFontSizePx":null,"minFontSample":null`.
- Direct `SlideMeasurement` literals in overflow and report tests gain
  `min_font_size_px: None` and `min_font_sample: None`.

Verification:

```bash
cargo test -p peitho lint
```

## Task 3 -- Measure min visible text font size

Goal: Extend `lint_measure.js` and the marker/unit tests in the same cycle,
including UTF-8-safe payload encoding and code-point-safe sample truncation.

Files: `crates/peitho-core/src/lint_measure.js`,
`crates/peitho-core/src/render.rs`, `crates/peitho/src/lint.rs` for the
UTF-8 fixture only.

Test:

```rust
#[test]
fn lint_measure_script_measures_utf8_safe_min_visible_text_font_size() {
    assert!(LINT_MEASURE_JS.contains("NodeFilter.SHOW_TEXT"));
    assert!(LINT_MEASURE_JS.contains("createTreeWalker"));
    assert!(LINT_MEASURE_JS.contains(r#".peitho-footnotes, sup.peitho-footnote-ref"#));
    assert!(LINT_MEASURE_JS.contains("getComputedStyle"));
    assert!(LINT_MEASURE_JS.contains(r#"visibility === "hidden""#));
    assert!(LINT_MEASURE_JS.contains(r#"visibility === "collapse""#));
    assert!(LINT_MEASURE_JS.contains("TextEncoder"));
    assert!(LINT_MEASURE_JS.contains("base64EncodeUtf8"));
    assert!(!LINT_MEASURE_JS.contains("btoa(JSON.stringify(results))"));
    assert!(LINT_MEASURE_JS.contains("minFontSizePx"));
    assert!(LINT_MEASURE_JS.contains("minFontSample"));
    assert!(LINT_MEASURE_JS.contains(r#"Array.from(sample).slice(0, 40).join("") + "…""#));
}

#[test]
fn lint_measurement_payload_accepts_utf8_min_font_sample() {
    let payload = encoded(
        r#"[{"slide":1,"contentWidth":1280.0,"contentHeight":720.0,"boxWidth":1280.0,"boxHeight":720.0,"minFontSizePx":24.0,"minFontSample":"日本語の小さい文字🙂"}]"#,
    );
    let measurements = parse_lint_measurements(&console_chunk(1, 1, &payload), 1).unwrap();
    assert_eq!(
        measurements[0].min_font_sample.as_deref(),
        Some("日本語の小さい文字🙂")
    );
}
```

Also remove the old negative `getComputedStyle` assertion from
`lint_measure_script_uses_descendant_rect_union_with_scroll_floor`.

Implementation:

```javascript
function truncateSample(sample) {
  if (Array.from(sample).length > 40) {
    return Array.from(sample).slice(0, 40).join("") + "…";
  }
  return sample;
}

function fontSample(text) {
  var sample = text.replace(/\s+/g, " ").trim();
  return truncateSample(sample);
}

function base64EncodeUtf8(text) {
  var bytes = new TextEncoder().encode(text);
  var binary = "";
  for (var index = 0; index < bytes.length; index += 1) {
    binary += String.fromCharCode(bytes[index]);
  }
  return btoa(binary);
}

function measureTextFont(slide) {
  var walker = document.createTreeWalker(slide, NodeFilter.SHOW_TEXT);
  var minFontSizePx = null;
  var minFontSample = null;
  var node;
  while ((node = walker.nextNode())) {
    var sample = fontSample(node.textContent || "");
    var parent = node.parentElement;
    if (sample === "" || !parent) continue;
    if (parent.closest(".peitho-footnotes, sup.peitho-footnote-ref")) continue;
    var rect = parent.getBoundingClientRect();
    var style = getComputedStyle(parent);
    var visibility = style.visibility;
    if ((rect.width === 0 && rect.height === 0) || visibility === "hidden" || visibility === "collapse") continue;
    var size = parseFloat(style.fontSize);
    if (!isFinite(size)) continue;
    if (minFontSizePx === null || size < minFontSizePx) {
      minFontSizePx = size;
      minFontSample = sample;
    }
  }
  return { minFontSizePx: minFontSizePx, minFontSample: minFontSample };
}

var textFont = measureTextFont(slide);
return {
  slide: index + 1,
  contentWidth: Math.max(bounds.maxRight - bounds.minLeft, slide.scrollWidth),
  contentHeight: Math.max(bounds.maxBottom - bounds.minTop, slide.scrollHeight),
  boxWidth: slideRect.width,
  boxHeight: slideRect.height,
  minFontSizePx: textFont.minFontSizePx,
  minFontSample: textFont.minFontSample
};

function publish(results) {
  var payload = base64EncodeUtf8(JSON.stringify(results));
}
```

Verification:

```bash
cargo test -p peitho-core lint_measure_script
cargo test -p peitho utf8_min_font_sample
```

## Task 4 -- Collect font warnings

Goal: Warn once per slide when `minFontSizePx` rounded to `0.01px` is below
`32.0px`.

Files: `crates/peitho/src/lint.rs`.

Test:

```rust
fn measurement(slide: usize, px: Option<f64>, sample: Option<&str>) -> SlideMeasurement {
    SlideMeasurement {
        slide,
        content_width: 800.0,
        content_height: 600.0,
        box_width: 800.0,
        box_height: 600.0,
        min_font_size_px: px,
        min_font_sample: sample.map(str::to_owned),
    }
}

let warnings = collect_font_size_warnings(&[
    measurement(1, Some(31.999), Some("rounds clean")),
    measurement(2, Some(31.994), Some("Tiny caption")),
    measurement(3, None, None),
]);

assert_eq!(warnings.len(), 1);
assert_eq!(warnings[0].slide, 2);
assert!((warnings[0].font_size_px - 31.99).abs() < f64::EPSILON);
assert_eq!(warnings[0].sample, "Tiny caption");
```

Implementation:

```rust
const MIN_FONT_SIZE_PX: f64 = 32.0;

#[derive(Debug, Clone, PartialEq)]
struct FontSizeWarning {
    slide: usize,
    font_size_px: f64,
    sample: String,
}

fn round_font_px(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn collect_font_size_warnings(measurements: &[SlideMeasurement]) -> Vec<FontSizeWarning> {
    measurements
        .iter()
        .filter_map(|measurement| {
            let font_size_px = round_font_px(measurement.min_font_size_px?);
            (font_size_px < MIN_FONT_SIZE_PX).then(|| FontSizeWarning {
                slide: measurement.slide,
                font_size_px,
                sample: measurement.min_font_sample.clone().unwrap_or_default(),
            })
        })
        .collect()
}
```

Verification:

```bash
cargo test -p peitho lint
```

## Task 5 -- Report font warnings and unified summaries

Goal: Print font warnings in pt, keep overflow warnings, and replace
overflow-only summary wording with warning-count wording.

Files: `crates/peitho/src/lint.rs`.

Test:

```rust
let measurements = vec![SlideMeasurement {
    slide: 3,
    content_width: 900.0,
    content_height: 642.4,
    box_width: 900.0,
    box_height: 600.2,
    min_font_size_px: Some(24.0),
    min_font_sample: Some("excerpt…".to_owned()),
}];

let exit_code = write_lint_report(&measurements, &mut stdout).unwrap();
let output = String::from_utf8(stdout).unwrap();
assert_eq!(exit_code, 1);
assert!(output.contains("warning: slide 3 content overflows the slide box vertically by 42px"));
assert!(output.contains(
    "warning: slide 3 has text at 18pt, below the recommended 24pt: \"excerpt…\""
));
assert!(output.contains(
    "  help: raise the font size in the layout CSS, or move content to another slide instead of shrinking it"
));
assert!(output.contains("checked 1 slide(s): 2 warning(s)"));
assert_eq!(format_font_size_pt(23.1), "17.3pt");

let clean = SlideMeasurement {
    slide: 1,
    content_width: 800.0,
    content_height: 600.0,
    box_width: 800.0,
    box_height: 600.0,
    min_font_size_px: None,
    min_font_sample: None,
};
assert_eq!(write_lint_report(&[clean], &mut clean_stdout).unwrap(), 0);
assert_eq!(String::from_utf8(clean_stdout).unwrap(), "checked 1 slide(s): no warnings\n");
```

Implementation:

```rust
const FONT_SIZE_HELP: &str =
    "raise the font size in the layout CSS, or move content to another slide instead of shrinking it";

fn format_font_size_pt(px: f64) -> String {
    let pt = (px * 0.75 * 10.0).round() / 10.0;
    if pt.fract() == 0.0 { format!("{pt:.0}pt") } else { format!("{pt:.1}pt") }
}

let overflow_warnings = collect_overflow_warnings(measurements);
let font_size_warnings = collect_font_size_warnings(measurements);
for warning in &font_size_warnings {
    writeln!(
        stdout,
        "warning: slide {} has text at {}, below the recommended 24pt: \"{}\"",
        warning.slide,
        format_font_size_pt(warning.font_size_px),
        warning.sample
    )
    .into_diagnostic()?;
    writeln!(stdout, "  help: {FONT_SIZE_HELP}").into_diagnostic()?;
}

let warning_count = overflow_warnings.len() + font_size_warnings.len();
if warning_count == 0 {
    writeln!(stdout, "checked {} slide(s): no warnings", measurements.len()).into_diagnostic()?;
    Ok(0)
} else {
    writeln!(stdout, "checked {} slide(s): {} warning(s)", measurements.len(), warning_count)
        .into_diagnostic()?;
    Ok(1)
}
```

Verification:

```bash
cargo test -p peitho lint
```

## Task 6 -- Run real Chrome E2E for Japanese samples

Goal: Exercise the Rust-to-Chrome measurement seam with a real `cargo run`
lint invocation before the full gate, preserving the CLAUDE.md / Issue #307
real-browser seam rule.

Files: none; the task creates temporary deck files under `mktemp -d`.

Test:

```bash
set -euo pipefail
tmp="$(mktemp -d)"
mkdir "$tmp/css"

cat > "$tmp/css/tiny.css" <<'CSS'
.slot-body p {
  font-size: 24px;
}
CSS

cat > "$tmp/tiny-ja.md" <<'MD'
---
css: ./css
---
# 小さい文字

日本語の小さい文字テキスト🙂をここに置きます
MD

cat > "$tmp/clean.md" <<'MD'
# Clean
MD

set +e
cargo run -p peitho -- lint "$tmp/tiny-ja.md" > "$tmp/tiny.out" 2> "$tmp/tiny.err"
tiny_status="$?"
set -e
test "$tiny_status" -eq 1
rg -F 'warning: slide 1 has text at 18pt, below the recommended 24pt: "日本語の小さい文字テキスト🙂をここに置きます"' "$tmp/tiny.out"
rg -F 'checked 1 slide(s): 1 warning(s)' "$tmp/tiny.out"

cargo run -p peitho -- lint "$tmp/clean.md" > "$tmp/clean.out" 2> "$tmp/clean.err"
rg -F 'checked 1 slide(s): no warnings' "$tmp/clean.out"
```

Implementation:

```markdown
---
css: ./css
---
# 小さい文字

日本語の小さい文字テキスト🙂をここに置きます
```

```css
.slot-body p {
  font-size: 24px;
}
```

Verification:

```bash
cargo run -p peitho -- lint "$tmp/tiny-ja.md"
cargo run -p peitho -- lint "$tmp/clean.md"
```

## Task 7 -- Full gate

Goal: Validate the whole workspace after the focused TDD cycles pass.

Files: none.

Test:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Implementation:

Run the same commands from the repository root. If formatting is the only
failure in `crates/peitho-core/src/render.rs` or `crates/peitho/src/lint.rs`,
run:

```bash
cargo fmt --all
```

Verification:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```
