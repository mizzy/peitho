# Implementation plan — layering CSS on top of a theme

Issue #620. Design record:
`docs/specs/2026-09-22-builtin-theme-plus-overrides-design.md`.

Every task is Red-Green-Refactor: write the failing test first, then the
production code.

The feature is a fifth asset key, `overrides`, resolved exactly like the
existing four and appended after whatever `css` resolves to. No existing deck
changes behaviour — `overrides/` and `overrides:` do not appear anywhere in the
repository today (verified).

## Task 1 — `AssetKey::Overrides` resolution

`crates/peitho/src/asset_resolution.rs`.

- Add `Overrides` to `AssetKey`, to `AssetKey::ALL` (now 5 entries — the array
  length annotation must change), and to `as_str` as `"overrides"`.
- Add `overrides: Provenance` to `ResolvedAssets` and populate it in
  `resolve_assets` via the same `resolve_asset` call the others use.
- Read the frontmatter value from `settings.overrides()`; that accessor has to
  be added in `peitho-core` alongside the existing `css()` / `layouts()` (see
  Task 2).

`resolve_asset_value` itself needs no change — it is already generic over the
key, and deck-adjacent detection is `deck_parent(deck).join(key.as_str())`,
which yields `overrides/` for free.

Tests, mirroring the existing per-key tests in this file:

1. `deck_adjacent_overrides_directory_records_deck_adjacent_provenance`
2. `missing_overrides_directory_records_builtin_provenance` — resolves to
   `Provenance::Builtin`, i.e. "nothing"; note that for this key `Builtin`
   means no override layer, not an embedded asset.
3. `explicit_overrides_path_that_does_not_exist_is_a_build_error` — line-numbered,
   same as the other keys.

If `Provenance::Builtin` reads wrong for a key that has no built-in asset,
say so in the report rather than inventing a fourth variant — `fonts` already
uses `Builtin` to mean "no extra asset", so this follows precedent.

## Task 2 — the `overrides` frontmatter key

Frontmatter keys are a closed set with line-numbered errors for unknown keys
(CLAUDE.md), so `overrides:` must be registered everywhere the other four asset
keys are, or it will be rejected as unknown. Located already:

- `crates/peitho-core/src/parser.rs:1193` — the recognised-key array used for
  `key_line` bookkeeping (`"layouts", "css", "syntaxes", "fonts", ...`).
- `crates/peitho-core/src/parser.rs:1454` — the `frontmatter_message_mentions_key`
  chain that maps a parse failure onto the offending key.
- `crates/peitho-core/src/phase.rs:424` — the `css()` / `fonts()` accessors
  returning `Option<&AssetPath>`; add `overrides()` beside them.

Follow those four exactly; do not invent a new shape. Search for any other site
that enumerates the asset keys and update it too — the compiler will not catch
a missed string array.

Tests:

1. `overrides_frontmatter_key_is_accepted_as_a_path`
2. `overrides_frontmatter_key_rejects_an_invalid_value` — matching how the
   other asset keys reject theirs.

Check whether `bindings/` contains a generated type covering deck settings. If
it does, regenerate and commit; `git diff --exit-code bindings/` is a gate.

## Task 3 — load and append the override layer

`crates/peitho/src/main.rs`.

`load_css` currently takes `Option<&Path>` and returns the theme files. Extend
the seam so the override files are appended after it. Preferred shape:

```rust
fn load_css(
    css_path: Option<&Path>,
    overrides_path: Option<&Path>,
) -> miette::Result<Vec<peitho_core::CssFile>>
```

so that "theme first, overrides after" is stated once, in the one function that
already owns the built-in-vs-deck decision. The call site at ~line 1867 passes
`assets.overrides.path()`.

While here, factor the `CssFile { name: "base.css (built-in)", .. }` literal
into one constructor rather than repeating the name string.

Tests:

1. `overrides_are_appended_after_the_builtin_theme` — no `css/`, an
   `overrides/` with one file: the result is the built-in theme followed by the
   override, in that order.
2. `overrides_are_appended_after_deck_css` — both present: deck CSS then
   override, and **no** built-in theme.
3. `no_overrides_leaves_css_loading_unchanged` — the regression guard for every
   existing deck.
4. `overrides_directory_reads_multiple_files_in_filename_order` — same
   determinism rule as `css`.

Order matters for the cascade and is the whole point of the feature, so assert
on the order, not just on membership.

## Task 4 — preview watch coverage

`crates/peitho/src/main.rs`, the watch-input snapshot (`WatchRoot` /
`capture_*`, ~line 630).

The deck-adjacent `overrides/` candidate needs the same explicit `Missing`
state the other four candidates have, so that *creating* the directory fires a
rebuild rather than being invisible until some other input changes. Follow the
existing pattern for `css/`; do not add a second detector (CLAUDE.md pitfall:
the watcher is a peitho-owned snapshot and must stay the only trigger).

Test: alongside the existing watch-input tests, assert that an `overrides/`
candidate appears in the captured snapshot for a deck that has no `overrides/`
directory yet.

## Task 5 — migrate `examples/footnotes`

- delete `examples/footnotes/css/base.css` (the vendored theme copy)
- move `examples/footnotes/css/overrides.css` to
  `examples/footnotes/overrides/outro.css` (the deck's `css/` directory then
  disappears entirely, so the built-in theme applies)

Expected, measured on the parent commit: slide HTML byte-identical. **If the
SHA-256 example snapshot moves, stop** — that would mean the migration changed
rendering and the design's premise is wrong.

Add `footnotes_example_layers_overrides_on_the_builtin_theme` to
`crates/peitho/tests/build.rs`: build the example and assert the emitted CSS
contains both a built-in theme marker (the Inter `@font-face`) and the deck's
own `.slot-outro` rule, with the theme appearing first. This is the dogfooding
guard against re-vendoring.

## Task 6 — documentation

- `CLAUDE.md`: the asset-resolution bullet lists four asset keys; add
  `overrides` and state the layering order. Keep the existing sentence that
  `css/` replaces the built-in theme — that stays true.
- `site/content/guide/frontmatter.md`: the "Asset resolution order" and "File
  and directory behavior" sections enumerate the keys. This file is embedded
  into the binary via `crates/peitho/src/docs.rs`, so `peitho docs` picks the
  change up automatically — no second edit site.
- `README.md` only if it enumerates asset keys.

## Gates

The full CLAUDE.md set, including `cargo test --workspace` three times.
`git diff --exit-code bindings/` must be clean (regenerate in Task 2 if deck
settings are part of the contract). The TS bundles are untouched; if any moves,
something unrelated was rebuilt and must not be committed.
