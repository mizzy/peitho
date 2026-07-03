# Presentation Time Tracking Implementation Plan

<!-- constrained-by ../specs/2026-07-03-time-tracking-design.md -->
<!-- constrained-by ../../CLAUDE.md -->

## Premises

- Do not change D1–D7 of the design document `docs/specs/2026-07-03-time-tracking-design.md`
- `time` is specified only in the YAML frontmatter at the top of the deck
- Unknown keys in frontmatter, invalid `time`, and metadata blocks other than at the top are all build errors with line numbers and help
- Deck settings are fixed at `Deck<Parsed>` and carried through to `Mapped`, `Checked`, and `Rendered`
- The tracker emits `peitho:timercontrol` and reads `PresentShell.elapsedMs()` and `peitho:slidechange`
- The Rust types for `manifest.json` and `present.json` are the single source, and `bindings/*.ts` generated artifacts are committed
- `present.json` and the tracker are present-cache only, and must not be mixed into the publish `dist/`

## Task 1: Add `serde_norway` as a workspace dependency

Goal: Pin the crate that reads YAML frontmatter with serde as a workspace dependency, so it is usable from `peitho-core`.

Files:
- `Cargo.toml`
- `crates/peitho-core/Cargo.toml`
- `Cargo.lock`
- `crates/peitho-core/src/parser.rs`

Test:

```rust
#[test]
fn deck_frontmatter_yaml_dependency_is_available() {
    #[derive(Debug, serde::Deserialize)]
    struct Probe {
        time: String,
    }

    let parsed: Probe = serde_norway::from_str("time: 15m\n").unwrap();

    assert_eq!(parsed.time, "15m");
}
```

Implementation:

```toml
[workspace.dependencies]
serde_norway = "0.9"
```

```toml
[dependencies]
serde_norway.workspace = true
```

Verification:

```bash
cargo test -p peitho-core deck_frontmatter_yaml_dependency_is_available
```

## Task 2: Capture the top-of-file frontmatter with pulldown-cmark's metadata block

Goal: Enable `ENABLE_YAML_STYLE_METADATA_BLOCKS` and exclude the YAML block at the top of the document from the slide range. `---` from the second occurrence onward continues to act as a slide separator as before.

Files:
- `crates/peitho-core/src/parser.rs`

Test:

```rust
#[test]
fn frontmatter_is_not_a_slide_and_later_rules_still_split_slides() {
    let deck = parse_markdown("---\ntime: 15m\n---\n\n# Intro\n\n---\n# Details").unwrap();

    assert_eq!(deck.parsed_slides().len(), 2);
    assert_eq!(deck.parsed_slides()[0].fragments[0].line(), 5);
    assert_eq!(deck.parsed_slides()[1].fragments[0].line(), 8);
}
```

Implementation:

```rust
use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, MetadataBlockKind, Options, Parser, Tag, TagEnd,
};

struct RawFrontmatter {
    line: usize,
    yaml: String,
}

struct SplitSlides {
    frontmatter: Option<RawFrontmatter>,
    ranges: Vec<SlideRange>,
}

fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}
```

```rust
let mut frontmatter_line: Option<usize> = None;
let mut frontmatter_yaml = String::new();

Event::Start(Tag::MetadataBlock(MetadataBlockKind::YamlStyle)) if ranges.is_empty() && start == 0 => {
    frontmatter_line = Some(line_for_offset(source, range.start));
}
Event::Text(text) if frontmatter_line.is_some() => {
    frontmatter_yaml.push_str(&text);
}
Event::SoftBreak | Event::HardBreak if frontmatter_line.is_some() => {
    frontmatter_yaml.push('\n');
}
Event::End(TagEnd::MetadataBlock(MetadataBlockKind::YamlStyle)) if frontmatter_line.is_some() => {
    let line = frontmatter_line.take().expect("frontmatter start exists");
    frontmatter = Some(RawFrontmatter {
        line,
        yaml: frontmatter_yaml.trim().to_owned(),
    });
    frontmatter_yaml.clear();
    start = range.end;
}
Event::Start(Tag::MetadataBlock(kind)) | Event::End(TagEnd::MetadataBlock(kind)) => {
    return Err(metadata_block_position_error(source, range.start, kind));
}
```

```rust
fn metadata_block_position_error(
    source: &str,
    offset: usize,
    _kind: MetadataBlockKind,
) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line_for_offset(source, offset)),
        "YAML frontmatter is only allowed at the top of the deck",
        "move deck settings before the first slide or replace this block with slide content",
    )
}
```

Verification:

```bash
cargo test -p peitho-core frontmatter_is_not_a_slide_and_later_rules_still_split_slides
```

## Task 3: Implement `DeckFrontmatter` and the accepted `PlannedTime` formats

Goal: Convert `time: 15m`, `90s`, `1h`, `1h30m`, and a bare integer `15` into milliseconds. Concentrate validation at the point where `PlannedTime` is constructed.

Files:
- `crates/peitho-core/src/parser.rs`

Test:

```rust
#[test]
fn parses_planned_time_values_from_frontmatter() {
    let cases = [
        ("15m", 900_000),
        ("90s", 90_000),
        ("1h", 3_600_000),
        ("1h30m", 5_400_000),
        ("15", 900_000),
    ];

    for (value, expected) in cases {
        let yaml = format!("time: {value}\n");
        let parsed: DeckFrontmatter = serde_norway::from_str(&yaml).unwrap();

        assert_eq!(parsed.time.map(PlannedTime::as_millis), Some(expected));
    }
}
```

Implementation:

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFrontmatter {
    time: Option<PlannedTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlannedTime(u64);

impl PlannedTime {
    fn as_millis(self) -> u64 {
        self.0
    }
}

struct ParsedDeckSettings {
    planned_duration_ms: Option<u64>,
}
```

```rust
fn parse_planned_time_text(input: &str) -> std::result::Result<u64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("time must not be empty".to_owned());
    }
    if trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        let minutes = trimmed.parse::<u64>().map_err(|err| err.to_string())?;
        return minutes
            .checked_mul(60_000)
            .filter(|millis| *millis > 0)
            .ok_or_else(|| "time must be greater than zero".to_owned());
    }

    let mut rest = trimmed;
    let mut total_seconds = 0_u64;
    while !rest.is_empty() {
        let digit_bytes = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digit_bytes == 0 {
            return Err("time must use h, m, or s units".to_owned());
        }
        let value = rest[..digit_bytes]
            .parse::<u64>()
            .map_err(|err| err.to_string())?;
        rest = &rest[digit_bytes..];
        let unit = rest
            .bytes()
            .next()
            .ok_or_else(|| "time string is missing a unit".to_owned())?;
        total_seconds = match unit {
            b'h' => total_seconds.saturating_add(value.saturating_mul(3600)),
            b'm' => total_seconds.saturating_add(value.saturating_mul(60)),
            b's' => total_seconds.saturating_add(value),
            _ => return Err("time must use h, m, or s units".to_owned()),
        };
        rest = &rest[1..];
    }

    total_seconds
        .checked_mul(1000)
        .filter(|millis| *millis > 0)
        .ok_or_else(|| "time must be greater than zero".to_owned())
}
```

```rust
impl<'de> Deserialize<'de> for PlannedTime {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PlannedTimeVisitor;

        impl<'de> serde::de::Visitor<'de> for PlannedTimeVisitor {
            type Value = PlannedTime;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a duration like 15m, 90s, 1h30m, or an integer minute count")
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                value
                    .checked_mul(60_000)
                    .filter(|millis| *millis > 0)
                    .map(PlannedTime)
                    .ok_or_else(|| E::custom("time must be greater than zero"))
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value <= 0 {
                    return Err(E::custom("time must be greater than zero"));
                }
                self.visit_u64(value as u64)
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                parse_planned_time_text(value).map(PlannedTime).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(PlannedTimeVisitor)
    }
}
```

Verification:

```bash
cargo test -p peitho-core parses_planned_time_values_from_frontmatter
```

## Task 4: Pin down frontmatter error paths with no silent drops

Goal: Fail with `ErrorKind::Parse`, a line number, and help text for unknown keys, invalid `time`, malformed YAML, and metadata blocks placed anywhere other than the top.

Files:
- `crates/peitho-core/src/parser.rs`

Test:

```rust
#[test]
fn rejects_unknown_frontmatter_key_with_line_and_help() {
    let err = parse_markdown("---\ntime: 15m\nunknown: true\n---\n# Intro").unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(3));
    assert!(err.to_string().contains("invalid deck frontmatter"));
    assert_eq!(
        err.help,
        "use only the supported deck frontmatter key: time"
    );
}

#[test]
fn rejects_invalid_frontmatter_time_with_line_and_help() {
    for value in ["0", "-1", "abc", ""] {
        let markdown = format!("---\ntime: {value}\n---\n# Intro");
        let err = parse_markdown(&markdown).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert_eq!(
            err.help,
            "set time to 15m, 90s, 1h, 1h30m, or an integer minute count"
        );
    }
}

#[test]
fn rejects_broken_frontmatter_yaml_with_line_and_help() {
    let err = parse_markdown("---\ntime: [\n---\n# Intro").unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(2));
    assert!(err.to_string().contains("invalid deck frontmatter"));
    assert_eq!(
        err.help,
        "write valid YAML frontmatter before the first slide"
    );
}

#[test]
fn rejects_metadata_block_inside_a_slide() {
    let source = "---\ntime: 15m\n---\n# Details";
    let err = parse_slide(
        source,
        SlideRange {
            start: 0,
            end: source.len(),
        },
        1,
    )
    .unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(1));
    assert!(err.to_string().contains("YAML frontmatter is only allowed at the top of the deck"));
}
```

Implementation:

```rust
fn parse_deck_frontmatter(raw: Option<&RawFrontmatter>) -> Result<ParsedDeckSettings> {
    let Some(raw) = raw else {
        return Ok(ParsedDeckSettings {
            planned_duration_ms: None,
        });
    };
    if raw.yaml.trim().is_empty() {
        return Ok(ParsedDeckSettings {
            planned_duration_ms: None,
        });
    }

    let parsed: DeckFrontmatter = serde_norway::from_str(&raw.yaml).map_err(|err| {
        let yaml_line = err.location().map(|location| location.line()).unwrap_or(1);
        let message = format!("invalid deck frontmatter: {err}");
        let help = if message.contains("unknown field") {
            "use only the supported deck frontmatter key: time"
        } else if message.contains("time")
            || message.contains("duration")
            || message.contains("greater than zero")
            || message.contains("unit")
        {
            "set time to 15m, 90s, 1h, 1h30m, or an integer minute count"
        } else {
            "write valid YAML frontmatter before the first slide"
        };
        BuildError::new(ErrorKind::Parse, Some(raw.line + yaml_line), message, help)
    })?;

    Ok(ParsedDeckSettings {
        planned_duration_ms: parsed.time.map(PlannedTime::as_millis),
    })
}
```

```rust
Event::Start(Tag::MetadataBlock(_)) => {
    let err = BuildError::new(
        ErrorKind::Parse,
        Some(line),
        "YAML frontmatter is only allowed at the top of the deck",
        "move deck settings before the first slide or replace this block with slide content",
    );
    return Err(attach_slide_context(
        err,
        index,
        explicit_key.as_ref(),
        &fragments,
    ));
}
```

Verification:

```bash
cargo test -p peitho-core rejects_unknown_frontmatter_key_with_line_and_help
cargo test -p peitho-core rejects_invalid_frontmatter_time_with_line_and_help
cargo test -p peitho-core rejects_broken_frontmatter_yaml_with_line_and_help
cargo test -p peitho-core rejects_metadata_block_inside_a_slide
```

## Task 5: Have the typestate `Deck` carry deck settings

Goal: Carry the `planned_duration_ms` fixed at `Deck<Parsed>` through to `Mapped`, `Checked`, and `Rendered` with no failure path.

Files:
- `crates/peitho-core/src/phase.rs`
- `crates/peitho-core/src/parser.rs`
- `crates/peitho-core/src/mapping.rs`
- `crates/peitho-core/src/check.rs`
- `crates/peitho-core/src/render.rs`

Test:

```rust
#[test]
fn deck_settings_survive_all_typestate_transitions() {
    let layout = parse_layout(
        "title-only",
        r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
    )
    .unwrap();
    let parsed = parse_markdown("---\ntime: 15m\n---\n# Intro").unwrap();
    let mapped = map_by_convention(parsed, &layout).unwrap();
    let checked = check_deck(mapped).unwrap();
    let rendered = render_deck(checked).unwrap();

    assert_eq!(rendered.settings().planned_duration_ms(), Some(900_000));
}
```

Implementation:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeckSettings {
    planned_duration_ms: Option<u64>,
}

impl DeckSettings {
    pub fn new(planned_duration_ms: Option<u64>) -> Self {
        Self {
            planned_duration_ms,
        }
    }

    pub fn planned_duration_ms(&self) -> Option<u64> {
        self.planned_duration_ms
    }
}

#[derive(Debug, Clone)]
pub struct Deck<P> {
    settings: DeckSettings,
    phase: P,
}
```

```rust
impl<P> Deck<P> {
    pub fn settings(&self) -> &DeckSettings {
        &self.settings
    }
}

impl Deck<Parsed> {
    pub(crate) fn parsed(settings: DeckSettings, slides: Vec<ParsedSlide>) -> Self {
        Self {
            settings,
            phase: Parsed { slides },
        }
    }

    pub(crate) fn into_parsed_parts(self) -> (DeckSettings, Vec<ParsedSlide>) {
        (self.settings, self.phase.slides)
    }
}

impl Deck<Mapped> {
    pub(crate) fn mapped(settings: DeckSettings, slides: Vec<MappedSlide>) -> Self {
        Self {
            settings,
            phase: Mapped { slides },
        }
    }

    pub(crate) fn into_mapped_parts(self) -> (DeckSettings, Vec<MappedSlide>) {
        (self.settings, self.phase.slides)
    }
}

impl Deck<Checked> {
    pub(crate) fn checked(settings: DeckSettings, slides: Vec<CheckedSlide>) -> Self {
        Self {
            settings,
            phase: Checked { slides },
        }
    }

    pub(crate) fn into_checked_parts(self) -> (DeckSettings, Vec<CheckedSlide>) {
        (self.settings, self.phase.slides)
    }
}

impl Deck<Rendered> {
    pub(crate) fn rendered(
        settings: DeckSettings,
        slides: Vec<RenderedSlide>,
        css: String,
    ) -> Self {
        Self {
            settings,
            phase: Rendered { slides, css },
        }
    }
}
```

```rust
let split = split_slide_ranges(source)?;
let settings = parse_deck_frontmatter(split.frontmatter.as_ref())?;
Ok(Deck::parsed(
    DeckSettings::new(settings.planned_duration_ms),
    slides,
))
```

```rust
pub fn dispatch_by_convention(deck: Deck<Parsed>, layouts: &Layouts) -> Result<Deck<Mapped>> {
    let (settings, parsed_slides) = deck.into_parsed_parts();
    let mut slides = Vec::new();
    for slide in parsed_slides {
        let slide_number = slide.index + 1;
        let slide_key = slide.key.as_str().to_owned();
        let mapped = dispatch_slide(slide, layouts)
            .map_err(|err| err.with_slide(slide_number, Some(&slide_key)))?;
        slides.push(mapped);
    }
    Ok(Deck::mapped(settings, slides))
}

pub fn map_by_convention(deck: Deck<Parsed>, layout: &Layout) -> Result<Deck<Mapped>> {
    dispatch_by_convention(deck, &Layouts::single(layout.clone()))
}
```

```rust
pub fn check_deck(deck: Deck<Mapped>) -> Result<Deck<Checked>> {
    let (settings, mapped_slides) = deck.into_mapped_parts();
    let mut slides = Vec::new();
    for slide in mapped_slides {
        let slide_number = slide.index + 1;
        let slide_key = slide.key.as_str().to_owned();
        check_slide(&slide).map_err(|err| err.with_slide(slide_number, Some(&slide_key)))?;
        let checked_slots = slide
            .slots
            .into_iter()
            .map(|(slot, mapped_slot)| (slot, mapped_slot.fragments().to_vec()))
            .collect();
        slides.push(CheckedSlide::new(
            slide.index,
            slide.key,
            slide.layout,
            checked_slots,
        ));
    }
    Ok(Deck::checked(settings, slides))
}
```

```rust
pub fn render_deck(deck: Deck<Checked>) -> Result<Deck<Rendered>> {
    let (settings, checked_slides) = deck.into_checked_parts();
    let mut slides = Vec::new();
    for slide in checked_slides {
        let html = render_slide(slide.key(), slide.slots(), slide.layout())?;
        slides.push(RenderedSlide::new(slide.index(), slide.key().clone(), html));
    }
    Ok(Deck::rendered(settings, slides, String::new()))
}
```

Verification:

```bash
cargo test -p peitho-core deck_settings_survive_all_typestate_transitions
```

## Task 6: Add `Manifest.planned_duration_ms`

Goal: Carry the deck's planned duration into `manifest.json`'s `plannedDurationMs`. When there is no `time`, it becomes `null`.

Files:
- `crates/peitho-core/src/manifest.rs`
- `bindings/Manifest.ts`

Test:

```rust
#[test]
fn manifest_serializes_planned_duration_ms() {
    let manifest = Manifest::new(
        "Deck",
        Some(900_000),
        vec![ManifestSlide::new(
            0,
            SlideKey::new("intro").unwrap(),
            "slides/000-intro.html",
            false,
        )],
    );

    let json = manifest_json(&manifest).unwrap();

    assert!(json.contains(r#""plannedDurationMs": 900000"#));
}

#[test]
fn build_manifest_reads_planned_duration_from_checked_deck() {
    let checked = checked_deck("---\ntime: 15m\n---\n# Intro", title_body_layout());
    let manifest = build_manifest(&checked);

    assert_eq!(manifest.planned_duration_ms(), Some(900_000));
}
```

Implementation:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    version: u8,
    #[serde(rename = "peithoVersion")]
    peitho_version: String,
    title: String,
    #[serde(rename = "slideCount")]
    slide_count: usize,
    #[serde(rename = "plannedDurationMs")]
    planned_duration_ms: Option<u64>,
    slides: Vec<ManifestSlide>,
}

impl Manifest {
    pub fn new(
        title: impl Into<String>,
        planned_duration_ms: Option<u64>,
        slides: Vec<ManifestSlide>,
    ) -> Self {
        let slide_count = slides.len();
        Self {
            version: 1,
            peitho_version: env!("CARGO_PKG_VERSION").to_owned(),
            title: title.into(),
            slide_count,
            planned_duration_ms,
            slides,
        }
    }

    pub fn planned_duration_ms(&self) -> Option<u64> {
        self.planned_duration_ms
    }
}
```

```rust
Manifest::new(title, deck.settings().planned_duration_ms(), slides)
```

Verification:

```bash
cargo test -p peitho-core manifest_serializes_planned_duration_ms
cargo test -p peitho-core build_manifest_reads_planned_duration_from_checked_deck
```

## Task 7: Add `PresentConfig` and `present_config_json`

Goal: Create a Rust type that carries the startup-time display-target decision into `present.json`, and pin down the camelCase JSON and the ts-rs export.

Files:
- `crates/peitho-core/src/present_config.rs`
- `crates/peitho-core/src/lib.rs`
- `bindings/PresentConfig.ts`

Test:

```rust
#[test]
fn serializes_present_config_schema_exactly() {
    let json = present_config_json(&PresentConfig::new(true)).unwrap();

    assert_eq!(
        json,
        "{\n  \"version\": 1,\n  \"presenterOpen\": true\n}\n"
    );
}

#[test]
fn exports_present_config_binding_with_serde_field_names() {
    let cfg = ts_rs::Config::from_env();
    PresentConfig::export_all(&cfg).unwrap();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bindings/PresentConfig.ts");
    let ts = std::fs::read_to_string(path).unwrap();

    assert!(ts.contains("presenterOpen: boolean"));
}
```

Implementation:

```rust
use serde::{Deserialize, Serialize};

use crate::{
    error::{BuildError, ErrorKind, Result},
};

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentConfig {
    version: u8,
    #[serde(rename = "presenterOpen")]
    presenter_open: bool,
}

impl PresentConfig {
    pub fn new(presenter_open: bool) -> Self {
        Self {
            version: 1,
            presenter_open,
        }
    }

    pub fn presenter_open(&self) -> bool {
        self.presenter_open
    }
}

pub fn present_config_json(config: &PresentConfig) -> Result<String> {
    let mut json = serde_json::to_string_pretty(config).map_err(|err| {
        BuildError::new(
            ErrorKind::Manifest,
            None,
            format!("failed to serialize present config: {err}"),
            "keep present config fields serializable",
        )
    })?;
    json.push('\n');
    Ok(json)
}
```

```rust
pub mod present_config;
pub use present_config::{present_config_json, PresentConfig};
```

Verification:

```bash
cargo test -p peitho-core serializes_present_config_schema_exactly
cargo test -p peitho-core exports_present_config_binding_with_serde_field_names
```

## Task 8: Regenerate the ts-rs bindings to update the type contract

Goal: Reflect `plannedDurationMs` in `Manifest.ts` and `presenterOpen` in `PresentConfig.ts`, and make the generated artifacts committed.

Files:
- `bindings/Manifest.ts`
- `bindings/ManifestSlide.ts`
- `bindings/PresentConfig.ts`
- `crates/peitho-core/src/manifest.rs`
- `crates/peitho-core/src/present_config.rs`

Test:

```rust
#[test]
fn exports_manifest_and_present_config_bindings_with_time_contract() {
    let cfg = ts_rs::Config::from_env();
    Manifest::export_all(&cfg).unwrap();
    ManifestSlide::export_all(&cfg).unwrap();
    PresentConfig::export_all(&cfg).unwrap();

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
    let manifest = std::fs::read_to_string(root.join("Manifest.ts")).unwrap();
    let present_config = std::fs::read_to_string(root.join("PresentConfig.ts")).unwrap();

    assert!(manifest.contains("plannedDurationMs: number | null"));
    assert!(present_config.contains("presenterOpen: boolean"));
}
```

Implementation:

```ts
export type PresentConfig = {
  version: number;
  presenterOpen: boolean;
};
```

```ts
export type Manifest = {
  version: number;
  peithoVersion: string;
  title: string;
  slideCount: number;
  plannedDurationMs: number | null;
  slides: Array<ManifestSlide>;
};
```

Verification:

```bash
cargo test -p peitho-core ts_tests
```

## Task 9: Fix `presenter_open` in the CLI and emit `present.json`

Goal: Move the layout detection in `present()` to before `emit_present_cache`, and produce both the browser open call and `present.json` from the same detection result.

Files:
- `crates/peitho/src/main.rs`
- `crates/peitho-core/src/present_config.rs`

Test:

```rust
#[test]
fn presenter_open_uses_startup_layout_result() {
    assert!(presenter_open(false, false, false, Some(fake_layout())));
    assert!(!presenter_open(true, false, false, Some(fake_layout())));
    assert!(!presenter_open(false, true, false, Some(fake_layout())));
    assert!(!presenter_open(false, false, true, Some(fake_layout())));
    assert!(!presenter_open(false, false, false, None));
}

fn fake_layout() -> displays::PresentationLayout {
    displays::PresentationLayout {
        slides: displays::WindowPlacement::Fullscreen { x: 0, y: 0 },
        presenter: displays::WindowPlacement::Windowed {
            x: 24,
            y: 48,
            width: 1200,
            height: 800,
        },
    }
}

#[test]
fn emit_present_cache_writes_present_json() {
    let fixture = WatchFixture::new("# Intro\n");
    let artifacts = build_artifacts(
        &fixture.options.input,
        fixture.options.effective_layouts().as_deref(),
        fixture.options.effective_css().as_deref(),
    )
    .unwrap();

    emit_present_cache(&fixture.options.out, &artifacts, None, true).unwrap();

    let json = fs::read_to_string(fixture.options.out.join("present.json")).unwrap();
    assert!(json.contains(r#""presenterOpen": true"#));
}
```

Implementation:

```rust
fn presenter_mode(options: &PresentOptions) -> displays::PresenterMode {
    if options.presenter_windowed {
        displays::PresenterMode::Windowed {
            saved: browser::chrome_profiles_from_home(std::env::var_os("HOME"))
                .as_ref()
                .and_then(browser::saved_presenter_bounds),
        }
    } else {
        displays::PresenterMode::Fullscreen
    }
}

fn presenter_open(
    no_open: bool,
    no_serve: bool,
    no_presenter: bool,
    layout: Option<displays::PresentationLayout>,
) -> bool {
    !no_open && !no_serve && !no_presenter && layout.is_some()
}
```

```rust
let layout = if options.no_open || options.no_serve {
    None
} else {
    Some(presenter_mode(&options)).and_then(displays::detect_presentation_layout)
};
let presenter_open = presenter_open(
    options.no_open,
    options.no_serve,
    options.no_presenter,
    layout,
);
emit_present_cache(&cache, &artifacts, options.shell.as_deref(), presenter_open)?;
if options.no_serve {
    println!("generated present cache at {}", cache.display());
    return Ok(());
}
```

```rust
if !options.no_open {
    browser::open_browser_with_request(
        browser::BrowserOpenRequest {
            slides_url: &url,
            presenter_url: &presenter_url,
            no_presenter: options.no_presenter,
        },
        layout,
    );
}
```

```rust
fs::write(
    cache.join("present.json"),
    core(peitho_core::present_config_json(&peitho_core::PresentConfig::new(
        presenter_open,
    )))?,
)
.into_diagnostic()?;
```

Verification:

```bash
cargo test -p peitho presenter_open_uses_startup_layout_result
cargo test -p peitho emit_present_cache_writes_present_json
```

## Task 10: Add `timeTracker.ts` as a UI component

Goal: Draw the rabbit's slide-progress ratio and the turtle's time-progress ratio on the same track, and emit `peitho:timercontrol start` exactly once on the first forward advance.

Files:
- `packages/peitho-present/src/timeTracker.ts`
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/timeTracker.test.ts`

Test:

```ts
import { afterEach, expect, it } from "vitest";
import { installTimeTracker } from "../src/index";

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
});

it("moves rabbit by slide progress and turtle by elapsed progress", () => {
  let elapsed = 30_000;
  const root = document.createElement("main");
  const bus = new EventTarget();
  const cleanup = installTimeTracker({
    root,
    shell: {
      manifest: { slideCount: 3 },
      currentIndex: 0,
      elapsedMs: () => elapsed
    },
    plannedDurationMs: 60_000,
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 1, total: 3, previousIndex: 0, key: "middle" }
    })
  );

  expect(root.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')?.style.left).toBe("50%");
  expect(root.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')?.style.left).toBe("50%");
});

it("dispatches timer start once on the first forward slidechange", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const starts: unknown[] = [];
  bus.addEventListener("peitho:timercontrol", (event) => starts.push((event as CustomEvent).detail));
  cleanups.push(
    installTimeTracker({
      root,
      shell: {
        manifest: { slideCount: 2 },
        currentIndex: 0,
        elapsedMs: () => 0
      },
      plannedDurationMs: 60_000,
      bus,
      window,
      document
    })
  );

  bus.dispatchEvent(new CustomEvent("peitho:slidechange", { detail: { index: 1, total: 2, previousIndex: 0, key: "two" } }));
  bus.dispatchEvent(new CustomEvent("peitho:slidechange", { detail: { index: 0, total: 2, previousIndex: 1, key: "one" } }));
  bus.dispatchEvent(new CustomEvent("peitho:slidechange", { detail: { index: 1, total: 2, previousIndex: 0, key: "two" } }));

  expect(starts).toEqual([{ action: "start" }]);
});
```

Implementation:

```ts
import type { SlideChangeDetail, TimerControlDetail } from "./shell";

export type TimeTrackerShell = {
  manifest: { slideCount: number } | null;
  currentIndex: number;
  elapsedMs(): number;
};

export type TimeTrackerOptions = {
  root: HTMLElement;
  shell: TimeTrackerShell;
  plannedDurationMs: number;
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  variant?: "present" | "presenter";
};

export function installTimeTracker(options: TimeTrackerOptions): () => void {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
  const track = doc.createElement("div");
  track.className = "peitho-time-tracker";
  track.dataset.peithoTimeTracker = options.variant ?? "present";
  track.innerHTML = [
    '<span data-peitho-marker="rabbit" aria-label="slide progress">🐰</span>',
    '<span data-peitho-marker="turtle" aria-label="time progress">🐢</span>'
  ].join("");
  options.root.appendChild(track);

  const rabbit = track.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')!;
  const turtle = track.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')!;
  let autoStarted = false;

  const setMarker = (element: HTMLElement, ratio: number): void => {
    element.style.left = `${Math.round(ratio * 10_000) / 100}%`;
  };
  const updateSlides = (index: number, total: number): void => {
    const ratio = total <= 1 ? 0 : index / (total - 1);
    setMarker(rabbit, Math.max(0, Math.min(ratio, 1)));
  };
  const tick = (): void => {
    const ratio = options.shell.elapsedMs() / options.plannedDurationMs;
    setMarker(turtle, Math.min(Math.max(ratio, 0), 1));
    track.toggleAttribute("data-peitho-overrun", ratio > 1);
  };
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<SlideChangeDetail>).detail;
    updateSlides(detail.index, detail.total);
    if (!autoStarted && detail.previousIndex !== null && detail.index > detail.previousIndex) {
      autoStarted = true;
      bus.dispatchEvent(
        new CustomEvent<TimerControlDetail>("peitho:timercontrol", {
          detail: { action: "start" }
        })
      );
    }
  };

  updateSlides(options.shell.currentIndex, options.shell.manifest?.slideCount ?? 0);
  tick();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  const interval = win.setInterval(tick, 250);

  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    track.remove();
  };
}
```

Verification:

```bash
cd packages/peitho-present && npx vitest run test/timeTracker.test.ts
```

## Task 11: Integrate the presenter timer display with the tracker installation

Goal: Show the tracker on the presenter screen when `plannedDurationMs` is present, and extend the existing timer to `MM:SS / MM:SS` plus an overrun display. Keep manual Start/Pause/Resume/Reset on their existing dispatch as-is.

Files:
- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/test/presenter.test.ts`

Test:

```ts
function standardFetchWithPlannedDuration(plannedDurationMs: number | null): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") {
      return okJson(Object.assign({}, manifest, { plannedDurationMs }));
    }
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-details.html")
      return okText("<section><h1>Details</h1></section>");
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
}

it("shows planned duration in presenter timer when manifest has time", async () => {
  let now = 1_000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetchWithPlannedDuration(60_000),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="start"]')?.click();
  now = 31_000;
  view.tick();

  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("00:30 / 01:00");
  expect(root.querySelector('[data-peitho-time-tracker="presenter"]')).not.toBeNull();

  view.destroy();
  views.pop();
  expect(root.querySelector('[data-peitho-time-tracker]')).toBeNull();
});

it("keeps legacy presenter timer text when manifest has no time", async () => {
  let now = 1_000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetchWithPlannedDuration(null),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="start"]')?.click();
  now = 65_000;
  view.tick();

  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("01:04");
  expect(root.querySelector('[data-peitho-time-tracker]')).toBeNull();
});
```

Implementation:

```ts
function formatPresenterTimer(elapsedMs: number, plannedDurationMs: number | null | undefined): string {
  if (plannedDurationMs == null) return formatElapsed(elapsedMs);
  const base = `${formatElapsed(elapsedMs)} / ${formatElapsed(plannedDurationMs)}`;
  if (elapsedMs <= plannedDurationMs) return base;
  return `${base} +${formatElapsed(elapsedMs - plannedDurationMs)}`;
}
```

```ts
const plannedDurationMs = mainShell.manifest?.plannedDurationMs ?? null;
const trackerCleanup =
  plannedDurationMs == null
    ? () => undefined
    : installTimeTracker({
        root: options.root.querySelector<HTMLElement>("aside")!,
        shell: mainShell,
        plannedDurationMs,
        bus,
        window: win,
        document: doc,
        variant: "presenter"
      });

function tick(): void {
  const elapsedMs = mainShell.elapsedMs();
  timerRoot.textContent = formatPresenterTimer(elapsedMs, plannedDurationMs);
  timerRoot.toggleAttribute(
    "data-peitho-overrun",
    plannedDurationMs != null && elapsedMs > plannedDurationMs
  );
}
```

```ts
return {
  mainShell,
  previewShell,
  tick,
  destroy(): void {
    win.clearInterval(interval);
    trackerCleanup();
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    keyboardCleanup();
    syncCleanup();
    previewShell.destroy();
    mainShell.destroy();
  }
};
```

Verification:

```bash
cd packages/peitho-present && npx vitest run test/presenter.test.ts
```

## Task 12: Wire up the present/presenter HTML entries and add CSS

Goal: `present.html` shows the tracker only when `present.json`'s `presenterOpen` is false. `presenter.html` leaves the decision to the `mountPresenterView` side. `render_distribution_index` does not include the tracker.

Files:
- `crates/peitho-core/src/render.rs`

Test:

```rust
#[test]
fn present_index_fetches_present_config_and_mounts_time_tracker_conditionally() {
    let html = render_present_index();

    assert!(html.contains("fetchOk('present.json')"));
    assert!(html.contains("installTimeTracker"));
    assert!(html.contains("!config.presenterOpen"));
    assert!(html.contains("shell.manifest?.plannedDurationMs"));
}

#[test]
fn distribution_index_does_not_include_time_tracker() {
    let html = render_distribution_index();

    assert!(!html.contains("installTimeTracker"));
    assert!(!html.contains("peitho-time-tracker"));
    assert!(!html.contains("present.json"));
}
```

Implementation:

```js
import {
  installCanvasClickNavigation,
  installCloseOnEscape,
  installFullscreenShortcut,
  installKeyboardNavigation,
  installPresentationControls,
  installSyncBridge,
  installTimeTracker,
  mountPresentShell,
  serverSyncChannelFactory
} from './shell.js';
```

```js
const config = await fetchOk('present.json').then((response) => response.json());
installCloseOnEscape(window);
installKeyboardNavigation(window);
installSyncBridge(window, serverSyncChannelFactory());
installPresentationControls({ root, window, document });
installCanvasClickNavigation({ root, window });
installFullscreenShortcut({ window, document });
const shell = await mountPresentShell({ root });
const plannedDurationMs = shell.manifest?.plannedDurationMs ?? null;
if (plannedDurationMs != null && !config.presenterOpen) {
  installTimeTracker({
    root,
    shell,
    plannedDurationMs,
    window,
    document,
    variant: 'present'
  });
}
```

```css
.peitho-time-tracker { position: absolute; left: 0; right: 0; bottom: 0; height: 6px; z-index: 20; pointer-events: none; background: rgba(255, 255, 255, 0.18); }
.peitho-time-tracker[data-peitho-time-tracker="presenter"] { position: relative; height: 26px; margin: 12px 0; background: rgba(255, 255, 255, 0.16); }
.peitho-time-tracker [data-peitho-marker] { position: absolute; transform: translateX(-50%); transition: left 120ms linear; font-size: 18px; line-height: 1; }
.peitho-time-tracker [data-peitho-marker="rabbit"] { top: -18px; }
.peitho-time-tracker [data-peitho-marker="turtle"] { bottom: -18px; }
.peitho-time-tracker[data-peitho-overrun] { background: rgba(255, 92, 92, 0.35); }
[data-peitho-presenter="timer"][data-peitho-overrun] { color: #ff8a8a; }
```

Verification:

```bash
cargo test -p peitho-core present_index_fetches_present_config_and_mounts_time_tracker_conditionally
cargo test -p peitho-core distribution_index_does_not_include_time_tracker
```

## Task 13: Rebuild shell.js and pin down the generated TS API

Goal: Reflect `installTimeTracker` in the public API and bundle, and check for drift between `bindings/` and `dist/shell.js`.

Files:
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/generated.test.ts`
- `packages/peitho-present/dist/shell.js`
- `bindings/Manifest.ts`
- `bindings/PresentConfig.ts`

Test:

```ts
import { installTimeTracker } from "../src/index";
import type { PresentConfig } from "../../../bindings/PresentConfig";

it("exports time tracker and present config contract", () => {
  const config: PresentConfig = { version: 1, presenterOpen: true };

  expect(typeof installTimeTracker).toBe("function");
  expect(config.presenterOpen).toBe(true);
});
```

Implementation:

```ts
export { installTimeTracker } from "./timeTracker";
export type { TimeTrackerOptions } from "./timeTracker";
```

Verification:

```bash
cd packages/peitho-present && npx vitest run test/generated.test.ts
cd packages/peitho-present && npm run build
git diff --exit-code bindings/
git diff --exit-code packages/peitho-present/dist/shell.js
```

## Task 14: Add the frontmatter configuration policy to `CLAUDE.md`

Goal: Record zero-config and the deck-top frontmatter as the first instance of deck-level settings, and drop the `peitho.toml` premise from the pending list.

Files:
- `CLAUDE.md`

Test:

```bash
rg -q "Deck-level settings are carried in Markdown frontmatter" CLAUDE.md
rg -q "time is specified in the deck's leading frontmatter" CLAUDE.md
rg -q "peitho.toml (not introduced for now" CLAUDE.md
```

Implementation:

```markdown
- Deck-level settings are carried in Markdown frontmatter (adopted 2026-07-03). The first key is `time`, and the planned duration is written only in the deck's leading frontmatter. Unknown frontmatter keys, invalid values, and misplacement are build errors with line numbers + help
```

```markdown
- peitho.toml (not introduced for now. Deck-level settings run on frontmatter; the author decides once a customization need arises that frontmatter cannot express)
```

Verification:

```bash
rg -n "frontmatter|peitho.toml|deck's leading frontmatter" CLAUDE.md
```

## Task 15: Pass all gates and a real-browser E2E check

Goal: Pass all the gates in CLAUDE.md, and confirm the display target and no-`time` compatibility in a real browser.

Files:
- `CLAUDE.md`
- `/tmp/peitho-time.md`
- `/tmp/peitho-no-time.md`

Test:

```markdown
E2E pass criteria:
- Single-screen setup: with `time` present, the tracker appears at the bottom edge of the presentation screen
- `--presenter-windowed`: with `time` present, the tracker appears only on the presenter screen, not on the presentation screen
- Without `time`: the tracker appears on neither the presentation screen nor the presenter screen
- After advancing to the second slide via `curl POST /sync`, the turtle progresses from 0%
```

Implementation:

```markdown
This task does not change any implementation files. Only gate and E2E failures are sent back to earlier tasks for fixing.
```

Verification:

```bash
cargo test --workspace          # run 3 times in a row (there has been a test-race incident in the past)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/  # contract drift
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js  # built-in shell drift (after npm run build)
```

```bash
cargo test --workspace
cargo test --workspace
```

```bash
cat >/tmp/peitho-time.md <<'EOF'
---
time: 1m
---

# Intro

---

# Details
EOF
```

```bash
cat >/tmp/peitho-no-time.md <<'EOF'
# Intro

---

# Details
EOF
```

```bash
cargo run -p peitho -- present /tmp/peitho-time.md --port 43101 &
PEITHO_PID=$!
sleep 2
curl -sS -X POST -H 'Content-Type: application/json' --data '{"index":1}' http://127.0.0.1:43101/sync
screencapture -x -D 1 /tmp/peitho-time-one-screen.png
curl -sS -X POST -H 'Content-Type: application/json' --data '{"close":true}' http://127.0.0.1:43101/sync
wait "$PEITHO_PID"
```

```bash
cargo run -p peitho -- present /tmp/peitho-time.md --port 43102 --presenter-windowed &
PEITHO_PID=$!
sleep 2
curl -sS -X POST -H 'Content-Type: application/json' --data '{"index":1}' http://127.0.0.1:43102/sync
screencapture -x -D 1 /tmp/peitho-time-windowed-display-1.png
screencapture -x -D 2 /tmp/peitho-time-windowed-display-2.png
curl -sS -X POST -H 'Content-Type: application/json' --data '{"close":true}' http://127.0.0.1:43102/sync
wait "$PEITHO_PID"
```

```bash
cargo run -p peitho -- present /tmp/peitho-no-time.md --port 43103 --presenter-windowed &
PEITHO_PID=$!
sleep 2
curl -sS -X POST -H 'Content-Type: application/json' --data '{"index":1}' http://127.0.0.1:43103/sync
screencapture -x -D 1 /tmp/peitho-no-time-display-1.png
screencapture -x -D 2 /tmp/peitho-no-time-display-2.png
curl -sS -X POST -H 'Content-Type: application/json' --data '{"close":true}' http://127.0.0.1:43103/sync
wait "$PEITHO_PID"
```

## Summary

<!-- derived-from #task-1-serde_norway-workspace- -->
<!-- derived-from #task-2-pulldown-cmark-metadata-block-frontmatter- -->
<!-- derived-from #task-5-typestate-deck- -->
<!-- derived-from #task-9-cli-presenter_open-presentjson- -->
<!-- derived-from #task-10-timetrackerts-ui- -->
<!-- derived-from #task-15-e2e- -->

This plan proceeds in dependency order from adding the dependency, through the parser, typestate, Rust/TS type contract, present-cache, presentation shell UI, HTML wiring, documentation updates, gates, and real-browser E2E. If a decision is needed during implementation, prioritize D1–D7 of the design document and the invariants in CLAUDE.md, and do not deviate from the type names and function names in this plan.
