# Issue #241: `code_images:` — build-time code-block-to-image conversion

Closes #241.

## Summary

Fenced code blocks whose language tag matches a user-declared entry in
`code_images:` frontmatter are handed to that entry's command via stdin
at build time. The command's stdout (SVG) is hashed, written to the
build output as an asset, and the code block is replaced by an image
that references it.

Name choice: `code_images:` — the mechanism is "fenced code block → image
via a user command." That mechanism is more general than diagrams
(Mermaid / Graphviz / PlantUML are the primary uses, but math via
`katex`, ASCII → SVG via `svgbob`, code screenshots via `silicon`, and
sheet music via `lilypond` all fit the same shape). k1LoW/deck's
`codeBlockToImageCommand` picked the same generality; we compress the
name.

## Three-lens design summary

Every design choice below was evaluated under long-term view / type
safety / root-cause. All four choices landed by unanimous agreement of
the three lenses, so they proceed without a per-choice ask:

| Choice | Long-term | Type safety | Root-cause |
|---|---|---|---|
| Nested mapping (`code_images:` with sub-keys) | Extension shape ready for per-entry options (timeout, output_format) | `HashMap<String, CodeImageCommand>` typed, not string-parsed | Extend `validate_frontmatter_lines` to allow nested mappings uniformly, not a `code_images`-only carve-out |
| `argv[0] argv[1] …` string, `shlex`-split | Same UX as k1LoW/deck; no asymmetry with prior art | shlex split is deterministic and testable; no `sh -c` means no shell expansion surprises | Command is `execve`'d directly; the "authored string is executed" contract stays flat |
| Rendered SVG becomes a `FragmentKind::Image` asset (not a new variant) | dist/ HTML stays lean; publish / preview / PDF export flow through the existing image path | Reuses the `RawImagePath → ResolvedImagePath` typestate; renderer / PDF flatten / publish gain no new match arms → no future carve-out risk | A pre-rendered diagram *is* an image; naming it as one keeps the ontology honest |
| Persistent cache at `.peitho/code-images-cache/` keyed by `SHA256(argv‖0x00‖code_text)` | preview watch stays fast even after restarts | Cache lookup is a pure function of the key; misses trigger the command, hits skip it | Same `.peitho/` sibling as `preview-cache` / `present-cache` |

## Non-goals (unchanged from issue)

- No built-in Mermaid / Graphviz / PlantUML support — users declare
  whatever binary they have installed.
- No client-side JS renderer.
- No shell (`sh -c`) between the command and its argv.

## Phase model

New pipeline step between Parsed and Mapped: **`transform_code_images`**.
It runs after `parse_markdown` produces `Deck<Parsed>` and before
`map_layouts` / check / render.

```
Parsed  ──┐
          ├── transform_code_images ──► Parsed (with Code→Image swaps)
          │
          └── map_layouts ──► Mapped ──► Checked ──► resolve_image_paths ──► Rendered
```

Why *between* Parsed and Mapped, and not inside render:

- **Root-cause**: the substitution changes the *fragment kind* (Code →
  Image). Doing it in render would leave every downstream phase (mapping,
  contract check, image path resolution) seeing a `Code` fragment that
  no consumer of the final HTML actually receives. Mapping's
  convention-mapping-by-kind (`Code → code` slot, `Image → image` slot)
  would route the fragment to the wrong slot.
- **Type safety**: substituting *before* Mapped means the produced
  `FragmentKind::Image { src: RawImagePath }` participates in the same
  `try_map_image_src` walk that `resolve_image_paths` already runs.
  Nothing downstream needs to know that this particular image was
  produced by a `code_images:` command — the type carries it.
- **Long-term**: any future consumer (e.g. an overview picker that
  wants to sample fragment kinds) sees the true kind of what will render.

The pass is invoked from the same call site as `parse_markdown` today
(the CLI's build/preview/publish entry). It takes:
- `Deck<Parsed>` — mutated in place semantically (returned as a new
  `Deck<Parsed>` because the phase-typed `Deck` is immutable in the
  crate's style)
- The `CodeImagesConfig` extracted from `DeckSettings`
- An I/O-facing runner injected as a closure (`Fn(&CodeImageCommand,
  &str) -> Result<Vec<u8>>`) so the pure-code test can substitute a
  fake runner and CI can force determinism

## Data model

### Frontmatter shape

```yaml
---
code_images:
  mermaid: mmdc -i - -o - -e svg
  dot: dot -Tsvg
---
```

Value is currently a bare command string. The type is a struct so
per-entry options can be added later (`timeout`, `output_format`,
`env`) without a breaking schema change:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeImageCommandDe {
    #[serde(deserialize_with = "deserialize_argv_string")]
    command: Vec<String>,
}

impl<'de> Deserialize<'de> for CodeImageCommand {
    // Accept either a bare string (shell-split into argv) or a mapping
    // with `command:` (for future extensibility).
    // Bare-string is v1's only documented form; the mapping shape is
    // reserved for later fields and does not appear in any doc example
    // until at least one such field ships.
}
```

`CodeImagesConfig` on `DeckSettings`:

```rust
pub struct CodeImagesConfig {
    entries: BTreeMap<String, CodeImageCommand>,
    key_line: Option<usize>,
}

pub struct CodeImageCommand {
    argv: Vec<String>,          // shlex-split at parse time
    // Reserved: timeout, output_format, ...
}
```

Storage is `BTreeMap` for deterministic iteration (mirrors the rest of
the pipeline; RawImagePath / SlideKey sorting).

### Frontmatter grammar extension

Today `validate_frontmatter_lines` enforces "flat `key: value` lines
only" via `starts_with_flat_yaml_key`. `code_images:` is the first
nested mapping. The grammar extension is:

- A line is valid if it starts a flat top-level `key:` **OR** it is an
  indented continuation of the most recent top-level key whose value is
  a nested mapping (currently allowed only for `code_images`).
- The allowance is *keyed by top-level key name*, not by depth, so
  arbitrary nesting under unrelated keys still fails. This is a real
  extension, not a carve-out: the next nested-mapping key (e.g. a
  future `sections:` mapping) is added to the same list.
- Indented lines under `code_images:` must themselves be flat
  `sub_key: value` — no 3-level nesting is accepted for now. A 3-level
  nest is a line-numbered error with a `help:` telling the author what
  is supported.

Alternatively, if extending `validate_frontmatter_lines` proves messy,
the fallback is to drop the pre-serde structural check for
`code_images` block only and rely on serde_norway's own errors (which
already carry line numbers via `frontmatter_yaml_error`). Codex should
try the extension first and fall back if the check turns into a special
case that eats the entire function.

## Substitution rules

Given a `SourceFragment` with `kind: FragmentKind::Code` and a
`language: Some(tag)`:

1. If `tag` is in `code_images:` → substitute (see below).
2. Else if `tag` is a syntect-known language → leave alone; render will
   syntax-highlight it.
3. Else → the existing `validate_language` build error fires (unknown
   language tag).

Rule 1 short-circuits rule 3 for tags declared in `code_images:`. The
current `validate_language` call in `parse_markdown` needs to check the
`code_images:` set *before* it errors, so an author can declare
`mermaid:` without syntect knowing what Mermaid is. Implementation:
`parse_markdown` takes the `&CodeImagesConfig` in addition to the
`&Highlighter`; the validation site checks `code_images` first.

### Substitution details

For a matched code block:

1. Compute `key = sha256(argv‖0x00‖code_text)` (hex).
2. Look for `.peitho/code-images-cache/<key>.svg`. If present, read it.
3. If absent, run `argv[0]` with the rest as arguments, `stdin = code_text`,
   `stdout` captured. On:
   - **Exit code 0 with non-empty stdout that begins with `<svg` (or
     `<?xml` followed by `<svg` after whitespace)**: cache the bytes to
     `.peitho/code-images-cache/<key>.svg` (atomic rename from a temp
     file to survive concurrent watch rebuilds), then use them.
   - **Any other outcome** (nonzero exit, empty stdout, stderr-only
     output, stdout not beginning with SVG marker): build error, line
     number = fenced code block's line, message includes the language
     tag, exit code, first ~200 bytes of stderr. `help:` points at the
     `code_images:` entry.
4. Replace the fragment with `SourceFragment::image(line, alt, raw)`
   where:
   - `line` = original code block's line (preserved for error messages
     further down the pipeline)
   - `alt = format!("diagram ({tag})")` (v1 — no author-controlled
     alt text; future: honor the code block's info-string tail like
     `mermaid attr="alt=architecture"`)
   - `raw = RawImagePath::new(format!(".peitho/code-images-cache/{key}.svg"))` —
     the same raw path form the existing image resolver already accepts

The `resolve_image_paths` pass then walks the substituted fragment like
any other image: it hashes, re-copies into `dist/assets/`, and produces
a `ResolvedImagePath`. **The image resolver already dedups by content
hash, so if two slides embed the same diagram they share one asset in
dist/.** No dedup logic added.

### Error / silent-drop invariants

Every failure mode produces a line-numbered `BuildError` — never a
silent drop:

| Failure | Fires |
|---|---|
| Unknown language tag AND not in `code_images:` | Existing `validate_language` |
| `code_images:` entry has empty command | Parse-time frontmatter error |
| `code_images:` command not on PATH | Substitution error at code-block line |
| Command exits nonzero | Substitution error, exit code + stderr |
| Command hangs > timeout | Substitution error (see Timeout below) |
| Stdout not SVG-shaped | Substitution error |
| Cache directory unwritable | Substitution error |

### Timeout

v1 hard-codes a 30-second per-command timeout. It's implemented like
the export runner's one-shot pattern (spawn + piped readers +
completion predicate + timeout + kill/reap). The timeout is not
authorable in v1 to keep the frontmatter surface minimal; the
`CodeImageCommand` struct is *shaped* for a future `timeout: 60s` field,
so adding it later is not a schema break.

## Cache

`.peitho/code-images-cache/<sha256>.svg`. Keyed on
`SHA256(argv[0] || 0x00 || argv[1] || 0x00 || … || 0x00 || code_text)`.

- Argv-not-command is deliberate: two decks that write the same command
  string in different whitespace/quoting shapes shlex-split to the same
  argv → same key. `sh -c` is not involved, so shell escaping cannot
  smuggle differences into a shared key.
- The cache is shared between `peitho build`, `peitho preview` (watch
  rebuilds), and `peitho publish`. `peitho present` reads pre-built
  output only.
- No eviction policy in v1. The directory grows; author can `rm -rf` it
  safely. A `.gitignore` inside `.peitho/` already covers this.

Cache writes are atomic (temp file + rename) so a `preview` watch that
kills a rebuild mid-write never leaves a truncated `.svg` that a later
run mis-reads as a valid hit.

**Cache is NOT walked by preview file-watching**. Adding a diagram cache
directory to the watch set would cause an infinite rebuild loop
(watcher notifies → rebuild → new cache file → watcher notifies).

## Preview watch interaction

- Watch set unchanged: deck / layouts / css / syntaxes / fonts roots.
  The command's own dependencies (theme files, Mermaid config JSON) are
  **not** watched. This is a documented limitation in the guide.
- Command invocation is on the build hot path; a slow command slows
  every rebuild that hits an uncached diagram. Cache is the mitigation.
- If a command fails during a preview rebuild, the existing
  watch-error-resilience contract applies: the escaped error page is
  served, the previous good generation stays reachable in-flight
  requests, next successful rebuild swaps forward.

## PDF export interaction

The existing `pdf_flatten.js` (gradient + shadow rasterization) runs
against the final DOM; since SVG diagrams enter as `<img src="…svg">`,
they participate in the same flatten pass. Mermaid SVGs can contain
gradients (theme fills) and box-shadow-equivalent filters — the flatten
pass handles both classes already.

**No new PDF-export code is added.** If a specific Mermaid theme
produces artifacts under Preview.app that flatten does not currently
cover, that is a separate follow-up issue with a reproducing deck, not
this issue's scope.

## Publish interaction

The publish contamination check (dist/ must not contain presentation
shell or notes) is unaffected: `.peitho/code-images-cache/` sits under
`.peitho/`, not `dist/`; the substituted images become normal
`dist/assets/<hash>.svg` files.

## Layouts / slots

Substituted images route to the same slot as any other image (`image`
slot by convention, or an explicit `::: {slot=name}` block). No layout
changes are required. Existing decks that use `code:` slots continue to
work — they simply do not declare `code_images:` and their fenced
blocks stay as code.

## Testing plan

Following the repo TDD skill (Red → Green → Refactor for each file):

### Unit — `crates/peitho-core/src/parser.rs`

1. **Red**: frontmatter with `code_images: { mermaid: mmdc -i - -o - -e svg }`
   parses and exposes `CodeImagesConfig` with one entry, shlex-split
   argv `["mmdc","-i","-","-o","-","-e","svg"]`.
2. **Red**: empty command value is a line-numbered parse error.
3. **Red**: unquoted shlex-invalid string (dangling quote) is a
   line-numbered parse error.
4. **Red**: 3-level nesting under `code_images:` is a line-numbered
   parse error.

### Unit — `crates/peitho-core/src/code_images.rs` (new)

5. **Red**: given a `Deck<Parsed>` with a `mermaid` code block and a
   fake runner that returns `<svg>…</svg>`, `transform_code_images`
   returns a deck whose fragment kind is `Image` with the expected raw
   path, and one entry is written under the cache dir.
6. **Red**: cache hit path — pre-populate the cache; the runner must
   NOT be called.
7. **Red**: runner exits nonzero → `BuildError` with the code block's
   line and stderr excerpt.
8. **Red**: runner returns empty stdout → `BuildError`.
9. **Red**: runner returns HTML that is not SVG → `BuildError`.
10. **Red**: unknown language tag not declared in `code_images:` and
    not known to syntect → existing `validate_language` error still
    fires (no regression).
11. **Red**: language tag known to syntect but ALSO declared in
    `code_images:` → `code_images:` wins (`mermaid` is not a syntect
    lang so this is somewhat theoretical; use `json` as the test tag).
12. **Red**: two slides embed the same diagram → one cache file, one
    dist/ asset via the existing image dedup.

### Integration — `crates/peitho-core/tests/`

13. **Red**: end-to-end build of a deck with a `mermaid` code block
    through `parse_markdown` → `transform_code_images` → `map_layouts`
    → `check_deck` → `resolve_image_paths` → `render_deck` produces an
    HTML string containing `<img src=…assets/…svg>`.

### CLI — `crates/peitho/`

14. **Red**: `peitho build` on a deck with `code_images:` writes
    `dist/assets/<hash>.svg` and the rendered HTML references it.
15. **Red**: `peitho build` with a `code_images:` command that does not
    exist on PATH exits nonzero and prints the code block's line.

### Docs

16. Update `CLAUDE.md`'s frontmatter list (add `code_images` after
    `fonts`).
17. Add a section to `site/content/guide/…` (guide page) documenting
    `code_images:` with a mermaid example.

## Contract drift

`bindings/` — no ts-rs type changes are required. `CodeImagesConfig`
lives in the Rust side; the browser never sees it (diagrams are
already-rendered assets by the time HTML ships).

## Example deck

The repository includes `examples/code-images/` as the end-to-end
demonstration deck for this feature. It shows Mermaid and Graphviz
fences transformed into cached SVG image fragments, a before/after
slide that contrasts ordinary highlighted Markdown source with the
transformed image, and a source slide displaying the `code_images:`
frontmatter that powers the build.

## Rollout order (for Codex)

1. Domain: add `CodeImagesConfig` + `CodeImageCommand` to
   `crates/peitho-core/src/domain.rs`. Extend `DeckSettings::new`
   signature.
2. Parser: add nested-mapping grammar extension, add
   `code_images` to `DeckFrontmatter`, thread `CodeImagesConfig` into
   `DeckSettings`. Update `validate_language` call site to accept the
   `code_images:` allow-list.
3. New module `crates/peitho-core/src/code_images.rs` with
   `transform_code_images` and the `SvgRunner` trait.
4. Real runner impl in `crates/peitho/` — wraps `std::process::Command`
   with the timeout / stdin / stdout capture pattern from export.
5. Wire the transform into build/preview/publish call sites.
6. Update `CLAUDE.md` frontmatter list and guide docs.
7. Tests at every step (Red → Green → Refactor).

## What is NOT decided in this plan (author judgement calls left open)

- Whether `code_images:` mapping-form entries (`command:` + future
  fields) are documented in v1 or held back until at least one future
  field ships. **Plan defaults to holding back**: document only the
  bare-string form.
- Whether the substituted image gets a heuristic alt text or an empty
  one for a11y. **Plan defaults to `format!("diagram ({tag})")`**;
  future extension could parse the info-string tail.
- Whether the guide gets a section right away or after this ships to
  users. **Plan writes the section as part of this PR.**

If any of these should flip, tell me before Codex starts.

## Amendments

- `layouts --explain` must also call `parse_deck_and_transform`; otherwise it explains dispatch for a pre-transform `Code` fragment while build/preview/publish dispatch the transformed `Image` fragment.
