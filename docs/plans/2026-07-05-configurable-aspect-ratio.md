# Configurable slide aspect ratio (Issue #23)

## Summary

スライドの canvas サイズを固定 1280×720 (16:9) から、frontmatter の `aspect_ratio` キーで選択可能にする。v1 の値は `16:9` (デフォルト) と `4:3` の 2 種類。

## Motivation

- 4:3 スクリーンや古いプロジェクタでの発表ニーズ
- Issue #109 (PDF export) が canvas サイズを前提にしているので、その前段として片付けたい
- 現状は `1280` と `720` が **6 箇所以上に散在**(TS 定数、CSS、Rust の埋め込み HTML/JS、テスト)しており、単一情報源を持つべき

## Non-goals

- 論理解像度そのものの指定(例: `resolution: 1920x1080`)は **やらない**。これは #109 (PDF export) で `resolution` キーとして扱う([#109 のコメント参照](https://github.com/mizzy/peitho/issues/109#issuecomment-4885813888))
- `16:9` / `4:3` 以外のアスペクト比(`16:10`、`21:9` など)は v1 では受けない。追加は将来の非破壊拡張

## Design decisions

### Key name: `aspect_ratio`

`aspect` ではなく `aspect_ratio`。理由:
- CSS `aspect-ratio` プロパティ、Reveal.js / Slidev などのエコシステムと整合
- `aspect` 単独は「側面・観点」の意味が主で、設定キーとして曖昧
- 既存キー(`time`, `layouts`, `css`, `syntaxes`)と同じく、2 語で意味が閉じる

### Value: `W:H` string, restricted to `16:9` and `4:3`

- 値は `"16:9"` または `"4:3"` の 2 種類のみ
- それ以外は **line-numbered build error**(既存の frontmatter 検証と同じ流儀)
- 未指定なら `16:9`(現状と同じ 1280×720)

これは以下の invariant を守るための選択:
- Silent path 禁止 → 不正値は必ずエラー
- 「受け付けるが使わない」も禁止 → consumer が居ないバリアントは作らない

論理解像度は内部固定マッピング(TS/CSS/Rust の 3 箇所同期を単純化):
- `16:9` → 1280 × 720
- `4:3` → 960 × 720 (高さを 720 で揃える。base.css のフォントサイズなどの px 設計を再チューニングせずに済む)

**高さ 720 を揃える理由**: base.css は `font-size: 56px` などが 720px 高を前提に設計されている。幅だけ変えることで、既存テーマがそのまま 4:3 でも読みやすさを保つ。ユーザーが「4:3 だと文字が小さすぎる」と感じたら CSS カスタムの側で対処できる。

### Type: `AspectRatio` enum (Rust)

`peitho-core` に `AspectRatio` を導入する。レビュー中に newtype ではなく enum に変更した。理由:
- `16:9` / `4:3` の 2 値だけが合法、という invariant を型そのものが表現する
- serde の `Deserialize` で wire label (`"16:9"` / `"4:3"`) と variant の対応を表す
- `FromStr` は `domain.rs` の 1 箇所で実装し、frontmatter parser はそこへ委譲する
- `pub fn width(self) -> u32` / `pub fn height(self) -> u32` を公開。consumer はここから値を取る
- Default は `16:9`(1280 × 720)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectRatio {
    #[serde(rename = "16:9")]
    Ratio16To9,
    #[serde(rename = "4:3")]
    Ratio4To3,
}
```

`FromStr` は `AspectRatio` に実装し、parser 側は `value.parse::<AspectRatio>()` に委譲する。他の値は parser 側で `BuildError` with line number に変換する。

### DeckSettings に `aspect_ratio` を追加

`DeckSettings::new` に `aspect_ratio: AspectRatio` を追加。`Deck<P>` を通じて全 phase に載る。既存の `planned_time` / `sections` / `layouts` / `css` / `syntaxes` と同じ扱い。

### Single source of truth

**Rust 側 (`AspectRatio`) が単一情報源。** 経路は 3 系統:

1. **manifest.json に出力** → `aspectRatio` は semantic string (`"16:9"` / `"4:3"`)、`canvasWidth` / `canvasHeight` は `AspectRatio` から Rust 側で導出した数値。`shell.ts` は数値フィールドを `installCanvasScaler` に渡すので、TS 側に label→pixel mapping を重複させない
2. **`themes/base.css` を CSS カスタムプロパティ化**: `width: 1280px` → `width: var(--peitho-canvas-width, 1280px)`, `height: 720px` → `var(--peitho-canvas-height, 720px)`。`shell.ts` は shell root に `--peitho-canvas-width` / `--peitho-canvas-height` / `--peitho-canvas-aspect` を注入する
3. **`crates/peitho-core/src/render.rs` の埋め込み HTML/JS**: `render_distribution_index` / present / presenter template は `Deck<Rendered>` の `AspectRatio` から width / height / CSS aspect (`16 / 9` or `4 / 3`) を生成する。Presenter の `.stage` / `.slide-pane` / `.next-preview` は `var(--peitho-canvas-aspect)` を使う

### Manifest schema 拡張

wire form は camelCase で、semantic label と導出済み numeric dimensions を持つ:

```json
{
  "aspectRatio": "16:9",
  "canvasWidth": 1280,
  "canvasHeight": 720,
  ...
}
```

`aspectRatio` は semantic string (`"16:9" | "4:3"`)。Rust 内部の `Manifest` は `aspect_ratio: AspectRatio` だけを持ち、`canvas_width()` / `canvas_height()` accessor は `aspect_ratio` から導出する。serde は private `ManifestWire` を経由する。Deserialize では `aspectRatio` が authoritative で、JSON 内の `canvasWidth` / `canvasHeight` は accept-and-drop される(実装上は `#[serde(default)]` で読み取り、`Manifest` への変換時に捨てる)。Serialize では `ManifestWire::from(&Manifest)` が `AspectRatio::width()` / `height()` から数値を埋める。

`bindings/Manifest.ts` にも `aspectRatio`, `canvasWidth`, `canvasHeight` が出る(ts-rs 生成)。CI の drift チェックは既存のものが利用できる。

## Scope of changes (Codex への実装スコープ)

Rust (peitho-core):
- [ ] `domain.rs` or 新ファイル: `AspectRatio` enum 追加
- [ ] `phase.rs::DeckSettings`: `aspect_ratio: AspectRatio` フィールド追加、`new()` に引数追加(既存呼び出し全部を更新)、`aspect_ratio()` accessor
- [ ] `parser.rs::DeckFrontmatter`: `aspect_ratio: Option<String>` フィールドを追加 → 検証して `AspectRatio` に変換 → `DeckSettings::new` に渡す
- [ ] `parser.rs::frontmatter_key_lines`: `"aspect_ratio"` を追加
- [ ] 不正値 (`"16:10"` など) を line-numbered build error にする(既存の time invalid と同じ流儀)
- [ ] `manifest.rs`: `Manifest` に `aspect_ratio: AspectRatio` を追加、`canvasWidth` / `canvasHeight` は private `ManifestWire` で serialization 時に導出
- [ ] `render.rs::render_distribution_index` / present / presenter template: シグネチャに `aspect_ratio: AspectRatio` を受けて、埋め込み HTML/JS/CSS の 1280/720/16:9 を動的値に

Rust (peitho crate):
- [ ] `crates/peitho/tests/build.rs`: 既存の `assert!(...contains("const CANVAS_WIDTH = 1280"))` 系を、デフォルト(未指定) 16:9 と 4:3 の両方でテスト
- [ ] `crates/peitho/tests/build.rs::base_theme_targets_fixed_canvas_size`: CSS カスタムプロパティ化後の変数注入をテスト
- [ ] `crates/peitho/src/main.rs`: 同様の assert 更新

TS (peitho-present):
- [ ] `packages/peitho-present/src/canvas.ts`: `installCanvasScaler` は `canvasWidth`/`canvasHeight` を required option として受ける。`?? CANVAS_WIDTH` / `?? CANVAS_HEIGHT` fallback と exported constants は削除する
- [ ] `packages/peitho-present/src/shell.ts`: manifest の `canvasWidth` / `canvasHeight` を読み取って `installCanvasScaler` に渡す + shell root に `--peitho-canvas-width` / `--peitho-canvas-height` / `--peitho-canvas-aspect` を注入
- [ ] `bindings/Manifest.ts`: ts-rs で自動生成 → コミット

CSS (themes/base.css):
- [ ] `.peitho-slide { width: 1280px; height: 720px }` → `width: var(--peitho-canvas-width, 1280px); height: var(--peitho-canvas-height, 720px);` (フォールバックは 16:9)

Examples:
- [ ] `examples/` に `aspect-ratio-4-3/` を追加(smoke test 兼ドキュメント)

Docs:
- [ ] `CLAUDE.md` の frontmatter キー列挙に `aspect_ratio` を追記したいが、Codex はこのブランチでは `CLAUDE.md` を編集しない。memo-worthy invariant として報告する
- [ ] このプランファイル

## Test plan (TDD order)

Red → Green → Refactor を Codex に指示。順序:

1. **AspectRatio enum の単体テスト** (`peitho-core/src/domain.rs`): `Ratio16To9.width() == 1280`, `Ratio4To3.height() == 720`, `Default::default() == Ratio16To9`, `FromStr` が `"16:9"` / `"4:3"` を受ける
2. **Frontmatter parser のテスト** (`peitho-core/src/parser.rs`):
   - `aspect_ratio: 16:9` → `DeckSettings::aspect_ratio() == Ratio16To9`
   - `aspect_ratio: 4:3` → `DeckSettings::aspect_ratio() == Ratio4To3`
   - 未指定 → デフォルト 16:9
   - `aspect_ratio:` → line-numbered build error(`aspect_ratio has no value`)
   - `aspect_ratio: 16:10` → line-numbered build error(`error.line == 該当行`)
   - `aspect_ratio: 1920x1080` → line-numbered build error。エラーメッセージは受け付ける値の列挙のみ(`use one of: 16:9, 4:3`)。`resolution:` については何も匂わせない(#109 が landed するまで前触れするのは弱いコミットメントになる)
3. **Manifest 出力のテスト** (`peitho-core/src/manifest.rs`): `aspectRatio`, `canvasWidth`, `canvasHeight` が出る、値が正しい。contradictory wire (`"aspectRatio":"4:3","canvasWidth":1280`) は deserialize 後に `canvas_width() == 960`
4. **Render output のテスト** (`peitho-core/src/render.rs`): `render_distribution_index(4:3)` の HTML/JS に `CANVAS_WIDTH = 960` が入る
5. **TS canvas.ts のテスト** (vitest): `calculateCanvasFit` が `canvasWidth`/`canvasHeight` を尊重する(既存テストで OK か確認、なければ追加)
6. **CSS カスタムプロパティのテスト** (`peitho/tests/build.rs`): `.peitho-slide` に CSS 変数フォールバックが入っている
7. **E2E**: `examples/aspect-ratio-4-3/` を `peitho build` して、`dist/index.html` の CANVAS_WIDTH が 960、manifest.json の `aspectRatio` が `"4:3"`、`canvasWidth` が `960`

## Root-cause self-check

- **Silent path**: 不正値は全て line-numbered build error。`AspectRatio` は enum なので外部 crate も合法 variant 以外を構築できない → OK
- **Long-term view**: 将来 `16:10` を追加する時は enum variant、serde label、`width()`/`height()`/`css_aspect_value()`/`FromStr` を更新する。parser は `FromStr` に委譲するので parser 側の label list は増えない → OK
- **Type safety**: `AspectRatio` は enum なので、生の `(u32, u32)` タプルとして誤って比較される経路はない。`Manifest` 内部も canvas width/height を保存せず、`AspectRatio` から導出する → OK
- **単一情報源**: Rust 側の `AspectRatio` が真、manifest 経由で TS/CSS に伝搬。「1280 を 3 箇所書き直す」パターンから脱却 → OK

## Verification gates

CLAUDE.md の必須ゲート:
- `cargo test --workspace` (3 回連続)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/`
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js`

E2E:
- `examples/aspect-ratio-4-3/` を `peitho build` し、`dist/index.html` を実ブラウザで開いて 4:3 スライドが正しく表示されることを確認(過去に「フル黒画面」インシデントがあるため必須)

## Shipped divergence / adjustments during review (2026-07-05)

1. `AspectRatio` は newtype ではなく enum として shipped。`Ratio16To9` / `Ratio4To3` だけを合法値にし、serde rename (`"16:9"` / `"4:3"`) と `FromStr` を `domain.rs` に集約した。これにより、frontmatter parser は label list を持たず `AspectRatio` の parsing に委譲する。
2. Manifest wire form は `{width,height}` object ではなく、`"aspectRatio": "16:9"` と derived numeric fields (`"canvasWidth": 1280`, `"canvasHeight": 720`) にした。Rust 内部の `Manifest` は `AspectRatio` だけを保持し、private `ManifestWire` が serialization/deserialization の JSON shape を担当する。
3. Manifest deserialize は accept-and-drop。`canvasWidth` / `canvasHeight` が JSON にあっても authoritative なのは `aspectRatio`。矛盾する JSON (`"aspectRatio":"4:3","canvasWidth":1280`) は deserialize 後に `canvas_width() == 960` になる。
4. `packages/peitho-present/src/canvas.ts` の `CANVAS_WIDTH` / `CANVAS_HEIGHT` exports と `?? CANVAS_WIDTH` / `?? CANVAS_HEIGHT` fallback は削除した。`installCanvasScaler` は `canvasWidth` / `canvasHeight` を required option にする。caller が dimensions を渡し忘れて 16:9 に戻る convention-only seam を残さないため。
5. Presenter view も aspect-ratio aware にした。`render.rs` は standalone / present / presenter templates の `:root` に `--peitho-canvas-aspect` を埋め込み、presenter の `.stage` / `.slide-pane` / `.next-preview` は `var(--peitho-canvas-aspect)` を使う。`shell.ts` も shell root に同じ CSS variable を設定する。
