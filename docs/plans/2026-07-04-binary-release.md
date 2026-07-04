# バイナリリリースの仕組み

## ゴール

`git tag vX.Y.Z && git push --tags` で GitHub Release が自動生成され、以下4ターゲットの `peitho` バイナリが tar.gz で添付される状態を作る。

- `aarch64-apple-darwin` (macOS arm64)
- `x86_64-apple-darwin` (macOS x86_64)
- `x86_64-unknown-linux-gnu` (Linux x86_64)
- `aarch64-unknown-linux-gnu` (Linux arm64)

## 方針

- **トリガー**: `v*` タグ push のみ (手動起動は当面不要)
- **配布形態**: 生バイナリを `peitho-vX.Y.Z-<target>.tar.gz` に固めて Release に添付
- **バージョン整合**: タグ (`vX.Y.Z`) と `workspace.package.version` (`X.Y.Z`) が一致しない場合はワークフローを失敗させる
- **シェル埋め込み**: バイナリは `include_str!` で `dist/shell.js` と `layouts/base.html` と `base.css` を焼き込むので、リリースビルド前に `packages/peitho-present` の `npm ci && npm run build` を必ず走らせる (CIと同じ順序)

## 実装ステップ

### 1. `.github/workflows/release.yml` を新設

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

### 2. LICENSE を用意するか同梱をやめる

現状 `LICENSE` ファイルがリポジトリ直下に存在しないので、`cp ... LICENSE` は失敗しないが同梱もされない。既存 `Cargo.toml` の `license = "MIT"` を根拠に、別コミットで `LICENSE` を追加するか、当面は同梱をやめる (`2>/dev/null || true` で許容してある)。

### 3. CIとの重複を最小化

`ci.yml` は現状のまま。`release.yml` は build に必要な最低限のみ (test/clippy/fmt/typecheck は タグを打つ前の PR 時点で通ってから main にマージされている前提)。

### 4. ドキュメント

`README.md` に "Install" セクションを追加し、Release ページの tar.gz を落として展開する手順、および `sha256` の検証手順を書く。

## Undecided (この PR では触らない)

- Homebrew tap 化
- crates.io publish
- Windows 対応 (現状 chrome の macOS 依存が強く、優先度低)

## 検証

- ドラフトタグ (`v0.1.0-rc1`) で一度発火させて Actions ログを見る (`-` を含むので prerelease として作成される)
- ローカルで `cargo build --release --locked` と `tar czf` が動くことは事前に確認する
