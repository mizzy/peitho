# Release

Create a new GitHub release for peitho with version bump, gates, and binary artifacts.

## When to Use

When the user says "リリースして" / "release" or asks to cut a new version.

## Process

### Step 1: Determine the next version

- Check the current version in `Cargo.toml` (`workspace.package.version`).
- List PRs merged since the last release (`gh release list` to get the timestamp, then `gh pr list --state merged`).
- If any PR is a new feature (`feat:`), bump the minor version. If only fixes, bump the patch version. Let the user override.

### Step 2: Create a worktree and bump the version

- Create a git worktree: `git worktree add ../<repo>-release-<version> -b release-<version>`
- Edit `Cargo.toml`: update `workspace.package.version`.
- **Update `Cargo.lock`**: run `cargo update --workspace` so the lockfile reflects the new version. The release workflow uses `--locked` and will fail if the lockfile is stale.

### Step 3: Run all gates

Run every gate listed in CLAUDE.md's "Gates" section. All must pass:

```
cargo test --workspace          # run 3 times
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm install && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
git diff --exit-code packages/peitho-present/dist/remote.js
```

### Step 4: Commit, push, and create PR

- Stage `Cargo.toml` and `Cargo.lock`.
- Commit with message: `chore: bump version to <version>`.
- Push the branch and create a PR (ready, not draft — see memory `pr-not-draft.md`).

### Step 5: Wait for CI and merge

- Use `merge-when-ready` skill or manually poll CI, then merge with `--merge --delete-branch`.
- Remove the worktree before merging.
- `git pull` and `git remote prune origin` after merge.

### Step 6: Push the tag

- From the main worktree on `main` (after the merge and `git pull`), tag and push:
  `git tag v<version> && git push origin v<version>`.
- **Do not run `gh release create`.** The tag push triggers `.github/workflows/release.yml`,
  which builds the three targets and publishes the release itself via
  `softprops/action-gh-release` with `generate_release_notes: true`. Creating the
  release by hand conflicts with that job.

### Step 7: Verify the release workflow

- Check that the Release workflow (triggered by the tag push) succeeds: `gh run list --limit 3`.
- The workflow runs `version-check` first, which fails if the tag does not match
  `workspace.package.version`.
- If it fails, investigate and fix (the most common issue is a stale `Cargo.lock`).
- Confirm the release published with all six assets (three `.tar.gz` + three `.tar.gz.sha256`):
  `gh release view v<version> --json assets -q '.assets[].name'`.
- Auto-generated notes are usually sufficient. Only edit them (`gh release edit`) if the
  author wants them grouped by New Features / Bug Fixes / Other.

### Step 8: Update the Homebrew formula

- Repo: `mizzy/homebrew-tap` (local clone: `~/src/github.com/mizzy/homebrew-tap`), formula: `Formula/peitho.rb`.
- Fetch the per-target SHA256 from the release assets: `curl -sL https://github.com/mizzy/peitho/releases/download/v<version>/peitho-v<version>-<target>.tar.gz.sha256` for `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`.
- Update `version`, the three `url`s, and the three `sha256`s. Keep `generate_completions_from_executable(bin/"peitho", "completions")` in `install`.
- Create a worktree, commit, push, create a PR, and merge it.
- Verify: `brew update && brew upgrade mizzy/tap/peitho && peitho --version`.

## Important Rules

- Always update both `Cargo.toml` AND `Cargo.lock` when bumping the version.
- Always run gates before committing.
- PRs in this project are created as ready (not draft).
- The release workflow builds binaries with `--locked`, so lockfile drift will cause failure.
- The release is published by the workflow, not by hand — push the tag and let
  `release.yml` create it. Never `gh release create`.
