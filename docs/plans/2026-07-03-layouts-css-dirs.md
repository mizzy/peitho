# Make --layouts/--css directory-specifiable

## Purpose (author's proposal)

- Stop repeating `--layout` and instead pass a whole directory via `--layouts <path>`
- Consolidate the two flags `--base-css`/`--overrides-css` into `--css <path>`. The base/overrides distinction is not a two-layer output distinction, it's only a "difference in validation scope" — once the validation rules are made uniform, it can be dropped from the CLI

## CLI

```
--layouts <PATH>  A layout HTML file, or a directory containing *.html (default: built-in title-body-code)
--css <PATH>      A CSS file, or a directory containing *.css (default: built-in base theme)
```

- Directories are read in filename order (deterministic). Both the dispatch trial order and the CSS concatenation order follow filename order (`base.css` → `overrides.css` naturally follows this order)
- If a directory has no files with the target extension, it's a build error
- File specification is also accepted for one-off use

## Uniformizing validation rules (theme.rs)

Old: base.css was unvalidated, overrides.css only allowed slot classes + key/slot validation.
New: apply the same rules to all CSS files.

- Selectors containing `[data-slide-key=...]`: validate that the key exists and that the `.slot-*` within that selector are slots of **that slide's layout** (a broken reference is a build error, invariant 3 of the three pillars holds)
- Bare `.slot-*` classes: validate against **the union of all provided layouts** (typo detection; a slot from a layout the slide doesn't use is still allowed as long as it's provided = doesn't break shared themes)
- All other selectors/properties are free (the theme's expressive power remains unlimited)
- Errors come with filename + line number (`overrides.css: line 3: ...`)

## Follow-up

- Reorganize examples into a `deck.md` + `layouts/` + `css/` structure (drop the empty overrides.css = if there's no file, there's simply nothing)
- Delete `themes/overrides.css` (empty). Keep `themes/base.css` as the source for the built-in default
- `--watch` rebuilds on `*.html`/`*.css` changes within the layouts/css directories
- Follow up in Makefile/README/CLAUDE.md

## Verification

- Unit: loading from file/directory/built-in, filename order, empty-directory error, uniform validation rules (keyed-selector validation applies to all files, bare slot-* union validation)
- Integration: successful build with directory specification
- E2E: build keynote (2 layouts) with directory specification and verify in a real browser
