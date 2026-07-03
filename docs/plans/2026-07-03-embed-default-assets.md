# Embedding the Default Template and Theme in the Binary

## Purpose

Since the defaults for `--template`/`--base-css` are repository-relative paths (`templates/`, `themes/`), running the installed binary in a directory containing only a deck immediately results in file not found. Embed the default assets in the binary so that `peitho build deck.md` works from anywhere.

## Approach

- Embed `templates/title-body-code.html` and `themes/base.css` at build time with `include_str!`. **The repository files remain the single source** (drift is impossible since the same file is pulled in at compile time)
- Make the CLI's `--template`/`--base-css`/`--overrides-css` `Option<PathBuf>`; when unspecified, use the embedded defaults (an empty string for overrides). Passing a path still takes priority over the file as before
- `--watch` only watches explicitly specified files (there is nothing to watch for embedded assets)
- shell.js (the present TS bundle) is an npm build artifact, and embedding it would require integrating the Rust/npm build pipelines, so it is out of scope (as before, `--shell`/repository-relative default)

## Verification

- Unit: Option support for watch_paths, CLI parsing (unspecified=None/specified=Some)
- E2E: confirm that placing only deck.md in a temporary directory and running `peitho build deck.md` succeeds, and that the output peitho.css matches the embedded base theme
