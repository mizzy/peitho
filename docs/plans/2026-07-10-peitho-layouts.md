# `peitho layouts` — inspect layout contracts and explain dispatch

Issue: #243. Adds a CLI subcommand that surfaces peitho's layout schema
(the third pillar: "the layout **is** the schema") so authors can
inspect slot contracts and understand why a slide dispatched to a given
layout.

## Design decisions (locked with the author 2026-07-10)

- **Positional arg**: `deck.md` (defaults to `deck.md`, matches `build` /
  `preview` / `present` / `export pdf`). Frontmatter drives layout
  resolution, so a deck path is the natural input.
- **`--explain <slide-key>`**: `SlideKey` only. Keys are already the
  public slide identifier used in `manifest.json` and `notes.json`;
  index-based addressing would drift when a deck is edited.
- **Output**: human-first by default, `--json` for programmatic use.
- **Provenance**: report which asset resolution branch was taken
  (`explicit frontmatter <path>` / `deck-adjacent <path>` / `built-in`)
  at the top of both modes. This is half the value per the Issue.

## Non-goals

- No `--watch` (this is a one-shot introspection command, not a build).
- No editing / fixing of dispatch failures — this only reports.
- No new deck-level frontmatter keys; the command reads what is already
  parsed.
- No changes to the dispatch algorithm itself. This subcommand only
  observes what `dispatch_by_convention` decides.

## User surface

### `peitho layouts [<deck.md>] [--json]`

Human-readable output (one layout per block, layouts in CLI order):

```
layouts source: deck-adjacent (./layouts)

title-body-code
  slots:
    - title  accepts=inline  arity=1
    - body   accepts=blocks  arity=0..*
    - code   accepts=code    arity=0..1

statement
  slots:
    - title  accepts=inline  arity=1
    - body   accepts=blocks  arity=1..*
```

`--json`:

```json
{
  "source": { "kind": "deck-adjacent", "path": "./layouts" },
  "layouts": [
    {
      "name": "title-body-code",
      "slots": [
        { "name": "title", "accepts": "inline", "arity": "1" },
        { "name": "body",  "accepts": "blocks", "arity": "0..*" },
        { "name": "code",  "accepts": "code",   "arity": "0..1" }
      ]
    }
  ]
}
```

- `source.kind` is one of `explicit`, `deck-adjacent`, `built-in`.
- `source.path` is present for `explicit` / `deck-adjacent`, absent for
  `built-in`.
- Slots preserve `BTreeMap` iteration order (alphabetical by slot
  name), matching how `Layouts::slot_classes()` composes them.

### `peitho layouts [<deck.md>] --explain <slide-key> [--json]`

Runs the deck through parse → frontmatter → layouts, locates the slide
by `SlideKey`, and reports the dispatch decision.

Three trace shapes, matching `dispatch_slide`'s three paths:

1. **Explicit layout request** (`{"layout":"name"}` in a page comment)
2. **Sole layout** (only one layout available)
3. **Structural match** (probed against each layout in CLI order)

Human-readable output (structural match example):

```
layouts source: deck-adjacent (./layouts)
slide: intro (index 2)

dispatch: structural match
  candidates:
    - cover        rejected: too many body fragments
    - title-body-code  matched
    - statement    rejected: title slot required but not present
  result: title-body-code
```

Explicit and sole-layout no-match failures also print a `reason:` line
with the underlying map error; JSON includes the same text in the
`reason` field for those no-match results.

`--json` mirrors the trace as a discriminated `kind` union:

```json
{
  "source": { "kind": "deck-adjacent", "path": "./layouts" },
  "slide": { "key": "intro", "index": 2 },
  "dispatch": {
    "kind": "structural-match",
    "candidates": [
      { "layout": "cover", "outcome": "rejected", "reason": "…" },
      { "layout": "title-body-code", "outcome": "matched" },
      { "layout": "statement", "outcome": "rejected", "reason": "…" }
    ],
    "result": "title-body-code"
  }
}
```

Failure modes for `--explain`:

- **Unknown slide key** → exit 2, human error message `slide key
  '<key>' not found in <deck.md>` + `help: known keys: intro, agenda,
  …`. Uses the standard `BuildError`-style formatting to stay
  consistent with build errors.
- **Dispatch failure inside the slide** → the command *does not fail
  the build*: it prints the failure trace (candidates + reasons) and
  exits with a non-zero code so scripts can detect it. Unlike `build`,
  which stops at the first slide error, `layouts --explain` is a
  diagnostic tool — the whole reason to run it is when dispatch is
  broken.

### `peitho layouts --explain <key>` without a matching layout at all

If layout resolution itself failed (e.g. `layouts:` in frontmatter
points at a non-existent path), the command surfaces the same
line-numbered error as `build` and exits non-zero. Provenance is
"unknown" only when we reach here — never a silent fallback to
built-in.

## Implementation shape

### 1. New public helpers in `peitho-core`

- `layout::LayoutSummary`, `layout::SlotSummary` — plain-old-data
  structs holding the fields the CLI needs to print / serialize. These
  are derived from `Layout` / `SlotContract`, so the domain types stay
  unchanged.
- `layout::describe_layouts(layouts: &Layouts) -> Vec<LayoutSummary>` —
  produces the summaries in CLI order.
- `mapping::explain_dispatch(slide: &ParsedSlide, layouts: &Layouts)
  -> DispatchTrace` — returns a structured trace without failing.
  Internally shares code with `dispatch_slide` (both call a common
  `try_dispatch` that produces `(Result<MappedSlide>, Vec<Rejection>)`
  or similar). **This is the key refactor**: rather than copy-pasting
  the dispatch logic into `explain`, split `dispatch_slide` into a
  trace-producing core plus the current thin failure-projection.

  The trace enum:

  ```rust
  pub enum DispatchTrace {
      Explicit { layout: String, line: usize, result: DispatchResult },
      SoleLayout { layout: String, result: DispatchResult },
      StructuralMatch { candidates: Vec<Candidate>, result: DispatchResult },
  }
  pub enum DispatchResult {
      Matched(String),
      NoMatch { reason: Option<String> },
      Ambiguous(Vec<String>),
      UnknownLayout(String),
      // Explicit / SoleLayout failures carry `Some(reason)` from `map_slide`;
      // structural no-match keeps `None` because each candidate has a reason.
  }
  pub struct Candidate {
      pub layout: String,
      pub outcome: CandidateOutcome, // Matched / Rejected { reason }
  }
  ```

- Do **not** re-export `MappedSlide` fields, do **not** touch the
  typestate. The trace holds only names and reasons, not phase-typed
  data.

### 2. Provenance in the CLI

`asset_resolution::resolve_assets` currently returns `Option<PathBuf>`
which conflates "explicit" and "deck-adjacent". Extend `ResolvedAssets`
with a per-asset `Provenance` enum:

```rust
pub enum Provenance {
    Explicit(PathBuf),   // author wrote `layouts: ./…` in frontmatter
    DeckAdjacent(PathBuf), // ./layouts/ exists next to deck.md
    Builtin,             // fell through to the built-in embed
}
```

`resolve_assets` returns `Provenance` for each asset key. `build`
callers only need the path, so add `Provenance::path() -> Option<&Path>`
and change one line each in `build_artifacts` / `WatchTargets::new` /
`preview` to `.path()`. No behavioral change to `build`.

**Type-level rationale**: this replaces the "None means built-in"
convention (currently only documented in `load_layouts` /
`load_css`) with an exhaustive enum. A future caller cannot accidentally
treat `None` as "user asked for nothing" vs. "user asked for built-in"
— the compiler forces them to handle all three branches.

### 3. New CLI subcommand in `crates/peitho/src/main.rs`

```rust
Command::Layouts {
    #[arg(default_value = "deck.md")]
    input: PathBuf,
    #[arg(long)]
    explain: Option<String>,
    #[arg(long)]
    json: bool,
},
```

Handler `cmd_layouts(input, explain, json)`:

1. Read deck → parse frontmatter → resolve assets → load layouts.
2. If `explain.is_none()`: print `LayoutSummary` list. Return.
3. If `explain.is_some(key)`: parse markdown, find the `ParsedSlide`
   whose `SlideKey == key`, call `explain_dispatch`, print the trace.
   Exit non-zero if the trace's result is not `Matched`.

Printing: two functions — `print_layouts_human` /
`print_layouts_json`, `print_explain_human` / `print_explain_json`.
JSON via `serde_json::to_string_pretty`. The serialization structs are
CLI-local (not in peitho-core), so we don't leak serde derives into
the domain types.

### 4. Tests (TDD, delegated to Codex)

Layered from unit → integration → CLI:

- **`peitho-core::layout::describe_layouts`**: given a `Layouts` with three
  layouts, returns three summaries in order with correct slot/arity/accepts
  strings.
- **`peitho-core::mapping::explain_dispatch`**:
  - Explicit-request trace records line + result Matched
  - Explicit-request with unknown name → `UnknownLayout`
  - Explicit + map failure → `NoMatch { reason: Some(_) }` (surface the
    `map_slide` error)
  - Sole-layout trace records `SoleLayout { result: Matched }`
  - SoleLayout + map failure → `NoMatch { reason: Some(_) }` (same)
  - Structural-match with one winner → `StructuralMatch { result: Matched }`
  - Structural-match with zero winners → `StructuralMatch { result: NoMatch }`
    with per-candidate `Rejected { reason }`
  - Structural-match with multiple winners → `StructuralMatch { result:
    Ambiguous(names) }`
- **`asset_resolution::resolve_assets`**: three tests per asset kind
  (explicit / deck-adjacent / built-in) confirming the `Provenance`
  variant.
- **CLI integration** (`crates/peitho/src/main.rs` `#[cfg(test)]`):
  - `layouts deck.md` prints provenance + one layout with the builtin
  - `layouts deck.md --json` produces valid JSON with expected shape
  - `layouts deck.md --explain <key>` on a matching slide exits 0
  - `layouts deck.md --explain <unknown-key>` exits 2 with help
  - `layouts deck.md --explain <key>` on a slide with dispatch failure
    exits non-zero and prints the trace

### 5. Docs

- `README.md` — one-line addition to the "commands" table (if it
  exists; otherwise skip).
- `site/content/guide/**` — a small "Inspecting layouts" section. Skip
  if the guide doesn't have a natural home; the Issue doesn't require
  guide docs and the command is discoverable via `--help`.
- Do **not** add to the docs guide unless there's an obvious slot.

### 6. Gates

Standard peitho gates (CLAUDE.md):

- `cargo test --workspace` 3× consecutively (race guard)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` — no ts-rs changes expected, since
  `LayoutSummary` / `DispatchTrace` stay Rust-side (not exposed to TS)
- Present-shell / preview-shell drift checks (no JS changes expected)

## Risks & mitigations

- **Refactoring `dispatch_slide`**: the current implementation is the
  single source of truth for dispatch behavior. The refactor must not
  change semantics — proven by keeping every existing dispatch test
  passing while adding new trace-producing tests. If any existing test
  breaks, the refactor is wrong.
- **Provenance change in `ResolvedAssets`**: touches `build` / `watch` /
  `preview` call sites. Kept minimal by adding `Provenance::path()` so
  the migration is one call per site. All existing behavior tests must
  still pass.
- **`SlideKey` collisions**: today keys are unique per deck (enforced
  in parser). `--explain <key>` matches on `SlideKey`, so we get
  uniqueness for free.

## Rollout

Single PR (non-draft — peitho convention).

Closes #243.
