# Slide plain text in manifest.json (Issue #136)

## Summary

`manifest.json`の各`slides[i]`に、そのスライドのtitle/body/codeの**平文テキスト**を新フィールド`text: {title, body, code}`として含める。decksクローラー、検索インデックス、アクセシビリティツールが`dist/*/slides/*.html`をregexパースする「altitude inversion」を、peithoが構造化データを出すことで根本解決する。

## Motivation

decks (`mizzy/decks` #16) はSEO対応でクローラー向けにスライド本文を露出したい。現状のmanifest.jsonにはスライドのテキスト自体は含まれておらず、decks側で`dist/<slug>/slides/*.html`フラグメントを正規表現でパースしてタイトル/本文/コードを抜き出すことになる。これは典型的なaltitude inversion(下流が上流の出力を再パース)で、以下の脆さがある:

- `class="slot-title"`が単引用符になるだけでマッチ失敗
- コードブロックの改行が`\s+`で潰れて`<pre>`に入れると1行になる
- HTMLエンティティのdecoderがpeitho出力の全パターンをカバーできる保証がない
- レイアウトHTMLの変更(slot名変更、新slot追加)でdecks側の抽出が黙って壊れる

deck.mdをASTとして持つpeitho自身が構造化データを出す方が根本解決。ちょうど`Parsed → Mapped → Checked`と段階変換していく過程で、Checkedフェーズはslot契約解決済みのタイプ付き構造(`BTreeMap<SlotName, Vec<SourceFragment<ResolvedImagePath>>>`)を持っていて、そこに平文テキストが既に載っている(`SourceFragment::plain_text()`、`SourceFragment::code_text()`、`heading_text()`)。ここからHTMLシリアライズと並行して平文シリアライズを生やす。

## Design decisions

### Field shape: `text: { title, body, code }` per slide

Issue本文の提案に沿って、`ManifestSlide`に新フィールドを追加:

```json
{
  "index": 0,
  "key": "cover",
  "src": "slides/000-cover.html",
  "hasNotes": true,
  "text": {
    "title": "Peitho",
    "body": "",
    "code": ""
  }
}
```

- **`title`**: 全`slot-title`スロット(通常1つ)のheadingsから平文テキストを取り、複数あれば改行区切りで結合。Markdownインライン記法は文字化済み(`**bold**` → `bold`)
- **`body`**: 全`slot-body`スロット(または`slot-body-*`のように接尾辞付きも含む)の`Paragraph`と`List`から平文テキストを取り、改行区切りで結合
- **`code`**: 全`slot-code`スロットの`Code`から**改行保持の生ソース**を取り、複数コードブロックがあれば空行区切りで結合。syntax highlightのspanは含まない
- スロットに該当コンテンツが無ければ空文字列。`text`オブジェクト自体は常に存在(常に3キー全部あり)

**「slot-title / slot-body / slot-code」の判定はslot名文字列ベース**。理由:

- `mapping.rs`のconvention mappingは`heading` → `title`、`paragraph`/`list` → `body`、`code` → `code`、`image` → `image`という名前ベースの規約(§X §Y §Zの三本柱の②「レイアウトが契約」の下位規約)。この規約に乗っている既存contentは、slot名と入っているfragment kindが1対1に対応している
- 明示スロット構文`::: {slot=name}`で作者が別名スロットに詰めた場合は、そのslot名が`title`/`body`/`code`と一致していなければ`text.title/body/code`には含まれない。これは意図した挙動(明示スロットで書いた作者は、convention nameへの写像を望んでいない)
- Issue本文の提案は「`slot-title`スロットに入る内容」「`slot-body`スロットに入る内容」「`slot-code`スロットに入る内容」と、明確にslot名ベースで書かれている

### Where to convert: new module `plain.rs` in peitho-core

`crates/peitho-core/src/plain.rs`(新設)に、Checkedスライドから平文テキストを取り出す関数を置く。

```rust
pub struct SlideText {
    pub title: String,
    pub body: String,
    pub code: String,
}

pub fn slide_text<S>(slide: &CheckedSlide<S>) -> SlideText { ... }
```

- `render.rs`はHTMLシリアライザ、`plain.rs`は平文シリアライザ、と役割を分離
- `manifest.rs`が`plain.rs`を呼んで`ManifestSlideText`を組み立てる
- 純関数なのでユニットテストが素直

### Extract logic per slot

すべての判定はslot名文字列で行う:

1. **title**: `slot.as_str() == "title"`のスロットを探し、その中の全fragment `f`について:
   - `f.kind()`が`Heading`なら`f.plain_text()`(既に`heading_text()`と同じフィールド`text`)
   - それ以外は無視
   - 複数あれば`"\n"`区切り

2. **body**: `slot.as_str() == "body"`のスロットを探し、その中の全fragmentについて:
   - `f.kind()`が`Paragraph`または`List`なら、`f.markdown()`から**インライン記法を剥がした平文**を返す
   - Text kindは実質未使用(既存コード上、`text`が空)なので無視
   - Imageは`alt`テキストは含めない(v1、シンプルさ優先)
   - 複数あれば`"\n"`区切り

3. **code**: `slot.as_str() == "code"`のスロットを探し、その中の全fragmentについて:
   - `f.kind()`が`Code`なら`f.code_text()`(改行保持のraw source)
   - 複数のコードブロックは`"\n\n"`(空行1行)区切り

### Paragraph / Listのインライン記法剥がし

`SourceFragment::paragraph(...)`と`SourceFragment::list(...)`は`markdown: raw`、`text: ""`で作られている(parserで確認済み、`domain.rs` line 521-541)。つまり`plain_text()`は空文字列を返す。今回`plain_text()`はText kindのためのフィールドなのでそのまま。

`body`の平文化は「markdown文字列をpulldown-cmarkでinlineパースして`Event::Text`のみ拾う」実装を新しく`plain.rs`に置く。既存の`render_heading_inline`(`render.rs`)がheading markdownをHTMLに落とすときの構造とパラレルで、こちらは`Event::Text`だけを`String`に押し込む形。

- リスト項目間は`"\n"`区切り
- Paragraph内のsoft breakは半角スペースに正規化(現行のMarkdownレンダの通常挙動と揃える。`\n`はfragment区切り専用にする)
- コードspan(`` `foo` ``)は`Event::Code`を拾って中身をそのまま含める
- リンク`[text](url)`は`Event::Text`だけ拾うのでlink textのみ残る(URLは落ちる) — SEO/クローラーの意図と整合
- 画像は`Event::Text`が来ないので空になる(alt textは`Tag::Image`の中にあるが、V1では拾わない — 単純さ優先。Body側の画像は既にimage slotに分離されているのが典型なので、body slotに埋め込まれた画像は稀)

### Serialization: additive, no version bump

- `ManifestSlide`に`text: ManifestSlideText`を追加、`serde(rename = "text")`
- `ManifestSlideText { title: String, body: String, code: String }`
- 常時全3キー出力(空文字列でもキー自体は残す)。理由: consumer側で`text?.title ?? ''`のような分岐が要らなくなり、契約が単純
- **`manifest.version`は上げない**。理由:
  - 既存のnullable/optional追加(`sections`、`images`、`aspectRatio`など)は`version`を上げていない
  - `text`は純粋にadditive、下流consumerは`slides[i].text`を無視すればこれまでどおり動く
  - decks側は現状`version`を見ていない(Issue本文の記述)
- **manifest deserialize時に`text`欠落は`text: {title:"", body:"", code:""}`にフォールバック**する(過去のバージョンで書かれたmanifestを`peitho publish`が読めるように)
- 新しい`ManifestSlideText`構造体は`ts-rs`でTS binding(`bindings/ManifestSlideText.ts`)を吐かせる。既存のCI drift checkに乗る

### `text` name vs `plainText` name

Issue本文でも「`text`でなく`plainText`など別案でもよい」と余地があるが、`text`を採用する。理由:

- 既存フィールド(`title`, `slide_count`, `plannedDurationMs`など)と並ぶ短さ
- consumer側のコードで`slide.text.title`という自然な読み方ができる
- HTMLも「text」だが、隣接する`src`が指す先はHTML、というのは文脈で明確

## Non-goals

- **Image alt text**は`text.body`に含めない(v1)。将来decks側のニーズが出てきたら、別フィールド`text.images: string[]`や`text.body`への統合を検討。V1では単純化優先
- **フォーマット固有の情報**(bullet markの種類、code language、link URL等)は含めない。これはあくまで「クローラーがスライドの意味を読み取るためのplain text」であって、structured contentではない
- **`manifest.version`のbump**(理由は前節)
- **`notes.json`との統合**。notesは`dist/`に入らないinvariantがあるのでmanifestには載らない。text.notesという扱いには**しない**

## Type-safety self-check (CLAUDE.md rule)

- 新関数`plain::slide_text`は`CheckedSlide<S>`(**Checked**フェーズのslide)しか受け付けない。Parsed/Mapped段階での呼び出しは型で不可能。これは既存のtypestate契約と揃う
- 空スロット(slot自体存在しない)と、slotは存在するが該当kindのfragmentがない場合の両方で空文字列。両者は「テキスト無し」と等価なので分岐は無し
- 新フィールドのdeserialize欠落フォールバックは`#[serde(default)]`+`Default for ManifestSlideText`で表現(runtime patchではなく型で保証)

## Test plan (TDD)

TDDで実装。`plain.rs`のユニットテスト → `manifest.rs`統合テスト → binding drift、の順。

### `plain.rs` unit tests

1. **title slot with single heading** → `title = "Peitho"`
2. **title slot with markdown inline (`# **Bold** heading`)** → `title = "Bold heading"`
3. **title slot missing** → `title = ""`
4. **body slot with two paragraphs** → `body = "First paragraph\nSecond paragraph"`
5. **body slot with a list `- item1\n- item2`** → `body = "item1\nitem2"`
6. **body slot with inline code `` `foo` bar``** → `body = "foo bar"`
7. **body slot with link `[click](url)`** → `body = "click"`(URLは含まれない)
8. **body slot with soft break** → `body`内では半角スペース1個
9. **body slot missing** → `body = ""`
10. **code slot with one code block** → `code = "fn main() {}\n"`(改行保持)
11. **code slot with two code blocks** → 空行区切り
12. **code slot missing** → `code = ""`
13. **explicit slot `::: {slot=aside}`** → title/body/codeいずれにも含まれない
14. **mixed title+body+code full slide** → 3フィールドすべて期待通り

### `manifest.rs` integration tests

1. `build_manifest`が各`ManifestSlide`に`text`を埋め込む
2. `manifest_json`のsnapshotが`"text": {...}`を含む
3. 既存の`serializes_manifest_schema_exactly` snapshotを`text`込みに更新
4. `text`フィールドが欠落したlegacy JSONもdeserializeできる(publish validation)

### bindings

`ManifestSlideText.ts`と、`ManifestSlide.ts`の`text: ManifestSlideText`フィールドのexportをCI drift checkに乗せる。既存の`ts_tests::exports_manifest_bindings_with_serde_field_names`にassertionを追加。

## Files touched

- `crates/peitho-core/src/plain.rs`(新規)
- `crates/peitho-core/src/lib.rs`(`mod plain;`追加)
- `crates/peitho-core/src/manifest.rs`(`ManifestSlideText`追加、`ManifestSlide::text`フィールド、`build_manifest`で埋める、tests更新)
- `bindings/ManifestSlideText.ts`(自動生成)
- `bindings/ManifestSlide.ts`(自動再生成、`text`追加)
- `docs/plans/2026-07-05-manifest-slide-text.md`(本文書)
- `packages/peitho-present/test/*.test.ts`(6ファイル、`Manifest`型が新たに必須化する`text`フィールドを既存fixtureに追加。ランタイム挙動やshell.jsには影響なし)

TypeScript側の実装(`packages/peitho-present/src/`)は変更なし。presentはmanifestの`text`を読まない。

## Gates (from project CLAUDE.md)

```
cargo test --workspace          # 3回連続
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/  # contract drift
```

`packages/peitho-present`は変更しないが、drift checkのために`npm run build`と`npm test`は走らせる。
