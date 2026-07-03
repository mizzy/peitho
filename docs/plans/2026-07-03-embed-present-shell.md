# Embedding the presentation shell (shell.js) in the binary

## Purpose

Even after embedding templates and themes, `present` alone still depended on the repo-relative path `packages/peitho-present/dist/shell.js`, so an installed binary couldn't run `peitho present` from outside the repo. Embed the shell too, so all three commands are fully standalone.

## Approach: same discipline as bindings/ — commit the generated artifact + check drift in CI

- Rejected: having build.rs invoke npm — this would drag a node/npm dependency into `cargo build` and break the pure-Rust build
- Adopted: commit `dist/shell.js` and embed it via `include_str!`. CI's node job runs `npm run build` then checks drift with `git diff --exit-code dist/shell.js` (the same discipline as the TS types in bindings/. esbuild output is deterministic because it's pinned via package-lock)
- `.gitignore` changes from `dist/` to `dist/*` + `!dist/shell.js` (because of git's behavior where re-including doesn't work if the parent directory is excluded). sourcemaps remain ignored
- `--shell` becomes `Option<PathBuf>`. Unspecified = write the embedded shell out to present-cache; specified = copy the file as before (the "shell bundle not found" error only applies when an explicit path is given)
- Dev flow: after editing TS, run `npm run build` → cargo recompiles via the `include_str!` dependency tracking, and the embedded copy updates too. The Makefile's `shell` dependency stays as before

## Verification

- Unit/integration: confirm that with `--shell` unspecified, present-cache's shell.js is generated with the embedded content; confirm the existing behavior with an explicit path; confirm that the missing-file error is limited to the explicit-path case
- E2E: in a temporary directory outside the repo, confirm that `peitho present deck.md --no-open --port <fixed>` starts and that /shell.js is served
