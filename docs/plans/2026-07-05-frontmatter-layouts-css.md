# Deck frontmatter for layouts / css / syntaxes (Issue #62)

Date: 2026-07-05
Status: Design finalized

## Goal

Move every deck-intrinsic asset location from the CLI into the deck's YAML frontmatter, so the deck file alone determines what gets built. `--layouts` and `--css` are removed from the CLI; `syntaxes` (previously auto-detect only, no flag) also gains a frontmatter key. CLI options remain only for invocation/runtime concerns.

```markdown
---
time: 15m
layouts: ./layouts
css: ./css
syntaxes: ./syntaxes
---
```

## Author decisions (2026-07-05)

1. **`--shell` stays a CLI-only dev/debug flag.** It swaps out runtime machinery (the presentation shell bundle), not deck content — a different `--shell` must not change the built output. So it does not belong on the frontmatter side.
2. **`syntaxes` is included in this Issue.** Users may want to share a common highlighter set across multiple decks; frontmatter is the natural pivot. Even though `syntaxes` currently has no CLI flag (only deck-adjacent auto-detect), we add it symmetrically alongside `layouts:` / `css:` so all three assets follow the same resolution chain.
3. **The chicken-and-egg loop is resolved by ordering alone — `Highlighter` needs no split.** Measured fact (highlight.rs:16–18): `Highlighter` holds *only* a `SyntaxSet`; there is no theme inside it (colors come from theme CSS per the `hl-*` class invariant). So the original idea of "parse takes a SyntaxSet, render builds the full Highlighter" would split a type that has nothing to split. Instead, the CLI parses frontmatter first (which needs no highlighter), resolves `syntaxes:`, constructs the one `Highlighter` exactly as today (`defaults()` / `with_user_dir`), and only then calls `parse_markdown`. `parse_markdown` keeps taking `&Highlighter`. The user's motivation — sharing a common highlighter set across decks — is served by the `syntaxes:` frontmatter key, not by an API split.
4. **Language-tag validation stays at parse time, verbatim.** The CLAUDE.md invariant "Unknown language tags are a parse-time error with a line number" is preserved; `Highlighter::validate_language` (highlight.rs:46–56) is untouched, including its help string (relevant to open Issue #105, which stays independent).
5. **Frontmatter is parsed exactly once, and the ordering is enforced by the type system.** A new `parse_frontmatter(source) -> Result<ParsedFrontmatter>` is the only way to obtain a `ParsedFrontmatter` (private constructor), and `parse_markdown(source, frontmatter, &highlighter)` *requires* one. A caller cannot skip the frontmatter step or hand in home-made settings — the resolver step is required by the type, not by convention.

## Principle

- **Frontmatter**: inputs that change the *built output* of the deck. Same deck file → same output regardless of invocation.
- **CLI**: options about *this invocation/session* (where to serve, whether to open a browser, watch mode, output location, dev-only shell swap).

## Disposition of every current option

| Option | Command | Classification | Disposition |
|---|---|---|---|
| `--layouts` | build/present | Deck-intrinsic | → frontmatter `layouts:`, remove flag |
| `--css` | build/present | Deck-intrinsic | → frontmatter `css:`, remove flag |
| (deck-adjacent `syntaxes/`) | build/present | Deck-intrinsic | + frontmatter `syntaxes:` (auto-detect stays as fallback) |
| `--shell` | present | Dev/debug override | Keep CLI (author call) |
| `--out` | build | Invocation | Keep CLI |
| `--watch` | build | Session behavior | Keep CLI |
| `--port` | present | Session/serving | Keep CLI |
| `--no-open` / `--no-serve` | present | Session behavior | Keep CLI |
| `--no-presenter` / `--presenter-windowed` | present | Session behavior | Keep CLI |
| `--dist` | publish | Invocation | Keep CLI |
| frontmatter `time` | (already done) | Deck-intrinsic | — |

## Resolution chain (per asset, unchanged in spirit)

For each of `layouts`, `css`, `syntaxes`:

1. **Frontmatter key** (`layouts:` / `css:` / `syntaxes:`) — if present, wins.
2. **Deck-adjacent auto-detect** (`<deck-parent>/layouts/`, `.../css/`, `.../syntaxes/`) — if the directory exists, use it.
3. **Built-in default** (embedded via `include_str!`; syntaxes falls back to `Highlighter::defaults()`).

Values are strings; a plain relative path resolves against the deck file's parent directory (not cwd — same reference point as auto-detect). Absolute paths are allowed as-is. A frontmatter value that points to a non-existent path is a **line-numbered build error with help** (not a silent fallback to auto-detect, matching the existing zero-config rule: "if you wrote it, you meant it").

File-or-directory semantics: unchanged. A path may be a single file (`.html` for layouts, `.css` for css, `.sublime-syntax` for syntaxes) or a directory whose matching-extension files are loaded in filename order.

## Type changes

### `DeckFrontmatter` (parser.rs:36–41)

Add three optional fields:

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFrontmatter {
    #[serde(default, deserialize_with = "deserialize_optional_planned_time")]
    time: Option<PlannedTime>,
    #[serde(default)]
    layouts: Option<AssetPath>,
    #[serde(default)]
    css: Option<AssetPath>,
    #[serde(default)]
    syntaxes: Option<AssetPath>,
}
```

`AssetPath` is a newtype around `PathBuf` with a custom `Deserialize` that:
- Rejects empty strings with a line-numbered error and help.
- Keeps the raw string (does not resolve against deck-parent) — resolution happens in the CLI where the deck path is known. This preserves the `peitho-core` invariant that pipeline code is filesystem-location-agnostic.

### `DeckSettings` (phase.rs:70–107)

Add three fields, all `Option<AssetPath>`:

```rust
pub struct DeckSettings {
    planned_time: Option<PlannedTime>,
    sections: Vec<DeckSection>,
    layouts: Option<AssetPath>,
    css: Option<AssetPath>,
    syntaxes: Option<AssetPath>,
}
```

Add public accessors `layouts() -> Option<&AssetPath>`, `css() -> Option<&AssetPath>`, `syntaxes() -> Option<&AssetPath>`. Extend `DeckSettings::new` accordingly. Same "carried through every phase" pattern as `planned_time`.

### `ParsedFrontmatter` — the typed seam (parser.rs, new)

```rust
pub struct ParsedFrontmatter {
    settings: DeckSettings,               // planned_time + asset keys, sections empty
    body_start: usize,                    // byte offset where slides begin
    key_lines: HashMap<&'static str, usize>, // frontmatter line of each asset key, for CLI error reporting
    // no public constructor — parse_frontmatter is the only producer
}

pub fn parse_frontmatter(source: &str) -> Result<ParsedFrontmatter>
```

- Extracts and validates just the frontmatter via the existing `leading_frontmatter` + `parse_deck_frontmatter` core (both already present; refactored to be shared).
- No frontmatter / blank frontmatter → `ParsedFrontmatter` with `DeckSettings::default()` and `body_start = 0` (or past leading whitespace) — same semantics as today.
- Accessors: `settings()`, `layouts()`, `css()`, `syntaxes()`, and `key_line(key) -> Option<usize>` so the CLI can report "layouts path does not exist" at the right frontmatter line.
- `body_start` is `pub(crate)`: `parse_markdown` slices from it, so byte offsets have a single source of truth — no re-detection, no `debug_assert`, no drift possible.
- `key_lines` lives here (a parse-time product) and **not** on `DeckSettings`, which rides through every phase to `Rendered` and must not carry CLI-error-reporting metadata.

### `parse_markdown` signature change (parser.rs:106)

Before:

```rust
pub fn parse_markdown(source: &str, highlighter: &Highlighter) -> Result<Deck<Parsed>>
```

After:

```rust
pub fn parse_markdown(
    source: &str,
    frontmatter: ParsedFrontmatter,
    highlighter: &Highlighter,
) -> Result<Deck<Parsed>>
```

- Requiring `ParsedFrontmatter` (only obtainable from `parse_frontmatter`) makes the parse-frontmatter-first ordering a compile-time fact, not a calling convention. Frontmatter is deserialized exactly once.
- `parse_markdown` slices slides starting at `frontmatter.body_start` and seeds `DeckSettings` from `frontmatter.settings` (then applies sections via the existing `finalize_section_settings` flow).
- `highlighter` stays: `Highlighter` is already just a `SyntaxSet` wrapper (no theme — measured), so there is nothing to slim down. `validate_language` keeps working unchanged at parse time.

## CLI change (crates/peitho/src/main.rs)

### Options structs (main.rs:27–33, 108–118)

Remove the `layouts` and `css` fields from `BuildOptions` and `PresentOptions`. Remove `effective_layouts()` / `effective_css()` helper methods (auto-detect is now done inside a shared resolver that also consults frontmatter).

### Clap declarations (main.rs:131–147, 148–172)

Remove `#[arg(long)] layouts: Option<PathBuf>` and `css: Option<PathBuf>` from both `Build` and `Present` subcommands. Update help text accordingly.

### New resolver (crates/peitho/src/asset_resolution.rs — new file)

```rust
pub struct ResolvedAssets {
    pub layouts: Option<PathBuf>,   // absolute
    pub css: Option<PathBuf>,       // absolute
    pub syntaxes: Option<PathBuf>,  // absolute
}

pub fn resolve_assets(
    deck: &Path,
    frontmatter: &ParsedFrontmatter,
) -> miette::Result<ResolvedAssets>
```

Resolution order per asset, matching the chain above:
1. `frontmatter.layouts()` — resolve against `deck.parent()` (plain join; absolute paths pass through). If the path does not exist, error with the frontmatter line from `frontmatter.key_line("layouts")`: `"layouts path does not exist: {path}"` + help.
2. Deck-adjacent `<deck>/layouts/` — if the directory exists, use it.
3. `None` (falls back to built-in defaults inside `load_layouts`).

The frontmatter line of each key comes from `ParsedFrontmatter::key_lines`, filled by `parse_deck_frontmatter` walking `raw.yaml` for `^layouts:` / `^css:` / `^syntaxes:` and mapping to `raw.line + within_offset`. The walk is reliable because `validate_frontmatter_lines` already guarantees every body line is a flat `key:` line. (Rejected alternative: keeping a serde_norway `Value` around — adds dependency surface and drift risk.)

### build_artifacts flow (main.rs:450–487)

New shape (signature shrinks to just `input` — the deck now carries everything):

```rust
fn build_artifacts(input: &Path) -> miette::Result<BuildArtifacts> {
    let markdown = fs::read_to_string(input).into_diagnostic()?;
    let frontmatter = core(peitho_core::parse_frontmatter(&markdown))?;
    let assets = resolve_assets(input, &frontmatter)?;
    let highlighter = match assets.syntaxes.as_deref() {
        Some(dir) => core(peitho_core::highlight::Highlighter::with_user_dir(dir))?,
        None => peitho_core::highlight::Highlighter::defaults(),
    };
    let layouts = load_layouts(assets.layouts.as_deref())?;
    let css_files = load_css(assets.css.as_deref())?;
    let parsed = core(peitho_core::parse_markdown(&markdown, frontmatter, &highlighter))?;
    let mapped = core(peitho_core::dispatch_by_convention(parsed, &layouts))?;
    let checked = core(peitho_core::check_deck(mapped))?;
    let slide_count = checked.slide_count();
    let css = core(peitho_core::build_theme_css(&css_files, ...))?;
    let mut image_resolver = ImageResolver::new(input);
    let (resolved, image_assets) = core(peitho_core::resolve_image_paths(checked, |r| image_resolver.resolve(r)))?;
    let manifest = peitho_core::build_manifest(&resolved, &image_assets);
    let manifest_json = core(peitho_core::manifest_json(&manifest))?;
    let rendered = core(peitho_core::render_deck(resolved, &highlighter))?;
    Ok(BuildArtifacts { slide_count, rendered, manifest_json, css, image_assets })
}
```

No test-override parameters: the integration tests invoke the compiled binary via `assert_cmd` (measured — tests/build.rs uses `Command::cargo_bin("peitho")`), so function-level overrides would serve nobody. Tests migrate to writing frontmatter keys (or deck-adjacent dirs) into their fixture decks.

### present() flow (main.rs:723–735)

Same restructuring: parse frontmatter → resolve assets → construct highlighter → parse → …

### Watch targets (main.rs:65–105)

`BuildOptions::watch_roots` currently relies on `effective_layouts/css/syntaxes`. After the change:
- Parse frontmatter once at watch startup, then compute `ResolvedAssets`.
- Watch the deck file plus every resolved asset path.
- If the deck's frontmatter changes such that `layouts:` moves to a different directory, watch restart is required (documented, matches the existing acceptance from the zero-config plan). Detect this by re-parsing frontmatter after a deck-md change and comparing to the cached `ResolvedAssets`; if they differ, print a hint asking the user to restart `--watch`. (Alternative: automatic restart is out of scope — file a follow-up if needed.)

## Error messages (samples)

- Non-existent frontmatter path:
  ```
  error: layouts path does not exist: ./layouts-nope
    → deck.md:3:1
    help: check the layouts: value in the frontmatter, or remove the key to use deck-adjacent layouts/
  ```
- Empty string:
  ```
  error: layouts value is empty
    → deck.md:3:1
    help: provide a path (relative to the deck file), or remove the layouts: key
  ```
- Directory contains no `*.html` (existing `collect_asset_files` error, unchanged).

## Help-string update in `frontmatter_help`

`parser.rs:476–491` currently branches on message substrings for the single supported key. Update:
- The "unknown field" branch's help message goes from `"use only the supported deck frontmatter key: time"` to a list of the four supported keys.
- Add per-key hints (`layouts`, `css`, `syntaxes`) that describe accepted forms.

## Test plan

Following the TDD Iron Law: every production change lands via Red → Green → Refactor.

### peitho-core unit tests (crates/peitho-core/src/parser.rs)

New / modified tests:

1. `parses_frontmatter_layouts_key_carries_to_settings`
2. `parses_frontmatter_css_key_carries_to_settings`
3. `parses_frontmatter_syntaxes_key_carries_to_settings`
4. `rejects_empty_layouts_string_with_line_and_help`
5. `rejects_empty_css_string_with_line_and_help`
6. `rejects_empty_syntaxes_string_with_line_and_help`
7. `unknown_key_help_mentions_all_supported_keys`
8. `frontmatter_settings_survive_all_typestate_transitions` (extend the existing test)
9. `parse_frontmatter_returns_default_settings_when_no_frontmatter`
10. `parse_frontmatter_returns_default_settings_when_frontmatter_is_empty`
11. `parse_markdown_slices_body_from_parsed_frontmatter_offset` (frontmatter is not re-deserialized; slides start at `body_start`)
12. `parse_frontmatter_records_key_lines_for_asset_keys`
13. Existing frontmatter error tests (unknown key, invalid time, duplicate key, broken YAML, line-shape whitelist, leading `---` guard) keep passing through the new `parse_frontmatter` path — they now pin the shared core

### peitho-core render tests

Extend `deck_settings_survive_all_typestate_transitions` (render.rs:837–859) to include `layouts`, `css`, `syntaxes` in the assertion.

### CLI integration tests (crates/peitho/tests/build.rs, present.rs, publish.rs)

- **Migration**: every existing test that passes `--layouts` / `--css` writes those paths into the deck's frontmatter instead. Approximately 25 test sites in build.rs, 2 in present.rs, 1 in publish.rs.
- **New**:
  - `build_reads_layouts_from_frontmatter`
  - `build_reads_css_from_frontmatter`
  - `build_reads_syntaxes_from_frontmatter`
  - `build_frontmatter_layouts_overrides_deck_adjacent`
  - `build_frontmatter_non_existent_path_errors_with_line_and_help`
  - `build_zero_config_still_uses_deck_adjacent_dirs` (regression guard, existing test at build.rs:200–235 should pass unchanged)
  - `present_reads_layouts_from_frontmatter`
  - CLI no longer accepts `--layouts` / `--css` (assert error message)

### E2E manual check

- `examples/keynote/deck.md` with `layouts:` / `css:` in frontmatter → `peitho present` → both windows render as before.
- `examples/lightning-talk/deck.md` (no keys) → zero-config still works.

## Documentation updates

- `README.md:70` — while touching, also fix the stale `--layout` / `--base-css` / `--overrides-css` references (pre-existing bug noted during investigation).
- `README.md:117` — rewrite the "Multiple layouts" opener to describe the `layouts:` frontmatter key. Example deck.
- `CLAUDE.md:15` — remove "Migrating --layouts/--css into frontmatter is Issue #62"; describe the finalized frontmatter keys and resolution chain.
- `CLAUDE.md:31` — update to describe frontmatter keys + auto-detect fallback + `--shell` remaining as a dev-only flag.
- New: `examples/` deck demonstrating `layouts:` / `css:` / `syntaxes:` in frontmatter (optional; if included, gets its own Makefile target).

## List of changed files

| File | Change |
|---|---|
| `crates/peitho-core/src/parser.rs` | Add `layouts`/`css`/`syntaxes` to `DeckFrontmatter`; `AssetPath` newtype; `ParsedFrontmatter` (private ctor, `body_start`, `key_lines`); new `parse_frontmatter` public fn; `parse_markdown` signature change to `(source, ParsedFrontmatter, &Highlighter)`; extend `frontmatter_help` |
| `crates/peitho-core/src/phase.rs` | Extend `DeckSettings` with `layouts`/`css`/`syntaxes`; accessors |
| `crates/peitho-core/src/highlight.rs` | Unchanged (`Highlighter` is already a plain `SyntaxSet` wrapper; only its construction site in the CLI moves) |
| `crates/peitho-core/src/lib.rs` | Expose `parse_frontmatter`, `ParsedFrontmatter`, `AssetPath` |
| `crates/peitho-core/src/{mapping,check,render}.rs` | Ensure new settings fields ride through phase transitions (mostly automatic via `DeckSettings` opaque handling) |
| `crates/peitho/src/main.rs` | Remove `--layouts`/`--css` from clap; drop `layouts`/`css` from `BuildOptions`/`PresentOptions`; rewrite `build_artifacts` and `present()` to parse frontmatter first, then resolve; watch targets refactor |
| `crates/peitho/src/asset_resolution.rs` (new) | `resolve_assets(deck, settings) -> ResolvedAssets` |
| `crates/peitho/tests/build.rs` | Migrate ~25 test sites to use frontmatter; add new tests |
| `crates/peitho/tests/present.rs` | Migrate 2 test sites; add new test |
| `crates/peitho/tests/publish.rs` | Migrate 1 test site |
| `README.md` | Rewrite `--layouts`/`--css` section + fix stale flag names |
| `CLAUDE.md` | Update frontmatter description; remove Issue #62 pending note |
| Examples (optional) | Add a deck showing frontmatter asset keys |

(`bindings/` is untouched: `DeckSettings` is not a ts-rs-exported type — verified.)

## Things that must not be broken (self-check)

- Pillar 1 (content ↔ design separation): asset locations are metadata, not content or design; frontmatter is the right home.
- Pillar 3 (no silent drops): every invalid frontmatter value produces a line-numbered error with help. `deny_unknown_fields` catches typos.
- Typestate: `DeckSettings` is complete at `Parsed` and carried through every phase; asset paths are never re-looked-up downstream.
- Language-tag validation stays at parse time; unknown tags remain parse-time errors with line numbers (CLAUDE.md invariant).
- Zero-config still works: a deck with no asset keys and adjacent `layouts/`/`css/` builds identically to today.
- Existing decks without any of the new keys build unchanged (`Option::default()` everywhere).
- Publish contamination check unchanged; publish never sees frontmatter.
- `--shell` stays a CLI flag (dev/debug only).

## Long-term view + type safety self-check

- **New caller tomorrow**: someone adding a new asset (e.g. `themes:`) follows the same three-key pattern in `DeckFrontmatter` + `DeckSettings` + `resolve_assets`. The `AssetPath` newtype prevents arbitrary raw strings from leaking into path handling.
- **Type-permits-broken-state?** `Option<AssetPath>` cannot be "empty string" (custom Deserialize forbids it). A raw `PathBuf` inside `AssetPath` is fine because resolution happens at exactly one place (`resolve_assets`), which is a total function on `(deck, frontmatter)`.
- **Ordering enforced by types**: `parse_markdown` requires a `ParsedFrontmatter`, which only `parse_frontmatter` can produce — a future caller cannot forget the frontmatter step or feed the parser settings from a different source. No `debug_assert`, no convention.
- **Sibling code paths**: `resolve_assets` is called in both `build_artifacts` and `present()`; there is one function, not two open-coded copies — no drift risk.
