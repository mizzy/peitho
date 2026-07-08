# Binary release pipeline

## Goal

`git tag vX.Y.Z && git push --tags` auto-creates a GitHub Release with the `peitho` binary attached as a tar.gz for the following four targets.

- `aarch64-apple-darwin` (macOS arm64)
- `x86_64-unknown-linux-gnu` (Linux x86_64)
- `aarch64-unknown-linux-gnu` (Linux arm64)

**Addendum (2026-07-04)**: `x86_64-apple-darwin` was originally on the list, but the queue wait for the `macos-13` runner on GitHub Actions blocks releases, so it is dropped from the first release. Bring it back as a `--target x86_64-apple-darwin` cross build on `macos-latest` (arm64) when needed (separate issue).

## Approach

- **Trigger**: `v*` tag push only (no manual dispatch needed for now)
- **Distribution shape**: bundle the raw binary into `peitho-vX.Y.Z-<target>.tar.gz` and attach it to the Release
- **Version consistency**: fail the workflow if the tag (`vX.Y.Z`) does not match `workspace.package.version` (`X.Y.Z`)
- **Shell embedding**: the binary bakes in `dist/shell.js`, `layouts/base.html`, and `base.css` via `include_str!`, so `npm ci && npm run build` in `packages/peitho-present` must run before the release build (same order as CI)

## Implementation steps

### 1. Add `.github/workflows/release.yml`

```yaml
name: Release

on:
  push:
    tags: ["v*"]

jobs:
  version-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Verify tag matches Cargo.toml
        run: |
          TAG="${GITHUB_REF_NAME#v}"
          CARGO_VER="$(grep -m1 '^version' Cargo.toml | sed -E 's/version = "(.*)"/\1/')"
          if [ "$TAG" != "$CARGO_VER" ]; then
            echo "::error::tag $GITHUB_REF_NAME does not match workspace version $CARGO_VER"
            exit 1
          fi

  build:
    needs: version-check
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: aarch64-apple-darwin
            os: macos-latest
          - target: x86_64-apple-darwin
            os: macos-13   # x86_64 runner
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest
            linker: aarch64-linux-gnu-gcc
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: packages/peitho-present/package-lock.json
      - run: npm ci
        working-directory: packages/peitho-present
      - run: npm run build
        working-directory: packages/peitho-present
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}
      - name: Install cross linker (arm64 linux)
        if: matrix.target == 'aarch64-unknown-linux-gnu'
        run: |
          sudo apt-get update
          sudo apt-get install -y gcc-aarch64-linux-gnu
          mkdir -p .cargo
          printf '[target.aarch64-unknown-linux-gnu]\nlinker = "aarch64-linux-gnu-gcc"\n' > .cargo/config.toml
      - run: cargo build --release --locked -p peitho --target ${{ matrix.target }}
      - name: Package
        run: |
          VER="${GITHUB_REF_NAME#v}"
          NAME="peitho-v${VER}-${{ matrix.target }}"
          mkdir -p "dist/${NAME}"
          cp "target/${{ matrix.target }}/release/peitho" "dist/${NAME}/"
          cp README.md LICENSE "dist/${NAME}/" 2>/dev/null || true
          tar czf "dist/${NAME}.tar.gz" -C dist "${NAME}"
          (cd dist && shasum -a 256 "${NAME}.tar.gz" > "${NAME}.tar.gz.sha256")
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.target }}
          path: dist/peitho-v*.tar.gz*

  release:
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            dist/*.tar.gz
            dist/*.sha256
          generate_release_notes: true
          draft: false
          prerelease: ${{ contains(github.ref_name, '-') }}
```

### 2. Add a LICENSE file or drop the bundling

There is currently no `LICENSE` file at the repository root, so `cp ... LICENSE` neither fails nor bundles anything. Based on the existing `Cargo.toml`'s `license = "MIT"`, either add `LICENSE` in a separate commit or drop the bundling for now (`2>/dev/null || true` already tolerates it).

### 3. Minimize overlap with CI

Leave `ci.yml` as is. Keep `release.yml` to only what the build needs (test/clippy/fmt/typecheck are assumed to have passed at PR time before merging to main, ahead of tagging).

### 4. Documentation

Add an "Install" section to `README.md` covering the steps to fetch and extract the tar.gz from the Release page, plus the `sha256` verification steps.

## Undecided (out of scope for this PR)

- Homebrew tap
- crates.io publish
- Windows support (currently low priority due to macOS-heavy Chrome dependency)

## Verification

- Fire once with a draft tag (`v0.1.0-rc1`) and inspect the Actions logs (contains `-`, so it is created as a prerelease)
- Confirm locally in advance that `cargo build --release --locked` and `tar czf` work
