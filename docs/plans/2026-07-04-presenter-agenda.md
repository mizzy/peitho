# Presenter Agenda Implementation Plan

<!-- constrained-by ../specs/2026-07-04-presenter-agenda-design.md -->
<!-- constrained-by ../../CLAUDE.md -->

> **Note (2026-07-04, post-implementation)**: This plan is a snapshot at the start of work (docs/plans/ is history). The spec was revised during implementation review; the following differ from the plan as written vs. the final implementation: the row state attribute was renamed to `data-peitho-agenda-outcome` (the `data-peitho-agenda-delta` naming in Tasks 8/11 is the old name), presenter.ts calls `installAgenda` unconditionally (the ternary guard in Task 10 was dropped), Task 9 gained a flush to `previousIndex` and a `peitho:timercontrol` reset subscription, under/over judgment uses rounded seconds as the single source, and a second page settings comment on the same slide is uniformly an error. See the spec for the canonical version.

## Premises

- Implement only the approved design in `docs/specs/2026-07-04-presenter-agenda-design.md`.
- Do not change behavior for decks without section markers: no agenda DOM, no changed present-side tracker DOM, no changed timer semantics.
- Every invalid section state is a build error with a line number and help text.
- Rust domain types remain the source of truth; TypeScript consumes generated `bindings/*.ts`.

## Task 1: Parse `section` / `time` in page comments and reject marker-local invalid states

**Goal**: Extend page settings comments to accept `section` and `time`, while rejecting `section` without `time`, `time` without `section`, and empty section names at the marker line.

**Files**:

- `crates/peitho-core/src/parser.rs`

**Test**:

```rust
#[test]
fn parses_section_page_comment_with_time_key_and_layout() {
    let settings = parse_page_comment(
        r#"<!-- {"key":"cover","layout":"cover","section":"Setup","time":"1m"} -->"#,
        7,
    )
    .unwrap()
    .unwrap();

    let section = settings.section.unwrap();
    assert_eq!(section.name, "Setup");
    assert_eq!(section.planned.as_millis(), 60_000);
    assert_eq!(section.line, 7);
    assert_eq!(settings.layout.as_deref(), Some("cover"));
    assert_eq!(settings.key.unwrap().as_str(), "cover");
}

#[test]
fn rejects_invalid_section_page_comments_with_line_and_help() {
    let cases = [
        (
            r#"<!-- {"section":"Setup"} -->"#,
            "section marker is missing time",
            r#"add "time":"1m" to this section marker"#,
        ),
        (
            r#"<!-- {"time":"1m"} -->"#,
            "section time requires a section name",
            "put deck-wide time in YAML frontmatter instead",
        ),
        (
            r#"<!-- {"section":"","time":"1m"} -->"#,
            "section name must not be empty",
            r#"set "section" to the agenda label shown in presenter view"#,
        ),
    ];

    for (raw, message, help) in cases {
        let err = parse_page_comment(raw, 3).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains(message));
        assert_eq!(err.help, help);
    }
}
```

**Implementation**:

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageComment {
    key: Option<String>,
    layout: Option<String>,
    section: Option<String>,
    time: Option<PlannedTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PageSectionMarker {
    name: String,
    planned: PlannedTime,
    line: usize,
}

struct PageSettings {
    key: Option<SlideKey>,
    layout: Option<String>,
    section: Option<PageSectionMarker>,
}
```

```rust
let section = match (parsed.section.as_deref(), parsed.time) {
    (Some(name), Some(planned)) if !name.trim().is_empty() => Some(PageSectionMarker {
        name: name.trim().to_owned(),
        planned,
        line,
    }),
    (Some(_), Some(_)) => {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "section name must not be empty",
            r#"set "section" to the agenda label shown in presenter view"#,
        ));
    }
    (Some(_), None) => {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "section marker is missing time",
            r#"add "time":"1m" to this section marker"#,
        ));
    }
    (None, Some(_)) => {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "section time requires a section name",
            "put deck-wide time in YAML frontmatter instead",
        ));
    }
    (None, None) => None,
};
```

**Verification**:

```bash
cargo test -p peitho-core parser::tests::parses_section_page_comment_with_time_key_and_layout
cargo test -p peitho-core parser::tests::rejects_invalid_section_page_comments_with_line_and_help
```

## Task 2: Add `DeckSection` and make `DeckSettings` carry owned sections

**Goal**: Add the validated section model to the phase layer, remove `Copy` from `DeckSettings`, and keep settings moving through `Parsed -> Mapped -> Checked -> Rendered`.

**Files**:

- `crates/peitho-core/src/phase.rs`
- `crates/peitho-core/src/parser.rs`

**Test**:

```rust
#[test]
fn deck_settings_carry_owned_sections() {
    let setup = DeckSection::new(
        "Setup".to_owned(),
        PlannedTime::from_millis(60_000).unwrap(),
        0,
        1,
    );
    let settings = DeckSettings::new(Some(setup.planned()), vec![setup.clone()]);

    assert_eq!(settings.planned_time().unwrap().as_millis(), 60_000);
    assert_eq!(settings.sections(), &[setup]);
    assert_eq!(settings.sections()[0].name(), "Setup");
    assert_eq!(settings.sections()[0].start(), 0);
    assert_eq!(settings.sections()[0].end(), 1);
}
```

**Implementation**:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckSection {
    name: String,
    planned: PlannedTime,
    start: usize,
    end: usize,
}

impl DeckSection {
    pub(crate) fn new(name: String, planned: PlannedTime, start: usize, end: usize) -> Self {
        Self { name, planned, start, end }
    }

    pub fn name(&self) -> &str { &self.name }
    pub fn planned(&self) -> PlannedTime { self.planned }
    pub fn start(&self) -> usize { self.start }
    pub fn end(&self) -> usize { self.end }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeckSettings {
    planned_time: Option<PlannedTime>,
    sections: Vec<DeckSection>,
}

impl DeckSettings {
    pub fn new(planned_time: Option<PlannedTime>, sections: Vec<DeckSection>) -> Self {
        Self { planned_time, sections }
    }

    pub fn planned_time(&self) -> Option<PlannedTime> { self.planned_time }
    pub fn sections(&self) -> &[DeckSection] { &self.sections }

    pub(crate) fn with_sections(mut self, sections: Vec<DeckSection>) -> Self {
        self.sections = sections;
        self
    }
}
```

```rust
Ok(DeckSettings::new(parsed.time, Vec::new()))
```

**Verification**:

```bash
cargo test -p peitho-core phase::tests::deck_settings_carry_owned_sections
cargo test -p peitho-core parser::tests::parses_planned_time_values_from_frontmatter
```

## Task 3: Resolve section ranges at parse end

**Goal**: Convert slide-local markers into `DeckSection` ranges after every slide is parsed; decks without markers keep empty sections, and decks with any marker require slide 1 to have one.

**Files**:

- `crates/peitho-core/src/parser.rs`
- `crates/peitho-core/src/phase.rs`

**Test**:

```rust
#[test]
fn resolves_section_ranges_from_marker_positions() {
    let deck = parse_markdown(
        "---\ntime: 3m\n---\n\
         <!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# A\n\n---\n# B\n\n---\n\
         <!-- {\"section\":\"Demo\",\"time\":\"2m\"} -->\n# C",
    )
    .unwrap();

    let sections = deck.settings().sections();
    assert_eq!(sections.len(), 2);
    assert_eq!((sections[0].name(), sections[0].start(), sections[0].end()), ("Setup", 0, 1));
    assert_eq!((sections[1].name(), sections[1].start(), sections[1].end()), ("Demo", 2, 2));
}

#[test]
fn decks_without_section_markers_keep_settings_unchanged() {
    let deck = parse_markdown("# A\n\n---\n# B").unwrap();

    assert_eq!(deck.settings().planned_time(), None);
    assert!(deck.settings().sections().is_empty());
}

#[test]
fn rejects_section_markers_when_first_slide_has_no_marker() {
    let err = parse_markdown("# A\n\n---\n<!-- {\"section\":\"Late\",\"time\":\"1m\"} -->\n# B")
        .unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(4));
    assert!(err.to_string().contains("first slide must declare a section"));
    assert_eq!(err.help, "add a section marker to the first slide or remove all section markers");
}
```

**Implementation**:

```rust
struct ParsedSlideDraft {
    slide: ParsedSlide,
    section: Option<PageSectionMarker>,
}

struct ResolvedSection {
    section: DeckSection,
    line: usize,
}

fn parse_markdown(source: &str) -> Result<Deck<Parsed>> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let SplitSlides { frontmatter, ranges } = split_slide_ranges(source)?;
    let settings = parse_deck_frontmatter(frontmatter.as_ref())?;
    if ranges.is_empty() {
        return Err(BuildError::new(
            ErrorKind::Parse,
            None,
            "deck has no slides",
            "add at least one slide with content before building",
        ));
    }

    let mut drafts = Vec::new();
    for (index, range) in ranges.into_iter().enumerate() {
        drafts.push(parse_slide(source, range, index)?);
    }

    let resolved_sections = resolve_deck_sections(&drafts)?;
    let sections = resolved_sections
        .into_iter()
        .map(|resolved| resolved.section)
        .collect::<Vec<_>>();
    let slides = drafts.into_iter().map(|draft| draft.slide).collect::<Vec<_>>();
    validate_unique_keys(&slides)?;
    Ok(Deck::parsed(settings.with_sections(sections), slides))
}
```

```rust
fn resolve_deck_sections(drafts: &[ParsedSlideDraft]) -> Result<Vec<ResolvedSection>> {
    let markers = drafts
        .iter()
        .enumerate()
        .filter_map(|(index, draft)| draft.section.as_ref().map(|marker| (index, marker)))
        .collect::<Vec<_>>();

    if markers.is_empty() {
        return Ok(Vec::new());
    }
    if markers[0].0 != 0 {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(markers[0].1.line),
            "first slide must declare a section",
            "add a section marker to the first slide or remove all section markers",
        ));
    }

    Ok(markers
        .iter()
        .enumerate()
        .map(|(marker_index, (start, marker))| {
            let end = markers
                .get(marker_index + 1)
                .map(|(next_start, _)| next_start - 1)
                .unwrap_or(drafts.len() - 1);
            ResolvedSection {
                section: DeckSection::new(marker.name.clone(), marker.planned, *start, end),
                line: marker.line,
            }
        })
        .collect())
}
```

**Verification**:

```bash
cargo test -p peitho-core parser::tests::resolves_section_ranges_from_marker_positions
cargo test -p peitho-core parser::tests::decks_without_section_markers_keep_settings_unchanged
cargo test -p peitho-core parser::tests::rejects_section_markers_when_first_slide_has_no_marker
```

## Task 4: Validate and derive total planned time from sections

**Goal**: Make section totals authoritative when frontmatter `time` is absent, require equality when it is present, and report checked-add overflow / planned-time upper-bound errors.

**Files**:

- `crates/peitho-core/src/parser.rs`
- `crates/peitho-core/src/phase.rs`

**Test**:

```rust
#[test]
fn derives_deck_time_from_section_sum_when_frontmatter_time_is_absent() {
    let deck = parse_markdown(
        "<!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# A\n\n---\n\
         <!-- {\"section\":\"Demo\",\"time\":\"2m\"} -->\n# B",
    )
    .unwrap();

    assert_eq!(deck.settings().planned_time().map(PlannedTime::as_millis), Some(180_000));
}

#[test]
fn rejects_frontmatter_time_that_differs_from_section_total() {
    let err = parse_markdown(
        "---\ntime: 5m\n---\n\
         <!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# A\n\n---\n\
         <!-- {\"section\":\"Demo\",\"time\":\"2m\"} -->\n# B",
    )
    .unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(4));
    assert!(err.to_string().contains("frontmatter time 300000ms does not match section total 180000ms"));
    assert_eq!(err.help, "adjust frontmatter time or section times so the totals match");
}

#[test]
fn rejects_section_time_total_above_manifest_safe_integer_limit() {
    let err = parse_markdown(
        "<!-- {\"section\":\"A\",\"time\":\"9007199254740s\"} -->\n# A\n\n---\n\
         <!-- {\"section\":\"B\",\"time\":\"9007199254740s\"} -->\n# B",
    )
    .unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert!(err.to_string().contains(PlannedTime::TOO_LARGE_MESSAGE));
    assert_eq!(err.help, "reduce section times so the total is at most Number.MAX_SAFE_INTEGER milliseconds");
}

#[test]
fn checked_add_overflow_in_section_sum_reports_line_and_help() {
    let err = section_total_from_millis([(2, u64::MAX), (5, 1)]).unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(5));
    assert!(err.to_string().contains("section time total overflowed"));
    assert_eq!(err.help, "reduce section times so the total can be represented safely");
}
```

**Implementation**:

```rust
fn finalize_section_settings(
    settings: DeckSettings,
    resolved_sections: Vec<ResolvedSection>,
) -> Result<DeckSettings> {
    if resolved_sections.is_empty() {
        return Ok(settings.with_sections(Vec::new()));
    }

    let first_line = resolved_sections[0].line;
    let total = section_total_from_millis(
        resolved_sections
            .iter()
            .map(|resolved| (resolved.line, resolved.section.planned().as_millis())),
    )?;
    let section_total = PlannedTime::from_millis(total).map_err(|message| {
        BuildError::new(
            ErrorKind::Parse,
            Some(first_line),
            message,
            "reduce section times so the total is at most Number.MAX_SAFE_INTEGER milliseconds",
        )
    })?;

    if let Some(frontmatter) = settings.planned_time() {
        if frontmatter.as_millis() != section_total.as_millis() {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(first_line),
                format!(
                    "frontmatter time {}ms does not match section total {}ms",
                    frontmatter.as_millis(),
                    section_total.as_millis()
                ),
                "adjust frontmatter time or section times so the totals match",
            ));
        }
        return Ok(settings.with_sections(
            resolved_sections.into_iter().map(|resolved| resolved.section).collect(),
        ));
    }

    Ok(DeckSettings::new(
        Some(section_total),
        resolved_sections.into_iter().map(|resolved| resolved.section).collect(),
    ))
}

fn section_total_from_millis<I>(items: I) -> Result<u64>
where
    I: IntoIterator<Item = (usize, u64)>,
{
    let mut total = 0_u64;
    for (line, millis) in items {
        total = total.checked_add(millis).ok_or_else(|| {
            BuildError::new(
                ErrorKind::Parse,
                Some(line),
                "section time total overflowed",
                "reduce section times so the total can be represented safely",
            )
        })?;
    }
    Ok(total)
}
```

```rust
let resolved_sections = resolve_deck_sections(&drafts)?;
let settings = finalize_section_settings(settings, resolved_sections)?;
let slides = drafts.into_iter().map(|draft| draft.slide).collect::<Vec<_>>();
validate_unique_keys(&slides)?;
Ok(Deck::parsed(settings, slides))
```

**Verification**:

```bash
cargo test -p peitho-core parser::tests::derives_deck_time_from_section_sum_when_frontmatter_time_is_absent
cargo test -p peitho-core parser::tests::rejects_frontmatter_time_that_differs_from_section_total
cargo test -p peitho-core parser::tests::rejects_section_time_total_above_manifest_safe_integer_limit
cargo test -p peitho-core parser::tests::checked_add_overflow_in_section_sum_reports_line_and_help
```

## Task 5: Add sections to `manifest.json`

**Goal**: Serialize `sections` as an always-present manifest field, add `ManifestSection`, keep manifest `version` at 1, and preserve additive deserialization compatibility.

**Files**:

- `crates/peitho-core/src/manifest.rs`
- `crates/peitho-core/src/lib.rs`

**Test**:

```rust
#[test]
fn serializes_manifest_schema_exactly() {
    let manifest = Manifest::new(
        "Peitho Architecture",
        None,
        Vec::new(),
        vec![ManifestSlide::new(0, SlideKey::new("arch-1").unwrap(), "slides/000-arch-1.html", false)],
    );

    let json = manifest_json(&manifest).unwrap();

    assert_eq!(
        json,
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"",
            env!("CARGO_PKG_VERSION"),
            "\",\n",
            "  \"title\": \"Peitho Architecture\",\n",
            "  \"slideCount\": 1,\n",
            "  \"plannedDurationMs\": null,\n",
            "  \"sections\": [],\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        )
    );
}

#[test]
fn build_manifest_serializes_sections_from_checked_deck() {
    let checked = checked_deck(
        "---\ntime: 3m\n---\n\
         <!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# Intro\n\n---\n# More\n\n---\n\
         <!-- {\"section\":\"Demo\",\"time\":\"2m\"} -->\n# Demo",
        title_body_layout(),
    );

    let manifest = build_manifest(&checked);

    assert_eq!(manifest.sections()[0].name(), "Setup");
    assert_eq!(manifest.sections()[0].start_index(), 0);
    assert_eq!(manifest.sections()[0].end_index(), 1);
    assert_eq!(manifest.sections()[0].planned_duration_ms(), 60_000);
    assert_eq!(manifest.sections()[1].name(), "Demo");
}

#[test]
fn deserializes_manifest_missing_sections_as_empty_for_additive_compatibility() {
    let json = concat!(
        "{\n",
        "  \"version\": 1,\n",
        "  \"peithoVersion\": \"0.1.0\",\n",
        "  \"title\": \"Deck\",\n",
        "  \"slideCount\": 1,\n",
        "  \"plannedDurationMs\": null,\n",
        "  \"slides\": [{\"index\":0,\"key\":\"intro\",\"src\":\"slides/000-intro.html\",\"hasNotes\":false}]\n",
        "}\n"
    );

    let manifest: Manifest = serde_json::from_str(json).unwrap();
    assert!(manifest.sections().is_empty());
}
```

**Implementation**:

```rust
#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(any(test, feature = "ts-bindings"), ts(export, export_to = "../../bindings/"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSection {
    name: String,
    #[serde(rename = "startIndex")]
    start_index: usize,
    #[serde(rename = "endIndex")]
    end_index: usize,
    #[serde(rename = "plannedDurationMs")]
    planned_duration_ms: u64,
}

impl ManifestSection {
    pub fn new(name: impl Into<String>, start_index: usize, end_index: usize, planned_duration_ms: u64) -> Self {
        Self { name: name.into(), start_index, end_index, planned_duration_ms }
    }

    pub fn name(&self) -> &str { &self.name }
    pub fn start_index(&self) -> usize { self.start_index }
    pub fn end_index(&self) -> usize { self.end_index }
    pub fn planned_duration_ms(&self) -> u64 { self.planned_duration_ms }
}
```

```rust
#[serde(default)]
sections: Vec<ManifestSection>,

pub fn build_manifest(deck: &Deck<Checked>) -> Manifest {
    let sections = deck
        .settings()
        .sections()
        .iter()
        .map(|section| {
            ManifestSection::new(
                section.name(),
                section.start(),
                section.end(),
                section.planned().as_millis(),
            )
        })
        .collect();

    Manifest::new(
        title,
        deck.settings().planned_time().map(PlannedTime::as_millis),
        sections,
        slides,
    )
}
```

```rust
pub use manifest::{build_manifest, fragment_src, manifest_json, Manifest, ManifestSection, ManifestSlide};
```

**Verification**:

```bash
cargo test -p peitho-core manifest::tests::serializes_manifest_schema_exactly
cargo test -p peitho-core manifest::tests::build_manifest_serializes_sections_from_checked_deck
cargo test -p peitho-core manifest::tests::deserializes_manifest_missing_sections_as_empty_for_additive_compatibility
```

## Task 6: Regenerate manifest bindings with `ManifestSection`

**Goal**: Commit the generated TypeScript contract for `sections` before TS agenda work depends on it.

**Files**:

- `crates/peitho-core/src/manifest.rs`
- `bindings/Manifest.ts`
- `bindings/ManifestSection.ts`
- `bindings/ManifestSlide.ts`

**Test**:

```rust
#[test]
fn exports_manifest_bindings_with_serde_field_names() {
    let cfg = Config::from_env();
    Manifest::export_all(&cfg).unwrap();
    ManifestSection::export_all(&cfg).unwrap();
    ManifestSlide::export_all(&cfg).unwrap();

    let root_bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
    let manifest = fs::read_to_string(root_bindings.join("Manifest.ts")).unwrap();
    let section = fs::read_to_string(root_bindings.join("ManifestSection.ts")).unwrap();

    assert!(manifest.contains(r#"import type { ManifestSection } from "./ManifestSection";"#));
    assert!(manifest.contains("sections: Array<ManifestSection>"));
    assert!(section.contains("startIndex: number"));
    assert!(section.contains("endIndex: number"));
    assert!(section.contains("plannedDurationMs: number"));
}
```

**Implementation**:

```ts
// bindings/ManifestSection.ts
export type ManifestSection = {
  name: string;
  startIndex: number;
  endIndex: number;
  plannedDurationMs: number;
};
```

```ts
// bindings/Manifest.ts
import type { ManifestSection } from "./ManifestSection";
import type { ManifestSlide } from "./ManifestSlide";

export type Manifest = {
  version: number;
  peithoVersion: string;
  title: string;
  slideCount: number;
  plannedDurationMs: number | null;
  sections: Array<ManifestSection>;
  slides: Array<ManifestSlide>;
};
```

**Verification**:

```bash
cargo test -p peitho-core manifest::ts_tests::exports_manifest_bindings_with_serde_field_names
test -f bindings/ManifestSection.ts
```

## Task 7: Share the `m:ss` formatter from `timeTracker.ts`

**Goal**: Expose the existing tracker-style `m:ss` formatter so agenda and tracker use the same display rule.

**Files**:

- `packages/peitho-present/src/timeTracker.ts`
- `packages/peitho-present/test/timeTracker.test.ts`

**Test**:

```ts
it("formats durations as tracker-style m:ss", () => {
  expect(formatMinuteSeconds(0)).toBe("0:00");
  expect(formatMinuteSeconds(15_000)).toBe("0:15");
  expect(formatMinuteSeconds(90_000)).toBe("1:30");
  expect(formatMinuteSeconds(3_600_000)).toBe("60:00");
});
```

**Implementation**:

```ts
export function formatMinuteSeconds(ms: number): string {
  const totalSeconds = Math.round(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

function timeScaleLabels(plannedDurationMs: number): string[] {
  return Array.from({ length: 5 }, (_, index) =>
    formatMinuteSeconds((plannedDurationMs * index) / 4)
  );
}
```

**Verification**:

```bash
cd packages/peitho-present && npx vitest run test/timeTracker.test.ts
```

## Task 8: Create `agenda.ts` and render agenda rows only when sections exist

**Goal**: Mount no DOM for empty sections; render the mock-compatible agenda head and 4-column rows with marker, label, actual/planned time, and done-only delta state.

**Files**:

- `packages/peitho-present/src/agenda.ts`
- `packages/peitho-present/test/agenda.test.ts`

**Test**:

```ts
it("does not mount agenda when sections are empty", () => {
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 0 }),
    sections: [],
    bus: new EventTarget(),
    window,
    document
  });

  expect(root.innerHTML).toBe("");
  cleanup();
});

it("renders agenda header and rows with mock-compatible structure", () => {
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 2 }),
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 1, plannedDurationMs: 60_000 },
      { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 120_000 }
    ],
    bus: new EventTarget(),
    window,
    document
  });

  expect(root.querySelector("[data-peitho-agenda-title]")?.textContent).toBe("Agenda");
  expect(root.querySelector("[data-peitho-agenda-hint]")?.textContent).toBe("Actual / Planned");
  const rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows.map((row) => row.dataset.peithoAgendaState)).toEqual(["done", "current"]);
  expect(rows[0].children.length).toBe(4);
  expect(rows[0].querySelector("[data-peitho-agenda-marker]")).not.toBeNull();
  expect(rows[0].querySelector("[data-peitho-agenda-label]")).not.toBeNull();
  expect(rows[0].querySelector('[data-peitho-agenda-name]')?.textContent).toBe("Setup");
  expect(rows[0].querySelector('[data-peitho-agenda-range]')?.textContent).toBe("01–02");
  expect(rows[0].dataset.peithoAgendaDelta).toBe("under");
  expect(rows[0].querySelector('[data-peitho-agenda-delta]')?.textContent).toBe("−1:00");
  expect(rows[1].querySelector('[data-peitho-agenda-range]')?.textContent).toBe("03");
  expect(rows[1].querySelector('[data-peitho-agenda-time]')?.textContent).toBe("0:00 / 2:00");
  expect(rows[1].querySelector('[data-peitho-agenda-delta]')?.textContent).toBe("·");
  expect(rows[1].hasAttribute("data-peitho-agenda-delta")).toBe(false);
  cleanup();
});
```

**Implementation**:

```ts
import type { ManifestSection } from "../../../bindings/ManifestSection";
import type { PresentShell } from "./shell";
import { formatMinuteSeconds } from "./timeTracker";

const EM_DASH = "—";
const MINUS_SIGN = "−";

export type AgendaOptions = {
  root: HTMLElement;
  shell: Pick<PresentShell, "currentIndex" | "elapsedMs" | "startedAt">;
  sections: ManifestSection[];
  window?: Window;
  document?: Document;
  bus?: EventTarget;
};

export function installAgenda(options: AgendaOptions): () => void {
  if (options.sections.length === 0) return () => undefined;

  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
  const host = doc.createElement("section");
  host.dataset.peithoAgenda = "true";
  host.innerHTML = [
    '<div data-peitho-agenda-head>',
    '<span data-peitho-agenda-title>Agenda</span>',
    '<span data-peitho-agenda-hint>Actual / Planned</span>',
    "</div>",
    '<div data-peitho-agenda-list></div>'
  ].join("");
  options.root.appendChild(host);
  const list = host.querySelector<HTMLElement>("[data-peitho-agenda-list]")!;

  const actualMs = new Array(options.sections.length).fill(0) as number[];

  function sectionIndexForSlide(slideIndex: number): number {
    return options.sections.findIndex(
      (section) => slideIndex >= section.startIndex && slideIndex <= section.endIndex
    );
  }

  function render(): void {
    const currentSection = sectionIndexForSlide(options.shell.currentIndex);
    list.replaceChildren(
      ...options.sections.map((section, index) => renderRow(doc, section, index, currentSection, actualMs[index]))
    );
  }

  function renderRow(
    doc: Document,
    section: ManifestSection,
    index: number,
    currentSection: number,
    actual: number
  ): HTMLElement {
    const state = index < currentSection ? "done" : index === currentSection ? "current" : "upcoming";
    const row = doc.createElement("div");
    row.dataset.peithoAgendaRow = "true";
    row.dataset.peithoAgendaState = state;
    if (state === "done") {
      row.dataset.peithoAgendaDelta = actual > section.plannedDurationMs ? "over" : "under";
    }
    row.innerHTML = [
      `<span data-peitho-agenda-marker aria-hidden="true"></span>`,
      `<span data-peitho-agenda-label><span data-peitho-agenda-name></span><span data-peitho-agenda-range></span></span>`,
      `<span data-peitho-agenda-time></span>`,
      `<span data-peitho-agenda-delta></span>`
    ].join("");
    row.querySelector("[data-peitho-agenda-name]")!.textContent = section.name;
    row.querySelector("[data-peitho-agenda-range]")!.textContent = formatSlideRange(section);
    row.querySelector("[data-peitho-agenda-time]")!.textContent =
      `${state === "upcoming" ? EM_DASH : formatMinuteSeconds(actual)} / ${formatMinuteSeconds(section.plannedDurationMs)}`;
    row.querySelector("[data-peitho-agenda-delta]")!.textContent =
      state === "done" ? deltaText(state, actual, section.plannedDurationMs) : "·";
    return row;
  }

  function formatSlideRange(section: ManifestSection): string {
    const start = String(section.startIndex + 1).padStart(2, "0");
    const end = String(section.endIndex + 1).padStart(2, "0");
    return section.startIndex === section.endIndex ? start : `${start}–${end}`;
  }

  function deltaText(state: "done" | "current" | "upcoming", actual: number, planned: number): string {
    if (state !== "done") return "·";
    const diff = actual - planned;
    const sign = diff > 0 ? "+" : MINUS_SIGN;
    return `${sign}${formatMinuteSeconds(Math.abs(diff))}`;
  }

  function onSlideChange(): void {
    render();
  }

  function tick(): void {
    render();
  }

  render();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  const interval = win.setInterval(tick, 250);

  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    host.remove();
  };
}
```

**Verification**:

```bash
cd packages/peitho-present && npx vitest run test/agenda.test.ts
```

## Task 9: Accumulate section actual time on slidechange and 250ms ticks

**Goal**: Add elapsed deltas to the section containing the current slide, pause naturally when `elapsedMs()` does not advance, reset actuals when the timer is stopped, support backtracking, and tear down listener/interval cleanly.

**Files**:

- `packages/peitho-present/src/agenda.ts`
- `packages/peitho-present/test/agenda.test.ts`

**Test**:

```ts
it("accumulates elapsed deltas into the current section and resumes when returning", () => {
  vi.useFakeTimers();
  let elapsed = 0;
  let currentIndex = 0;
  let startedAt: number | null = 100;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: shell({
      get currentIndex() { return currentIndex; },
      elapsedMs: () => elapsed,
      startedAt: () => startedAt
    }),
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 1, plannedDurationMs: 1_000 },
      { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 1_000 }
    ],
    bus,
    window,
    document
  });

  elapsed = 1_000;
  vi.advanceTimersByTime(250);
  currentIndex = 2;
  bus.dispatchEvent(new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
    detail: { key: "demo", index: 2, total: 3, previousIndex: 0 }
  }));

  let rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].dataset.peithoAgendaState).toBe("done");
  expect(rows[0].dataset.peithoAgendaDelta).toBe("under");
  expect(rows[0].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("−0:00");
  expect(rows[1].dataset.peithoAgendaState).toBe("current");
  expect(rows[1].hasAttribute("data-peitho-agenda-delta")).toBe(false);

  elapsed = 3_000;
  vi.advanceTimersByTime(250);
  currentIndex = 0;
  bus.dispatchEvent(new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
    detail: { key: "setup", index: 0, total: 3, previousIndex: 2 }
  }));
  elapsed = 4_000;
  vi.advanceTimersByTime(250);

  const times = Array.from(root.querySelectorAll("[data-peitho-agenda-time]"), (node) => node.textContent);
  expect(times).toEqual(["0:02 / 0:01", "— / 0:01"]);

  rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].dataset.peithoAgendaState).toBe("current");
  expect(rows[0].hasAttribute("data-peitho-agenda-delta")).toBe(false);
  expect(rows[1].dataset.peithoAgendaState).toBe("upcoming");
  expect(rows[1].hasAttribute("data-peitho-agenda-delta")).toBe(false);

  currentIndex = 2;
  bus.dispatchEvent(new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
    detail: { key: "demo", index: 2, total: 3, previousIndex: 0 }
  }));
  rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].dataset.peithoAgendaDelta).toBe("over");
  expect(rows[0].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("+0:01");

  startedAt = null;
  elapsed = 0;
  vi.advanceTimersByTime(250);
  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:00 / 0:01");
  cleanup();
});

it("removes agenda interval listener and DOM on cleanup", () => {
  vi.useFakeTimers();
  let elapsed = 0;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: shell({ elapsedMs: () => elapsed }),
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window,
    document
  });

  expect(vi.getTimerCount()).toBe(1);
  cleanup();
  elapsed = 60_000;
  vi.advanceTimersByTime(250);
  bus.dispatchEvent(new CustomEvent("peitho:slidechange", {
    detail: { key: "only", index: 0, total: 1, previousIndex: null }
  }));

  expect(vi.getTimerCount()).toBe(0);
  expect(root.querySelector("[data-peitho-agenda]")).toBeNull();
});
```

**Implementation**:

```ts
let lastElapsedMs = options.shell.elapsedMs();

function tick(): void {
  const elapsedMs = options.shell.elapsedMs();
  if (options.shell.startedAt() === null) {
    actualMs.fill(0);
    lastElapsedMs = 0;
    render();
    return;
  }

  const delta = Math.max(0, elapsedMs - lastElapsedMs);
  const sectionIndex = sectionIndexForSlide(options.shell.currentIndex);
  if (sectionIndex >= 0) actualMs[sectionIndex] += delta;
  lastElapsedMs = elapsedMs;
  render();
}

function onSlideChange(): void {
  render();
}

function deltaText(state: "done" | "current" | "upcoming", actual: number, planned: number): string {
  if (state !== "done") return "·";
  const diff = actual - planned;
  const sign = diff > 0 ? "+" : MINUS_SIGN;
  return `${sign}${formatMinuteSeconds(Math.abs(diff))}`;
}
```

**Verification**:

```bash
cd packages/peitho-present && npx vitest run test/agenda.test.ts
```

## Task 10: Mount agenda from `presenter.ts` only when manifest sections are non-empty

**Goal**: Insert an agenda slot between tracker and controls, install agenda only for non-empty `manifest.sections`, update TS fixtures with `sections: []`, and keep `.clock` as flex column with controls at `margin-top: auto`.

**Files**:

- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/test/presenter.test.ts`
- `packages/peitho-present/test/generated.test.ts`

**Test**:

```ts
const manifest: Manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 2,
  plannedDurationMs: null,
  sections: [],
  slides: [
    { index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false },
    { index: 1, key: "details", src: "slides/001-details.html", hasNotes: false }
  ]
};

it("keeps agenda slot empty when manifest has no sections", async () => {
  const root = document.createElement("main");
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ sections: [] }),
    window,
    now: () => 1000,
    syncChannelFactory: mockSyncChannelFactory().factory
  });
  views.push(view);

  expect(root.querySelector('[data-peitho-presenter="agenda-slot"]')?.childElementCount).toBe(0);
  expect(root.querySelector("[data-peitho-agenda]")).toBeNull();
});

it("mounts agenda between tracker and controls when manifest has sections", async () => {
  const root = document.createElement("main");
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({
      plannedDurationMs: 180_000,
      sections: [
        { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
        { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 120_000 }
      ]
    }),
    window,
    now: () => 1000,
    syncChannelFactory: mockSyncChannelFactory().factory
  });
  views.push(view);

  const clockChildren = Array.from(
    root.querySelector('[data-peitho-presenter="clock"]')?.children ?? [],
    (node) => (node as HTMLElement).dataset.peithoPresenter ?? (node as HTMLElement).className
  );
  expect(clockChildren).toEqual(["clock-row", "tracker-slot", "agenda-slot", "controls"]);
  expect(root.querySelector('[data-peitho-presenter="agenda-slot"] [data-peitho-agenda]')).not.toBeNull();
});
```

**Implementation**:

```ts
import { installAgenda } from "./agenda";
```

```html
<div class="tracker-wrap" data-peitho-presenter="tracker-slot"></div>
<div data-peitho-presenter="agenda-slot"></div>
<div class="controls">
```

```ts
const agendaSlot = options.root.querySelector<HTMLElement>(
  '[data-peitho-presenter="agenda-slot"]'
)!;

const sections = mainShell.manifest?.sections ?? [];
const agendaCleanup =
  sections.length === 0
    ? () => undefined
    : installAgenda({
        root: agendaSlot,
        shell: mainShell,
        sections,
        bus,
        window: win,
        document: doc
      });
```

```ts
destroy(): void {
  win.clearInterval(interval);
  agendaCleanup();
  trackerCleanup();
  bus.removeEventListener("peitho:slidechange", onSlideChange);
  keyboardCleanup();
  syncCleanup();
  previewShell.destroy();
  mainShell.destroy();
}
```

**Verification**:

```bash
cd packages/peitho-present && npx vitest run test/presenter.test.ts test/generated.test.ts
```

## Task 11: Add agenda CSS to `render_presenter_index()`

**Goal**: Port the Claude Design mock `.agenda` styling into `render_presenter_index()`, rewriting selectors to `data-peitho-*` hooks while preserving the existing clock/control layout.

**Files**:

- `crates/peitho-core/src/render.rs`

**Test**:

```rust
#[test]
fn presenter_index_includes_agenda_css_with_data_selectors() {
    let html = render_presenter_index();

    assert!(html.contains(r#"[data-peitho-agenda] { overflow: hidden;"#));
    assert!(html.contains(r#"[data-peitho-agenda-head]"#));
    assert!(html.contains(r#"[data-peitho-agenda-list]"#));
    assert!(html.contains(r#"[data-peitho-agenda-row]"#));
    assert!(html.contains("grid-template-columns: 10px minmax(0, 1fr) auto auto"));
    assert!(html.contains(r#"[data-peitho-agenda-row] + [data-peitho-agenda-row]"#));
    assert!(html.contains(r#"[data-peitho-agenda-marker]"#));
    assert!(html.contains(r#"[data-peitho-agenda-state="done"]"#));
    assert!(html.contains(r#"[data-peitho-agenda-state="current"]"#));
    assert!(html.contains(r#"[data-peitho-agenda-state="upcoming"]"#));
    assert!(html.contains(r#"[data-peitho-agenda-label] { min-width: 0; display: flex; align-items: baseline; gap: 8px; }"#));
    assert!(html.contains(r#"[data-peitho-agenda-name] { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-mute); }"#));
    assert!(html.contains(r#"[data-peitho-agenda-state="done"] [data-peitho-agenda-name] { color: var(--fg-dim); }"#));
    assert!(html.contains(r#"[data-peitho-agenda-state="done"][data-peitho-agenda-delta="under"]"#));
    assert!(html.contains(r#"[data-peitho-agenda-state="done"][data-peitho-agenda-delta="over"]"#));
    assert!(!html.contains(r#"[data-peitho-agenda-delta="under"] {"#));
    assert!(html.contains(".clock { display: flex; flex-direction: column;"));
    assert!(html.contains(".controls {"));
    assert!(html.contains("margin-top: auto"));
    assert!(!html.contains(".agenda"));
}
```

**Implementation**:

```css
[data-peitho-agenda] { overflow: hidden; padding: 0 16px 14px; }
[data-peitho-agenda-head] { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 4px; color: var(--fg-dim); font-size: 10px; letter-spacing: 0.14em; text-transform: uppercase; }
[data-peitho-agenda-title] { color: var(--fg-mute); }
[data-peitho-agenda-hint] { white-space: nowrap; }
[data-peitho-agenda-list] { display: grid; }
[data-peitho-agenda-row] {
  display: grid;
  grid-template-columns: 10px minmax(0, 1fr) auto auto;
  gap: 8px;
  align-items: center;
  min-height: 28px;
  padding: 6px 0;
}
[data-peitho-agenda-row] + [data-peitho-agenda-row] { border-top: 1px solid var(--line-soft); }
[data-peitho-agenda-marker] { width: 8px; height: 8px; border-radius: 50%; border: 1px solid var(--fg-dim); box-sizing: border-box; }
[data-peitho-agenda-state="done"] [data-peitho-agenda-marker] { background: var(--fg-dim); border-color: var(--fg-dim); }
[data-peitho-agenda-state="current"] [data-peitho-agenda-marker] { background: var(--accent); border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
[data-peitho-agenda-state="upcoming"] [data-peitho-agenda-marker] { background: transparent; border-color: var(--fg-dim); }
[data-peitho-agenda-label] { min-width: 0; display: flex; align-items: baseline; gap: 8px; }
[data-peitho-agenda-name] { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-mute); }
[data-peitho-agenda-state="done"] [data-peitho-agenda-name] { color: var(--fg-dim); }
[data-peitho-agenda-state="current"] [data-peitho-agenda-name] { color: var(--fg); font-weight: 600; }
[data-peitho-agenda-range] { color: var(--fg-dim); font-size: 10px; letter-spacing: 0.08em; }
[data-peitho-agenda-time],
[data-peitho-agenda-delta] { font-family: "Geist Mono", ui-monospace, monospace; font-variant-numeric: tabular-nums; white-space: nowrap; color: var(--fg-dim); }
[data-peitho-agenda-state="current"] [data-peitho-agenda-time] { color: var(--accent); }
[data-peitho-agenda-state="done"][data-peitho-agenda-delta="under"] [data-peitho-agenda-time],
[data-peitho-agenda-state="done"][data-peitho-agenda-delta="under"] [data-peitho-agenda-delta] { color: color-mix(in oklch, var(--accent) 72%, var(--fg-mute)); }
[data-peitho-agenda-state="done"][data-peitho-agenda-delta="over"] [data-peitho-agenda-time],
[data-peitho-agenda-state="done"][data-peitho-agenda-delta="over"] [data-peitho-agenda-delta] { color: var(--warn); }
```

**Verification**:

```bash
cargo test -p peitho-core render::tests::presenter_index_includes_agenda_css_with_data_selectors
cargo test -p peitho-core render::tests::presenter_index_mounts_presenter_view_with_canvas_panes_and_notes
```

## Task 12: Add section markers to the lightning talk example

**Goal**: Make `examples/lightning-talk/deck.md` exercise real agenda data while keeping the existing 5-minute total.

**Files**:

- `examples/lightning-talk/deck.md`
- `crates/peitho/tests/build.rs`

**Test**:

```rust
#[test]
fn lightning_talk_example_declares_agenda_sections() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args(["build", "examples/lightning-talk/deck.md", "--out", out.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 5 slide(s)"));

    let manifest = fs::read_to_string(out.path().join("manifest.json")).unwrap();
    assert!(manifest.contains(r#""plannedDurationMs": 300000"#));
    assert!(manifest.contains(r#""sections": ["#));
    assert!(manifest.contains(r#""name": "Setup""#));
    assert!(manifest.contains(r#""startIndex": 2"#));
    assert!(manifest.contains(r#""endIndex": 3"#));
}
```

**Implementation**:

```markdown
<!-- {"key":"cover","section":"Setup","time":"1m"} -->
# I want to write slides in Markdown
```

```markdown
<!-- {"key":"problem","section":"Problem","time":"1m"} -->
# Once you start tweaking the design, prep never ends
```

```markdown
<!-- {"key":"separation","section":"Approach","time":"2m"} -->
# Separate content from design
```

```markdown
<!-- {"key":"closing","section":"Wrap-up","time":"1m"} -->
# Focus on the writing
```

**Verification**:

```bash
cargo test -p peitho --test build lightning_talk_example_declares_agenda_sections
cargo test -p peitho --test build lightning_talk_example_declares_five_minute_planned_duration
```

## Task 13: Rebuild the presenter bundle

**Goal**: Commit `dist/shell.js` after `agenda.ts` and presenter integration are complete.

**Files**:

- `packages/peitho-present/dist/shell.js`

**Test**:

```bash
rg -n "peithoAgenda|agenda-slot" packages/peitho-present/dist/shell.js
```

**Implementation**:

```bash
cd packages/peitho-present && npm run build
```

**Verification**:

```bash
cd packages/peitho-present
npm run build
rg -n "peithoAgenda|agenda-slot" dist/shell.js
```

## Task 14: Run final gates and real-browser E2E

**Goal**: Prove the whole change satisfies repository gates, generated artifact drift checks, and the presenter agenda UX in a real browser.

**Files**:

- None

**Test**:

```bash
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
```

**Implementation**:

```bash
cargo run -- present examples/lightning-talk/deck.md --port 4173 --no-open
```

Open `http://localhost:4173/presenter` in a browser and verify:

- timer card contains agenda rows between tracker and controls
- section ranges display as 1-based slide ranges
- current/done/upcoming states move when navigating
- actual time stops while paused and resumes in the current section
- decks without section markers still show no agenda

**Verification**:

```bash
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
cargo run -- present examples/lightning-talk/deck.md --port 4173 --no-open
```

## Open Questions

None.
