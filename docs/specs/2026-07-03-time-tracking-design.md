# Presentation Time Tracking (Rabbit and Turtle) Design Document

Date: 2026-07-03
Status: Design finalized (the method for specifying time follows the author's instruction, "make it zero-config, and write the setting in Markdown frontmatter. It would be good if presentation time could be set there," 2026-07-03)

## Goal

Add time management functionality equivalent to slidev-rabbit-turtle (https://zenn.dev/kaakaa/articles/slidev-rabbit-turtle).

- **Rabbit**: a marker showing slide progress (current index / final index)
- **Turtle**: a marker showing time progress (elapsed time / planned presentation time)
- Move both along the same track to the right; if the rabbit is ahead of the turtle, you're on schedule at a glance, and if the turtle is ahead, you're pacing over time

**Display destination requirement (user-specified)**: other tools display this on the presentation screen, but peitho displays it **on the presenter screen**. Only when there is a single screen with no presenter screen does it display **on the presentation screen**.

## Decisions

### D1: The planned presentation time is specified in YAML frontmatter at the top of the deck (author's instruction)

```markdown
---
time: 15m
---

# First slide
```

- This is the first instance of a **general mechanism for deck-level settings**. Under the zero-config policy (not creating separate files like peitho.toml), the deck itself carries its settings. `time` is the first key
- Accepted forms: `15m` / `90s` / `1h` / `1h30m` (string), or a bare integer `15` (interpreted as minutes, the same feel as slidev's `?time=10`)
- If `time` is unspecified, or frontmatter itself is absent → no time tracking display (as before). Existing decks work unchanged
- No CLI flag will be added (zero-config. If an override becomes necessary, like a 15-minute/30-minute version of the same deck, that's a separate Issue)

**Errors (silent dropping is strictly forbidden)**:
- Unknown key → build error with line number + help via `deny_unknown_fields` (same convention as PageComment)
- Invalid time value (`0`, negative, unit-less string `abc`, empty) → build error with line number + help
- Malformed YAML → build error with line number + help

### D2: Frontmatter parsing is lexed via pulldown-cmark's metadata block

Since slide separators are also `---` (thematic break), distinguishing them from frontmatter's `---` is the core challenge. Rather than custom string preprocessing, add `Options::ENABLE_YAML_STYLE_METADATA_BLOCKS` to `parser_options()`, letting pulldown-cmark 0.10 (existing dependency) tokenize the YAML block at the top of the document as `Tag::MetadataBlock`.

- In CommonMark terms, frontmatter is **only at the top of the document**. Any `---` from the second slide onward remains a slide separator as before. The mishap where the `---` right after `time: 15m` turns into a setext heading is also eliminated at the lexical level
- `split_slide_ranges` captures the `MetadataBlock` events (Start/Text/End) to extract the raw YAML and line numbers, and slide ranges start after the block's end
- On the `parse_slide` side, encountering a `MetadataBlock` event is an explicit error (frontmatter anywhere but the top; not swallowed by `_ => {}`)
- The YAML body is deserialized into a new `DeckFrontmatter` (serde, `deny_unknown_fields`). The YAML crate is a serde-compatible one that's still actively maintained (serde_norway), added as a workspace dependency
- The `time` value is interpreted via a custom Deserialize into a dedicated type (e.g., `PlannedTime`), centralizing handling of both string/integer forms and invalid-value errors at the type's construction point (not scattering validation across consumers)
- The parse result rides on `Deck<Parsed>` as deck configuration, and is carried forward through subsequent phases (Mapped/Checked/Rendered)

### D3: Wiring to the frontend is via manifest.json (planned time) + present.json (display destination)

**The planned time is carried on the Manifest**. Since it's deck metadata originating from frontmatter, the manifest that describes the deck is the natural carrier. The shell already fetches manifest.json.

```rust
// crates/peitho-core/src/manifest.rs (add field to existing struct)
pub struct Manifest {
    // existing: version, peitho_version, title, slide_count, slides
    pub planned_duration_ms: Option<u64>,  // None if frontmatter time is unspecified
}
```

**The material for deciding the display destination is carried on present.json**. "Whether to open the presenter window" is runtime knowledge at launch time, not deck metadata, so it's not mixed into the manifest.

```rust
// crates/peitho-core/src/present_config.rs (new, same pattern as manifest.rs)
pub struct PresentConfig {
    pub version: u32,
    pub presenter_open: bool,
}
```

- Both generate and commit `bindings/*.ts` via ts-rs (single-source-of-contract principle, CI drift check)
- present.json is always written out by `emit_present_cache`. It is **present-cache only** and not included in the distributable dist/ (non-contamination invariant). manifest.json continues to go into dist/ as before; since `plannedDurationMs` is merely deck metadata and not part of the presentation shell, it doesn't violate the non-contamination condition

Rejected alternatives:
- **CLI flag `--time`** (initial design): changed to the frontmatter approach per the author's instruction
- **Also carrying the planned time on present.json**: since time is a value originating from the deck, the manifest is authoritative. present.json is limited to runtime configuration only
- **Embedding JSON into the entry HTML**: this would become string assembly and fall outside the type contract

### D4: The display destination decision is finalized by the CLI at launch time (`presenter_open`)

`presenter_open = !no_open && !no_presenter && display layout detection is Some`

- 2 displays (presenter window present) → `true` → tracker shows **only on the presenter screen**
- 1 display + default Fullscreen (no presenter window) → `false` → tracker shows **on the presentation screen**
- `--presenter-windowed` (1-screen debugging, both windows) → `true` → presenter screen only
- `--no-presenter` → `false` → presentation screen
- Implementation-wise, the layout detection inside `present()` runs once, before `emit_present_cache`, so both the config write-out and the browser launch use the same detection result

Frontend display rules:
- presenter.html: show the tracker if `manifest.plannedDurationMs != null`
- present.html: show the tracker if `manifest.plannedDurationMs != null && !config.presenterOpen`

**Edge case (conservative decision)**: with `--no-open`, detection doesn't run, so `presenter_open=false`; if the user then manually opens the presenter screen too, it can end up displayed on both screens. Also, if the presenter screen is opened later via the "Presenter" button on the control bar after launch, the display on the presentation screen side doesn't disappear either. Both are accepted as consequences of the simple model "placement is finalized at launch time" (dynamic presenter-connection detection would require adding a role concept to the /sync protocol, which is excessive; a separate Issue if it becomes necessary).

### D5: The tracker is a shell-layer UI component (conforms to the §16 event contract)

Implement `installTimeTracker(options)` in the new `packages/peitho-present/src/timeTracker.ts`.

- **Read-only**: update the rabbit's position on the `peitho:slidechange` event; update the turtle's position via `setInterval` (250ms, same as the presenter timer) reading `shell.elapsedMs()`
- **Only issues request events**: automatic timer start (D6) is done by dispatching `peitho:timercontrol {action:"start"}`. The shell remains the executor of transitions and the timer
- Never touches the slide body itself (layout HTML / theme CSS). The overlay is shell DOM
- Return value is a cleanup function (per the existing convention for guarding against listener contamination in vitest)

Position calculation:
- Rabbit: `index / (total - 1)` (right edge on the final slide). Fixed at 0% when `total <= 1` (division-by-zero guard)
- Turtle: `min(elapsedMs / plannedDurationMs, 1)`. Pins to the right edge on overrun, and adds an overrun state attribute (`data-peitho-overrun`) to the tracker to turn it a warning color

### D6: Automatic timer start

When `time` is set, on the **first forward navigation** (`peitho:slidechange` where `previousIndex !== null && index > previousIndex`), the tracker dispatches `peitho:timercontrol start`.

- `startPresentation()` does nothing if already started (existing implementation is idempotent), so it doesn't conflict with a manual Start on the presenter screen
- Manual Start/Pause/Resume/Reset on the presenter screen all remain fully functional as before
- Rationale: in single-screen operation with only the presentation screen, there is no Start button, and without auto-start the turtle would stay at 0% forever. Treating "the moment the slide advances" as "the presentation has begun" is the most natural interpretation

### D7: Appearance

- **Track**: a thin bar at the bottom edge of the screen (about 6px tall and semi-transparent on the presentation screen so it doesn't interfere with the slide; shown somewhat larger inside the sidebar on the presenter screen)
- **Markers**: 🐰 and 🐢 emoji (no assets needed, an homage to rabbit-turtle). Rabbit offset slightly to the upper row and turtle to the lower row so they remain distinguishable even when overlapping
- **Presenter screen numeric display**: the existing timer `MM:SS` is extended to `MM:SS / MM:SS` (elapsed/planned) when `time` is set, and on overrun the excess is shown alongside as `+MM:SS` in a warning color
- CSS is added to the `<style>` block in the entry HTML (render.rs), per existing convention. Theme CSS (themes/) is not touched (design separation)
- The tracker is not shown in the dist/ viewer (this is a presentation-time feature)

## List of changed files

| File | Change |
|---|---|
| `Cargo.toml` (workspace) | Add YAML crate (serde_norway) |
| `crates/peitho-core/src/parser.rs` | Enable `ENABLE_YAML_STYLE_METADATA_BLOCKS`, `DeckFrontmatter`+`PlannedTime`, metadata block capture in `split_slide_ranges`, explicit error in `parse_slide` for non-leading frontmatter |
| `crates/peitho-core/src/phase.rs` | Carry deck configuration on `Deck<Parsed>` and later phases |
| `crates/peitho-core/src/manifest.rs` | Add `Manifest.planned_duration_ms` |
| `crates/peitho-core/src/present_config.rs` | New: `PresentConfig` + JSON serialization + ts-rs export + tests |
| `crates/peitho-core/src/lib.rs` | Expose module |
| `bindings/Manifest.ts` / `bindings/PresentConfig.ts` | ts-rs generated (commit) |
| `crates/peitho/src/main.rs` | Move layout detection in `present()` earlier + write out present.json via `emit_present_cache` |
| `crates/peitho-core/src/render.rs` | present.html/presenter.html entry scripts fetch manifest/present.json → wire up tracker, add CSS |
| `packages/peitho-present/src/timeTracker.ts` | New: `installTimeTracker` |
| `packages/peitho-present/src/presenter.ts` | Extend timer display (elapsed/planned), install tracker |
| `packages/peitho-present/src/index.ts` | Add export |
| `packages/peitho-present/dist/shell.js` | Rebuild and commit (drift check) |
| `CLAUDE.md` | Record of author's decision: zero-config + frontmatter configuration policy (2026-07-03), update the peitho.toml premise on the §18 pending list |
| Tests | vitest (timeTracker unit, presenter integration), Rust (frontmatter parsing, PlannedTime, manifest, present_config, presenter_open determination) |

## Test policy

- **Frontmatter parsing**: `time: 15m`/`time: 90s`/`time: 1h30m`/`time: 15` (integer minutes)/no frontmatter/empty frontmatter/unknown key (error + line number + help)/invalid time value (`0`, negative, `abc`, empty; error + help)/malformed YAML (error)/**`---` from the second slide onward still functions as a separator as before**/metadata block anywhere but the top is an error
- **Manifest/PresentConfig**: serde round-trip, camelCase field names, ts-rs drift (same pattern as the existing `ts_tests`)
- **timeTracker (vitest)**: rabbit position (first/middle/last/single-slide deck), turtle position (0%/50%/overrun pinning + overrun attribute), auto-start dispatch (fires on forward, does not fire on backward, does not fire twice), no leftover listeners after cleanup
- **presenter integration**: with `time` → `MM:SS / MM:SS` display, without → display as before
- **E2E (real browser required)**: confirm 1 screen (tracker at the bottom of the presentation screen) / `--presenter-windowed` (shown only on the presenter screen, not on the presentation screen) / no `time` (shown nowhere), using `--port` fixed + `curl POST /sync` + `screencapture`

## Things that must not be broken (self-check)

- Pillar 1: the time setting is deck metadata (frontmatter), not design. Don't mix it into layout/theme
- Pillar 3: don't silently swallow unknown keys, invalid values, or positional violations in frontmatter. `_ => {}` is forbidden
- §16: the tracker only issues request events and reads state. The shell remains the executor
- typestate: deck configuration is finalized at `Parsed` and carried through subsequent phases (don't create a lookup-failure path further down the pipeline)
- dist/ non-contamination: present.json is present-cache only
- Single source of contract: Manifest/PresentConfig are authoritative in Rust, TS is ts-rs generated
- Don't change the behavior of the existing presenter timer (Start/Pause/Resume/Reset)
- Existing decks/examples without frontmatter must build unchanged
