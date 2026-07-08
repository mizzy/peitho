# Remove pptx export (Issue #152)

## Background

`peitho export pptx` produces files that PowerPoint flags as corrupt ("problem was found with the content"). Quality is insufficient, so the entire feature is removed.

## Scope

Delete the `pptx` subcommand and all code, tests, types, bindings, docs, and dependencies that exist only to support it. Preserve shared infrastructure used by PDF export.

## Files to delete entirely

1. `crates/peitho-core/src/pptx.rs` — the pptx writer
2. `crates/peitho/tests/export_pptx.rs` — E2E test
3. `packages/peitho-present/src/measure.ts` — DOM measurement script (pptx-only)
4. `packages/peitho-present/dist/measure.js` — compiled measurement script (embedded in binary)
5. `packages/peitho-present/test/measure.test.ts` — measurement tests
6. `bindings/Measured{Deck,Slide,Box,BoxStyle,Paragraph,Run,Image,Rect}.ts` — 8 TS binding files

## Files to edit

### Rust

1. **`crates/peitho-core/src/lib.rs`** — remove `pub mod pptx;`, `pub use pptx::build_pptx;`, and all `Measured*` re-exports from `pub use domain::`
2. **`crates/peitho-core/src/domain.rs`** — remove 8 `Measured*` struct definitions (~lines 399-517) and their tests (~lines 1114-1245)
3. **`crates/peitho-core/src/render.rs`** — remove `BUILTIN_MEASURE_JS` constant, `render_measure_document()` function, and its two tests
4. **`crates/peitho/src/main.rs`** — remove:
   - `ExportCommand::Pptx` variant and its match arm
   - `export_pptx()`, `write_pptx_output()`, `keep_measure_workspace_for_error()`
   - `emit_measure_workspace()`
   - `run_chrome_dump_dom()`, `extract_measure_json()`, `extract_script_payload()`, `measurement_error_message()`
   - `ChromeCompletion::DumpDom` variant and its `is_ready` arm
   - 2 unit tests for pptx export
5. **`crates/peitho-core/Cargo.toml`** — remove `zip.workspace = true`
6. **`crates/peitho/Cargo.toml`** — remove `zip.workspace = true` from dev-dependencies
7. **`Cargo.toml` (workspace root)** — remove `zip` from workspace dependencies

### TypeScript / JS

8. **`packages/peitho-present/esbuild.config.mjs`** — remove the second `build()` call for measure.ts

### Documentation

9. **`README.md`** — remove pptx usage example and export explanation
10. **`CLAUDE.md`** — remove pptx/measure.js references from Structure, Gates, and Pitfalls sections

### CI

11. **`.github/workflows/ci.yml`** — remove `git diff --exit-code dist/measure.js` drift check

## Shared infrastructure to PRESERVE

- `run_one_shot_chrome` and Chrome pipe infrastructure (used by PDF)
- `ChromeCompletion::PdfWritten` variant and its logic
- `locate_chrome`, `chrome_print_args`, `run_chrome_print`
- The entire `export pdf` subcommand and everything it depends on

## Verification

After all changes:
```
cargo test --workspace          # 3 times
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
```

Note: `dist/measure.js` drift check is removed as part of this change.
