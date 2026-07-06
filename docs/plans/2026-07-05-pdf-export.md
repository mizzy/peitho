# PDF export (Issue #109)

## Summary

`peitho export pdf <deck.md> -o out.pdf` を追加する。同じMarkdown → Rendered IRを経由し、`Page.printToPDF`相当をヘッドレスChromeで実行して1スライド = 1PDFページを生成する。スペック§13で予告されている「同じIRに対する追加emit target」の最初の実例。

## Motivation

- PDF配布(ブラウザ不要、印刷、アーカイブ)
- §13の拡張点を実際に使い、build/present/publishと並ぶ4番目のemit targetをCLIに追加する
- Issue #23で`aspect_ratio`は表示用、`resolution`はPDF用と役割を分けた前提を、consumerを伴って導入する(受け付けるが使わないキーを作らないという不変条件を守る)

## Non-goals

- v1では**speaker notesをPDFに含めない**。`--with-notes`フラグも入れない。「notesは配布物に入れない」不変条件と方向は同じで、実装量を抑える。将来必要になったらCLI形状の`export pdf`のサブ機能として非破壊で追加できる
- v1では**`peitho publish`とは連携しない**。PDFは`export pdf`専用サブコマンドで完結。将来必要なら`publish --pdf`を非破壊拡張として追加可能
- v1では**フォント埋め込みはブラウザ任せ**。Chromeの`printToPDF`はWebFont/システムフォントをデフォルトで埋め込む。追加の埋め込み処理はしない
- v1では**用紙名(A4, Letter)を`resolution`に受けない**。WxH pxのみ。将来の非破壊拡張

## Design decisions

### CLI shape: `peitho export pdf`

```
peitho export pdf <deck.md> -o out.pdf
peitho export pdf <deck.md>              # -o省略時は`<deck>.pdf`(deck.mdの隣)
```

- `export`は将来のemit target(thumbnails, ...)追加を意識したサブグループ。v1では`pdf`だけ持つ
- `-o`/`--out`のみ受ける。`--watch`は入れない(PDFはワンショット出力)
- Chromeがない環境(見つからない/実行失敗)はline-numbered build errorに準じて明確なエラーメッセージ(存在チェックはbuild時ではなくexport時に行う)

3レンズで一意に決まらなかった選択なので著者判断済み(この選択でproceed):Issueの提案通り`peitho export pdf`。

### Engine: headless Chrome via `--print-to-pdf`

**採用**: 既存の`std::process::Command`スタイルでChromeを呼び、`--headless=new --print-to-pdf=<out> <URL>`だけ渡す。CDP WebSocket不要。

- 3レンズ一致: long-term(CSS変更が自動追従)、type-safety(browser build ↔ PDFが同一のRendered IR起点)、root-cause(§13が予告する通り「同じIRへのemit追加」)
- 既存Chrome dependencyの再利用(browser.rsで既にChromeを起動している)、新規crateなし
- 代替の`chromiumoxide`/`headless_chrome`は重い非同期依存を持ち込むので不採用
- Rustネイティブ(printpdf等)はpillar①「レイアウトの単一源はHTML/CSS」を壊すので不採用

Chromeの呼び出し形状:
```
$CHROME --headless=new \
        --disable-gpu \
        --no-sandbox \
        --no-pdf-header-footer \
        --print-to-pdf=<absolute_out_path> \
        --user-data-dir=<temp_dir>/chrome-profile \
        <url>
```

- `--no-sandbox`はCI/コンテナで必要になるため常時付ける(headlessでの安全性影響は限定的)
- `--user-data-dir`はexport専用の一時ディレクトリ(既存の`chrome-profile-slides`/`presenter`を汚さない)
- `--disable-gpu`はheadless慣習

### Chrome discovery: reuse browser.rs

`browser.rs`が既にChromeの検出ロジックを持つ。それをPDF export用にも拡張する。Chromeが見つからない場合のエラー: line-numbered build errorスタイル(help付き)ではなく、CLI-level miette errorで `install Google Chrome or set PATH to a Chromium-based browser` と返す(Chromeはbuild artifactの入力ではないため)。

Linux/Windows: Chromiumも受ける。macOS: `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`。既存の`browser.rs::detect_chrome`(相当関数)を再利用。

### `resolution` frontmatter key

Issue #109のコメントで確定した仕様に従う:

- **値**: `WxH` px の文字列のみ(例: `1920x1080`)。用紙名は非対応
- **デフォルト**: 未指定なら`aspect_ratio`から導出
  - `16:9` → `1920x1080`
  - `4:3` → `1440x1080`
- **整合ルール**: `resolution`のアスペクト比と`aspect_ratio`が一致しなければ**line-numbered build error**
  - 例: `aspect_ratio: 16:9` + `resolution: 1024x768` → error
- **不正値**(空、`WxH`パースエラー、0×0、桁溢れ)は全てline-numbered build error

**デフォルト解像度**: `aspect_ratio: 16:9` → `1920×1080`は、canvas論理サイズ(1280×720)より高解像度。理由: PDFは印刷/拡大表示されうるので物理解像度を高くとる。CSS px単位は`96 DPI`で解釈されるので、1920×1080はChrome内部で20 inch × 11.25 inchのページになる。用紙相当ではなくスライド1枚1ページの物理サイズと解釈する。

### Type: `Resolution` newtype in `peitho-core`

`AspectRatio` enum と対になる型として `Resolution` を導入する。

```rust
/// A physical PDF page size in CSS pixels (96 dpi).
/// Constructed only via `Resolution::from_frontmatter` or
/// `Resolution::from_aspect_ratio_default`, so raw (u32, u32)
/// pairs cannot masquerade as a validated resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Resolution {
    width: u32,
    height: u32,
}

impl Resolution {
    /// Frontmatter parse — the only entry point for user-authored values.
    /// Validates `WxH` shape, non-zero, and no overflow.
    pub fn from_frontmatter(raw: &str) -> Result<Self, String> { ... }

    /// Default derived from aspect_ratio when frontmatter omits `resolution`.
    /// - 16:9 → 1920x1080
    /// - 4:3 → 1440x1080
    pub fn from_aspect_ratio_default(ratio: AspectRatio) -> Self { ... }

    pub fn width(self) -> u32 { self.width }
    pub fn height(self) -> u32 { self.height }

    /// Verify `self`'s aspect ratio matches the deck's `AspectRatio`.
    /// Returns Ok if they match, Err with a message if they diverge.
    pub fn check_matches(self, ratio: AspectRatio) -> Result<(), String> { ... }
}
```

**Rationale — 型で不変条件を表現する**:
- `Resolution`のフィールドは`pub`にしない → 外部crateは検証済みの値しか作れない
- 生の`(u32, u32)`との比較経路が閉じる(mislabel防止)
- `check_matches`は`DeckSettings::new`内部で呼ぶ。frontmatter parser以外のcallerでもconstruction時にintegrity checkを通る

### DeckSettings に `resolution` を追加

```rust
pub struct DeckSettings {
    // ... existing
    aspect_ratio: AspectRatio,
    resolution: Resolution,      // new
}
```

`DeckSettings::new`シグネチャに`resolution: Option<Resolution>`を追加。`None`なら`Resolution::from_aspect_ratio_default(aspect_ratio)`を使い、`Some`なら内部で`check_matches(aspect_ratio)`を呼ぶ。既存呼び出し(テストfixtures含む)を全て更新する。

**Manifest には出さない**: `resolution`はPDF exportだけの関心事で、browser build/presentが読む理由がない。Manifestに載せると「受け付けるが使わない」ペイロードになるので敢えて省く。

### Parser: `resolution` key handling

`parser.rs::DeckFrontmatter`に`resolution: Option<String>`を追加:

1. Rawが`None` → `DeckSettings::new(..., None, ...)`に渡し、constructor内部で`Resolution::from_aspect_ratio_default(aspect_ratio)`を使う
2. Rawが`Some(s)` → `Resolution::from_frontmatter(s)` → `DeckSettings::new(..., Some(resolution), ...)`に渡し、constructor内部で`check_matches(aspect_ratio)`を呼ぶ
3. どちらのエラーもline-numbered build error

`frontmatter_key_lines`に`"resolution"`を追加。

### PDF entry HTML: `pdf.html`

PDFはブラウザで実際にレイアウトさせる必要があるので、専用のentry HTMLを生成する:

```html
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="peitho.css">
  <style>
    @page {
      size: {{resolution.width}}px {{resolution.height}}px;
      margin: 0;
    }
    html, body { margin: 0; padding: 0; background: #fff; }
    .peitho-slide {
      width: {{resolution.width}}px;
      height: {{resolution.height}}px;
      page-break-after: always;
      page-break-inside: avoid;
      break-after: page;
      break-inside: avoid;
      overflow: hidden;
      /* Scale the 1280x720 (or 960x720) canvas up to resolution. */
      transform: scale({{resolution.width / aspect_ratio.width}});
      transform-origin: top left;
    }
    .peitho-slide:last-child { page-break-after: auto; break-after: auto; }
  </style>
</head>
<body>
  {{#each slides}}<div class="peitho-slide-wrap">{{slide.html}}</div>{{/each}}
</body>
</html>
```

実際にはHandlebarsは使わずRust側で文字列で組み立てる(既存の`render_distribution_index`と同じ流儀)。

**サイズ計算の要点**:
- Layoutの`.peitho-slide`は`--peitho-canvas-width/height` CSS変数で1280×720 (or 960×720) を宣言済み(Issue #23で導入)
- PDF `.peitho-slide-wrap`は`resolution.width × resolution.height`のフレームを作り、その中で内側の`.peitho-slide`(canvas論理サイズ)を`transform: scale()`で拡大する
- 論理サイズと物理サイズを分けるのがpoint。base.cssのフォントサイズ設計を壊さない

`resolution: 1920x1080` + `aspect_ratio: 16:9`(canvas 1280×720)の場合、scale = 1.5。

**Rendering location**: `render.rs`に`render_pdf_document(&Deck<Rendered>) -> String`を追加する。既存の`render_distribution_index`とは別関数(役割が違う; distribution indexはfetch-drivenで動的、PDF entryはstaticでinline)。

### Export pipeline

```rust
fn export_pdf(input: PathBuf, out: Option<PathBuf>) -> miette::Result<()> {
    // 1. Same as build: parse → map → check → render.
    let artifacts = build_artifacts(&input)?;

    // 2. Determine output path.
    let out = out.unwrap_or_else(|| input.with_extension("pdf"));

    // 3. Emit static PDF entry into a temp directory:
    //    <tmp>/pdf.html
    //    <tmp>/peitho.css
    //    <tmp>/assets/*   (images)
    //    (No slides/, no manifest — the entry HTML inlines slide HTML.)
    let tmp = tempfile::tempdir()?;
    emit_pdf_workspace(tmp.path(), &artifacts)?;

    // 4. Locate Chrome.
    let chrome = locate_chrome()?;

    // 5. Invoke Chrome:
    //    $chrome --headless=new --disable-gpu --no-sandbox
    //            --no-pdf-header-footer
    //            --user-data-dir=<tmp>/chrome-profile
    //            --print-to-pdf=<abs_out_path>
    //            file://<tmp>/pdf.html
    run_chrome_print(&chrome, tmp.path(), &out)?;

    println!("exported {} slide(s) to {}", artifacts.slide_count, out.display());
    Ok(())
}
```

**Why `file://` not a local HTTP server**: 
- pdf.htmlは同一originの相対パスで`peitho.css`と`assets/*`を参照する
- `file://`origin下でも相対fetchはimg/link/scriptタグ経由ならCORSに引っかからない(fetch APIは制限がかかるがpdf.htmlはfetchを使わない、全てinlineでstatic)
- Serverを立てるとephemeral port管理・shutdownが増えるので、fetchを使わないstatic HTMLならfile://で十分

**Image assets**: 既存の`ResolvedImageAsset`を`assets/`にコピー(build時と同じロジック)。相対srcはpdf.htmlのdocument locationからの相対で解決される。

**Syntax highlighting**: `render_slide`が生成する`hl-*`クラスのspanは、`peitho.css`(theme)に依存する。既存のCSS pipelineをそのまま使う。

### Notes stripping

Renderedの`RenderedSlide::notes`はPDF entry HTMLに一切埋め込まない。invariant「notesは配布物に入らない」を守るためexplicit check(`emit_pdf_workspace`内でrendered HTMLからnotesが排除されていることをassert)。

### Chrome discovery module

Chrome executable pathの検出:

```rust
fn locate_chrome() -> miette::Result<PathBuf> {
    // macOS: /Applications/Google Chrome.app/Contents/MacOS/Google Chrome
    // Linux: PATH lookup for `google-chrome` / `google-chrome-stable` / `chromium` / `chromium-browser`
    // Windows: registry / PATH lookup (defer to v1.1 if not trivial; unsupported in v1)
    // Env override: PEITHO_CHROME_PATH
}
```

- `PEITHO_CHROME_PATH`環境変数を最優先(CIやカスタムパス用)
- 見つからない場合: `miette::miette!("Chrome not found\nhelp: install Google Chrome or Chromium, or set PEITHO_CHROME_PATH=<absolute-path>")`

## Scope of changes (Codexへの実装スコープ)

### Rust: peitho-core

- [ ] `domain.rs`: `Resolution` newtype 追加(private fields, `from_frontmatter`, `from_aspect_ratio_default`, `width`, `height`, `check_matches`, serde `try_from = "String"`)
- [ ] `phase.rs::DeckSettings`: `resolution: Resolution`フィールド追加、`DeckSettings::new`シグネチャに`resolution: Option<Resolution>`を追加、`None`時のdefault derivationと`Some`時の`check_matches(aspect_ratio)`を内部で実行、既存呼び出し(fixtures含む)を全て更新、`resolution()`accessor
- [ ] `parser.rs::DeckFrontmatter`: `resolution: Option<String>`フィールド追加、build時に `Resolution::from_frontmatter` した値を `DeckSettings::new(..., Some(resolution), ...)` に渡し、line-numbered build errorに変換
- [ ] `parser.rs::frontmatter_key_lines`: `"resolution"` を追加
- [ ] `render.rs`: `render_pdf_document(deck: &Deck<Rendered>) -> String` を追加。`.peitho-slide`をstackし、`@page {size:WxH; margin:0}`とscale transformを埋め込む。notes埋め込みなしをassertする内部フローを持つ

### Rust: peitho crate

- [ ] `src/main.rs`: `Command::Export { command: ExportCommand }` を追加。`ExportCommand::Pdf { input, out }` variant
  ```rust
  #[derive(Debug, Subcommand)]
  enum Command {
      Build { ... },
      Present { ... },
      Publish { ... },
      Export {
          #[command(subcommand)]
          command: ExportCommand,
      },
  }
  #[derive(Debug, Subcommand)]
  enum ExportCommand {
      Pdf {
          input: PathBuf,
          #[arg(short, long)]
          out: Option<PathBuf>,
      },
  }
  ```
- [ ] `src/main.rs::export_pdf`: pipeline実装(build_artifacts再利用 → emit_pdf_workspace → locate_chrome → run_chrome_print)
- [ ] `src/main.rs::emit_pdf_workspace`: temp dirに `pdf.html`, `peitho.css`, `assets/*` を書き出し(既存の `emit_distribution` の部分を再利用/リファクタ)
- [ ] `src/main.rs::locate_chrome`: `PEITHO_CHROME_PATH`優先 → macOS default path → Linux PATH lookup(既存の`browser.rs`のChrome検出があれば再利用)
- [ ] `src/main.rs::run_chrome_print`: `Command::spawn` + wait + exit code チェック → 出力ファイル存在チェック
- [ ] `Cargo.toml`: `tempfile.workspace = true` を dev-dependencies から regular dependencies に移す(既にworkspace設定があるので確認)

### Tests

- [ ] `peitho-core/tests/parser`: `resolution: 1920x1080` → parse OK、`resolution: 1024x768` + `aspect_ratio: 16:9` → line-numbered error、`resolution: abc` → error、`resolution: 0x1080` → error、`resolution: 9999999999x1080`(10桁 > u32::MAX)→ u32 parse失敗のerror、default derivation(16:9 → 1920×1080、4:3 → 1440×1080)
- [ ] `peitho-core/src/domain.rs`: `Resolution::from_frontmatter`, `from_aspect_ratio_default`, `check_matches` の単体テスト
- [ ] `peitho-core/src/render.rs`: `render_pdf_document`が全slideを含む、`@page {size:1920px 1080px}`を含む、`page-break-after: always`を含む、notesを一切含まない
- [ ] `peitho/tests/export_pdf.rs`: 新規統合テスト。fixtureのdeck.mdをexportしてoutput PDFが存在する&サイズが0でない&`%PDF-`headerで始まる。**Chromeが検出できない環境ではskip**(CIには`ChromeDpEnvVar`が入ってる、なければ`#[ignore]`扱い or `PEITHO_CHROME_PATH=$(command -v chromium || echo /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome) cargo test`)

### Bindings

`Resolution`は`AspectRatio`と違いManifestに載せないので、ts-rs binding不要。

### Examples

- [ ] `examples/pdf-export/`: 動作確認用のsample deck(README付き)。`aspect_ratio: 16:9`と`resolution: 1920x1080`を明示

### Docs

- [ ] このplan file(既に書いた)
- [ ] `CLAUDE.md` frontmatterキー一覧に`resolution`を追記(worktree/PR内で)
- [ ] Issue #109をclose(PRの`Closes #109`)

## Test plan (TDD order)

Codexへの指示順:

1. `Resolution::from_frontmatter` の単体テスト(RED) → 実装(GREEN)
2. `Resolution::from_aspect_ratio_default` (RED → GREEN)
3. `Resolution::check_matches` (RED → GREEN)
4. Serde `try_from`/`into` の丸トリップ(RED → GREEN)
5. Parser tests: `resolution`キーのvalid/invalid/default derivation/mismatch(RED → GREEN)
6. `DeckSettings::new`シグネチャ変更(`resolution: Option<Resolution>`、内部でdefault derivation + `check_matches`) → 既存呼び出しupdate(REFACTOR)
7. `render_pdf_document` (RED → GREEN)、notes除外assert(RED → GREEN)
8. `export_pdf` pipeline integration test(Chrome availableな環境で、`#[ignore]`可)
9. E2E手動: `peitho export pdf examples/pdf-export/deck.md -o /tmp/out.pdf` → 開いて確認(過去にflashing/black screenのincident多発なので実PDFで確認)

## Root-cause / long-term / type-safety self-check

- **Root-cause**: Issue #109のscopeは「PDF出力の実装」であり、`resolution`キー未実装は#23の分割時点で予告済み。同PRで一緒に実装するのが正しい(#23の`aspect_ratio`だけ入れて`resolution`は受け付けるが使わない、という状態を作らないため)
- **Long-term / type-safety**:
  - `Resolution` newtype化により、外部からは`from_frontmatter`か`from_aspect_ratio_default`経由でしか作れない → 生の`(u32,u32)`をResolutionとして扱う経路が閉じる
  - `check_matches(aspect_ratio)`を`DeckSettings::new`内部で呼ぶ設計により、mismatchのままDeck<P>を流通させる経路がない(consumer側で`check_matches`を呼び忘れる懸念が消える)
  - `Command::Export { ExportCommand }` サブグループ化により、将来のexport target追加(thumbnails等)がenumのvariant追加として型で表現される
- **Silent path**: 
  - 不正な`resolution`値 → line-numbered error
  - `aspect_ratio`と`resolution`のmismatch → line-numbered error
  - Chrome未検出 → clear error with help
  - Chrome非0 exit → propagated with stderr
  - PDF outputが空/存在しない → error(サニティチェック)
- **The "new caller tomorrow" check**: 将来のexport target(thumbnails等)は`Deck<Rendered>` + `Resolution`を受け取ればよい(型が導く)。忘れられがちなdeckのresolutionは`DeckSettings`にridingしているので明示的引数を要求される

## Verification gates

CLAUDE.md必須:
- `cargo test --workspace`(3回連続)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/`(今回は変更なしのはず)
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js`

E2E(手動):
- Chromeがある環境で `cargo run -p peitho -- export pdf examples/pdf-export/deck.md -o /tmp/out.pdf`
- 生成された`out.pdf`を開いて全slideが正しく描画されている(コード/画像/背景色含む)ことを確認
- `aspect_ratio: 4:3`のfixtureでも同様に確認
- `resolution: 1024x768`(16:9とmismatch)がline-numbered errorになることを確認

## Open questions (implementation中に判明したら著者に確認)

- Linux/Windows/CI環境でのChrome検出のfallback順序: PEITHO_CHROME_PATH → 具体的にどのbin名を試すか(google-chrome-stable/google-chrome/chromium/chromium-browser)。Issueで先に確定させるより実装時に確認
- `@page`サイズをCSS pxで指定できるか: 実測が必要。もし駄目ならinch単位に変換(width_px / 96)
- 空Deck(0スライド)の扱い: PDF生成をskipしてエラーにするか、空PDFを出すか

## Shipped divergence / adjustments during review (2026-07-06)

1. `Resolution` serde は private wrapper ではなく `#[serde(try_from = "String", into = "String")]` として shipped。`TryFrom<String>` は `Resolution::from_frontmatter` に委譲し、`From<Resolution> for String` が `WxH` wire string を生成する。
2. `DeckSettings::new` は必須の解決済み値ではなく `resolution: Option<Resolution>` を受け取る形で shipped。`None` は `Resolution::from_aspect_ratio_default(aspect_ratio)` に解決し、`Some` は constructor 内部で `check_matches(aspect_ratio)` と canvas 論理サイズ以上の検証を通すため、parser以外のcallerもmismatch / undersized resolutionを流通させられない。
