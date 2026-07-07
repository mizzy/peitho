# `fonts:` asset key (Issue #159)

## Goal

Bring fonts into the deck's contract as a first-class shared asset so that
`peitho build`, `export pdf`, and `present` all render with the same
web font — regardless of the host environment.

Concretely: a `fonts:` frontmatter key (with deck-adjacent `fonts/`
auto-detect fallback) resolves to a path, and both the build output
directory and the PDF export workspace get a `fonts/` directory
containing the fonts. Deck CSS then uses `@font-face { src:
url("fonts/xxx.woff2") }` — the relative URL resolves inside the emitted
peitho.css, so the export environment (headless Chrome in Linux CI) uses
the deck's fonts instead of falling back to WenQuanYi Zen Hei /
DejaVu Sans.

## Symptom the issue reports

`decks.gosu.ke` `peitho-intro` exports a PDF whose `/BaseFont` shows
`WenQuanYi+Zen+Hei` + `DejaVu Sans` on Linux CI. Headings that fit on one
line in HTML wrap in the PDF because metrics differ, and glyph variants
turn Chinese. Site-vs-PDF parity — the whole promise of peitho's
`export pdf` — is broken *specifically on the export path*, because
peitho has no font contract.

## Root cause

- Deck CSS names `"Noto Sans JP Variable", "Hiragino Sans", …` but the
  matching `@font-face` blocks live in a script (`decks/build.mjs`) that
  injects them into the *site's* HTML after peitho builds. peitho itself
  never sees the fonts.
- peitho's asset tracker (`resolve_image_paths`) only follows markdown
  image references; `write_shared_assets` copies rendered CSS + images
  and nothing else.
- Result: at PDF export time headless Chrome falls back to whatever the
  environment ships. Ubuntu CI without Noto → `fontconfig` picks
  WenQuanYi. macOS → Iowan Old Style / Hiragino.
- The site's PDF thumbnail generator screenshots the built site HTML (with
  fonts injected), so *thumbnails* look right — that's why the mismatch
  went unnoticed until someone rendered a full PDF.

The invariant "site and PDF are rendered the same way" is broken *at the
export seam*, and the reason is that fonts are not part of what peitho
promises to preserve.

## Design — follow the existing asset pattern verbatim

`layouts:` / `css:` / `syntaxes:` already implement exactly this shape:

- YAML frontmatter key with an `AssetPath` value (validated at parse
  time; empty string is a line-numbered error; unknown key is a
  line-numbered error).
- `resolve_assets` in `crates/peitho/src/asset_resolution.rs` returns
  the resolved absolute path, choosing between explicit frontmatter,
  deck-adjacent conventional directory, and "None" in that order.
- `write_shared_assets` copies the assets into the output/workspace
  directory.

The three lenses all point the same way:

1. **Long-term** — anything the site injects today (fonts, `favicon`,
   etc.) should live in the deck's contract, not in a per-site build
   script. Adding `fonts:` closes the specific site-vs-PDF gap and
   establishes the precedent that "if the deck needs it, peitho copies
   it".
2. **Type safety** — `AssetPath` is already the newtype that only the
   parser can construct; extending it to fonts inherits the same
   "impossible to fabricate a path from outside the parser" property
   that layouts/css/syntaxes have. `ResolvedAssets` gains a `fonts:
   Option<PathBuf>` field — matching its siblings — so every downstream
   consumer that reads assets sees fonts in the same shape.
3. **Root cause** — the export-path font mismatch is one symptom of a
   broader missing invariant ("fonts are part of the deck"). Rejected
   alternatives: (a) rewriting `pdf.html` to inline `@font-face src:
   url(file://…)` — carves out the export path only, leaves build and
   present using different fonts; (b) an `--extra-assets` CLI flag —
   bandaid that puts the contract outside the deck. Frontmatter matches
   the existing shape and is discoverable from the deck source.

## What changes

### 1. `crates/peitho-core/src/parser.rs`

- Add `fonts: Option<AssetPath>` to `DeckFrontmatter` (matches
  `layouts` / `css` / `syntaxes`).
- Add `"fonts"` to the key list in `frontmatter_key_lines` so
  `key_line("fonts")` returns the line number.
- Extend the unknown-key help string
  (`"use only the supported deck frontmatter keys: time, aspect_ratio,
  resolution, layouts, css, syntaxes, fonts"`).
- Add the `fonts` branch to `frontmatter_help` for empty/invalid values
  → `"provide a path (relative to the deck file), or remove the fonts:
  key"`.

### 2. `crates/peitho-core/src/phase.rs`

- Add `fonts: Option<AssetPath>` to `DeckSettings` and expose
  `fn fonts(&self) -> Option<&AssetPath>`.
- Thread the new argument through `DeckSettings::new`.

### 3. `crates/peitho/src/asset_resolution.rs`

- Add `fonts: Option<PathBuf>` to `ResolvedAssets`.
- Wire `resolve_asset(deck, frontmatter, "fonts", settings.fonts())`
  into `resolve_assets`. The existing `resolve_asset` function already
  handles explicit-vs-conventional-vs-none — no changes needed there.
- The deck-adjacent conventional directory is `fonts/` (auto-detected
  when the frontmatter key is absent).

### 4. `crates/peitho/src/main.rs`

- `describe_resolved_assets` gains a `fonts=` line.
- `WatchTargets::new` adds fonts to `roots`. Unlike layouts/css/syntaxes
  (which filter by extension), fonts intentionally accept *any*
  extension: a fonts directory may contain `.woff2`, `.woff`, `.ttf`,
  `.otf`, and CSS `@font-face` definition files side by side. Encode
  this with `ext: None` and treat "no extension filter" as "any file in
  the directory is relevant" — with two exclusions:
  - **Dotfiles are ignored** at every depth. `.DS_Store`, editor
    `.swp`, and similar OS-generated noise inside `fonts/` would
    otherwise trigger rebuild storms in watch mode.
  - **Nested subdirectories are recursively watched**. `copy_dir_contents`
    recurses into subdirs (e.g. `fonts/@fontsource/inter/`), so
    `WatchTargets::watch_dirs` calls a new `collect_watch_tree` helper
    to enumerate the fonts subtree and register each subdir with the
    non-recursive notify watcher. Without this, edits to
    `fonts/@fontsource/inter/400.woff2` would be copied by the build
    but silently miss the watcher, producing stale output.
- `write_shared_assets` grows a `write_fonts_assets` step that:
  - **Always clears `<out>/fonts/` first**, whether or not
    `fonts_source` is Some. peitho owns the entire `dist/` tree (same as
    `write_slide_fragments` clearing `dist/slides/` and
    `write_image_assets` clearing `dist/assets/`); leaving a stale
    `dist/fonts/foo.woff2` when the deck later removes its `fonts:` key
    would leak into `peitho publish`.
  - Then if `fonts_source` is None, returns without recreating the dir.
  - Otherwise checks `fs::symlink_metadata` on the source: **any
    symlink** (whether pointing at a file or a directory) is a
    line-numbered miette error, matching the "silent dropping is
    absolutely forbidden" pillar. The rejection is symmetric across
    file and directory forms.
  - Recreates `<out>/fonts/` and copies contents:
    - Directory: `copy_dir_contents` reads entries via
      `DirEntry::file_type()` (does NOT follow symlinks), sorts by
      filename for deterministic output order, then recursively copies
      regular files and subdirectories. Any symlink or special-file
      entry inside is a miette error naming the offending path.
    - Single file: copied to `<out>/fonts/<name>`.
- Fonts are copied to **all three output seams**:
  - `emit_distribution` → build output (calls `write_shared_assets`).
  - `emit_pdf_workspace` → PDF export workspace (calls
    `write_shared_assets`).
  - `emit_present_cache` → `.peitho/present-cache/` (adds a direct
    `write_fonts_assets` call alongside `write_image_assets`). This is
    the load-bearing addition for the issue's own promise: `peitho
    present` also renders with the deck's fonts, not host fallbacks.
- **Not** presentation-only: `PRESENTATION_ONLY_DIST_FILES` stays as-is;
  fonts belong in `dist/` because deck CSS references them from
  peitho.css, which sits at the dist root. The publish contamination
  check uses a top-level filename allowlist and is unaffected by the
  `dist/fonts/` subtree.

### 5. Tests

Follow the existing pattern in `asset_resolution.rs` and `main.rs`:

- Parser: `parse_frontmatter_records_key_line_for_fonts`,
  `parses_frontmatter_fonts_key_carries_to_settings`,
  `rejects_empty_fonts_string_with_line_and_help`, and the
  unknown-key help assertion updates to include `fonts`.
- `asset_resolution.rs`: at least the "explicit path resolves",
  "missing path errors with line + help", and "deck-adjacent
  fonts/ directory is used without frontmatter" cases.
- `main.rs`:
  - `write_shared_assets_copies_fonts_directory` — set up a temp
    dir with `fonts/x.woff2` + `fonts/font.css`, resolve, build,
    assert both files land in `<out>/fonts/`.
  - `write_shared_assets_copies_single_font_file` — file-shaped input.
  - `write_fonts_assets_clears_stale_fonts_when_source_is_none` —
    pre-populate `<out>/fonts/stale.woff2`, run the writer with `None`,
    assert the file is gone.
  - `write_fonts_assets_rejects_symlink_source` (`#[cfg(unix)]`) —
    single-file symlink source is rejected.
  - `write_fonts_assets_rejects_symlink_to_directory_source`
    (`#[cfg(unix)]`) — symlink whose target is a directory is
    also rejected (guards the `Path::is_dir()` symlink-follow gap).
  - `write_fonts_assets_rejects_symlink_entries` (`#[cfg(unix)]`) —
    symlink INSIDE a fonts directory is rejected during recursive copy.
  - `watch_covers_fonts_dir_contents_without_extension_filter` — any
    file extension inside `fonts/` triggers a rebuild.
  - `watch_ignores_dotfiles_in_fonts_dir` — `.DS_Store`, `.swp`, etc.
    are NOT relevant changes.
  - `watch_covers_nested_font_files` — files inside `fonts/inter/`
    still trigger a rebuild; dotfiles inside nested subdirs are still
    ignored.
  - `present_cache_copies_fonts` — `emit_present_cache` writes the
    fonts into the present cache so `peitho present` renders with the
    deck's fonts.
  - `write_shared_assets_copies_single_font_file` — file-shaped
    input.
  - `watch_covers_fonts_dir_contents_without_extension_filter` —
    a file with any extension inside the fonts dir is a relevant
    change.

## Non-goals

- **No CSS `url()` rewriting.** Deck CSS may write `url("fonts/…")`
  directly; the concatenated `peitho.css` sits at the output root so the
  relative URL resolves against the copied `fonts/` sibling. Same in the
  PDF workspace. This is the whole reason the design stays small.
- **No decks-side changes in this PR.** The follow-up work described in
  the issue (migrating `decks/build.mjs` to use `fonts:`, adjusting
  `injectHead`) happens in the `decks` repo after this peitho release.
- **No font subset optimization** or format conversion. peitho just
  copies bytes.

## Validation

- Existing `cargo test --workspace` (run 3× per CLAUDE.md).
- Existing `cargo clippy` / `cargo fmt --check`.
- `git diff --exit-code bindings/` — no binding change expected since
  `DeckSettings` is not exported.
- Manual smoke: build a deck with a `fonts:` dir containing a `.woff2`
  and `@font-face` CSS, run `peitho export pdf`, verify the fonts
  directory lands in the workspace and Chrome uses the specified font.
