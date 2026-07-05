# Markdown画像対応計画 (Issue #117 / #106)

## 決定事項

- `![alt](path)` の `path` は **デッキファイルからの相対パスのみ**。リモートURL、絶対パス、URL scheme風の値は行番号付きビルドエラー
- MVPで許可する画像拡張子は **`.png` / `.jpg` / `.jpeg` / `.gif` / `.webp`**。大文字小文字は区別せず小文字化して判定する。拡張子無し、`.exe`、`.md`、`.svg` は parser段階で行番号付きビルドエラー
- 画像は **`accepts="image"` のスロットにだけ** 割り当てる。画像を受けるスロットが無いレイアウト、または複数あって convention で一意に決められないレイアウトはビルドエラー
- dist配置は `dist/assets/<hash>-<basename>.<ext>`。コンテンツハッシュで重複除去し、同じ内容は1回だけコピーする
- #106 は同時解消する。`FragmentKind::Image` / `Accepts::Image` を parser → mapping → check → render の実到達経路に乗せる
- `Accepts::Text` はこのPRでは触らない。#106 には「Imageは解消、Textは残り」とコメントする

## 不変条件

- 画像はMarkdown側のコンテンツ。サイズ、配置、トリミングはレイアウトHTML/CSS側に置く
- unknown / unsupported / mixed構造を `_ => {}` で握りつぶさない。未定義ケースは line + help 付きエラーにする
- `Parsed → Mapped → Checked → Rendered` の型境界を守る。`Rendered` へ渡る画像srcは `ResolvedImagePath` だけにし、`RawImagePath` から直接 `<img src>` を生成できない型にする

## 対象コード

- `crates/peitho-core/src/domain.rs`
  - `FragmentKind` を `Image { alt: String, src: S }` を持てる形に変更し、`RawImagePath` / `ResolvedImagePath` / `ResolvedImageAsset` を追加する
  - `SourceFragment::image(line, alt, RawImagePath)` と `image()` accessor を追加する
  - `FragmentKind` は `Copy` を外す。`kind()` は参照返し、`default_accepts()` / `removal_noun()` / `Display` は image payload を無視して動かす
- `crates/peitho-core/src/parser.rs`
  - `unsupported_tag()` / `unsupported_tag_name()` から `Tag::Image { .. }` を外す
  - `OpenBlock::Paragraph` に inline状態を持たせ、画像単独段落だけ `SourceFragment::image` に変換する
  - `RawImagePath::new(raw, line)` を呼び、URL/絶対パス/非許可拡張子を parser段階の行番号付きエラーにする
  - `list_depth > 0` の丸ごと無視より前に `Tag::Image` を検出し、list内画像は当面エラーにする
- `crates/peitho-core/src/mapping.rs`
  - `map_slide()` を image slot選択で失敗し得る形にする (`Result<MappedSlide>`)
  - `FragmentKind::Image { .. }` は `body` に落とさず、`accepts == Accepts::Image` のスロットへだけ割り当てる
- `crates/peitho-core/src/check.rs`
  - 既存 `(Accepts::Image, FragmentKind::Image)` アームを、到達するテストで固定する
  - `FragmentKind` payload化に合わせて全matchを網羅的に更新する
- `crates/peitho-core/src/error.rs`
  - resolve段階のI/O失敗を parser/layout/manifest と混ぜないため、`ErrorKind::Asset` を追加する
- `crates/peitho-core/src/render.rs`
  - `render_deck()` は `Deck<Checked<ResolvedImagePath>>` だけを受ける
  - `render_slot()` / `render_image_fragment()` は `ResolvedImagePath` の `html_src()` だけを使い、altをHTML属性escapeして `<img>` を出す
- `crates/peitho-core/src/manifest.rs`
  - `ManifestImage { src }` と `images: Vec<ManifestImage>` を追加し、`#[serde(default)]` で既存manifestのpublish validationを壊さない
  - `build_manifest()` は解決済み画像asset一覧を受け取るか、解決済みChecked deckから `images` を作る
- `crates/peitho-core/src/lib.rs`
  - CLIが使う `RawImagePath` / `ResolvedImagePath` / `ResolvedImageAsset` / `ManifestImage` / 画像解決関数を公開する
- `crates/peitho/src/main.rs`
  - `build_artifacts()` に `resolve_image_paths()` 呼び出しを挟む
  - CLI側 resolver が deck parent基準で実ファイルを解決し、hashと `assets/...` を決定する
  - `emit_distribution()` / `emit_present_cache()` で `assets/` を作り直して画像をコピーする
  - `validate_publish_dist()` / `read_publish_manifest()` で `manifest.images[*].src` の存在とdist内相対パス性を検証する
- `crates/peitho/tests/{build.rs,publish.rs}`、`crates/peitho-core/src/*` の単体テスト、`examples/`、`bindings/`
  - 画像入りexample、bindings再生成、publish欠落検出を追加する

## 型設計

`RawImagePath` はMarkdownに書かれた値。構築時に「ローカル相対パス」だけを許可する。

```rust
pub struct RawImagePath(String);
impl RawImagePath {
    pub const ALLOWED_EXTENSIONS: &'static [&'static str] = &["png", "jpg", "jpeg", "gif", "webp"];
    pub fn new(raw: impl Into<String>, line: usize) -> Result<Self>;
    pub fn as_str(&self) -> &str;
    pub fn extension(&self) -> &str;
}
```

拡張子チェックは parser段階で強制する。理由は、`foo.exe` / `notes.md` をresolverでcopyしてからChromeのMIME解釈に任せると、HTML生成までは成功する silent path になるため。SVGは将来扱いを決める余地を残すが、MVPでは `.svg` を明示拒否する。

`ResolvedImagePath` はHTMLに書いてよいdist相対srcだけを公開する。source pathはcopy用asset側に分ける。

```rust
pub struct ResolvedImagePath(String); // "assets/<hash>-<basename>.<ext>"
pub struct ResolvedImageAsset {
    pub source_abs: PathBuf,
    pub dist_rel: ResolvedImagePath,
}
```

`SourceFragment` / `FragmentKind` は画像src型でgenericにする方針。

```rust
pub enum FragmentKind<S = RawImagePath> {
    Heading { level: u8 },
    Paragraph,
    Text,
    Code,
    Image { alt: String, src: S },
    List,
}
```

Coreの境界:

```rust
pub fn resolve_image_paths<R>(
    deck: Deck<Checked<RawImagePath>>,
    resolver: R,
) -> Result<(Deck<Checked<ResolvedImagePath>>, Vec<ResolvedImageAsset>)>
where
    R: FnMut(ImageRequest<'_>) -> Result<ResolvedImageAsset>;

pub fn render_deck(deck: Deck<Checked<ResolvedImagePath>>) -> Result<Deck<Rendered>>;
```

これで `<img src>` を生成する `render_image_fragment()` は `ResolvedImagePath` しか受け取れない。CLIはresolverを注入するが、core自身はfilesystemやcopy副作用を持たない。

## エラー流路

全層で `peitho_core::BuildError` を返し、CLIは既存の `core(result)` で `miette` diagnosticへ統一変換する。

- parser段階: URL/absolute/非許可拡張子、混在段落、list内画像は `ErrorKind::Parse`
- mapping段階: image slot無し、複数image slotでconvention不可能、structural match不成立/曖昧は `ErrorKind::Layout`
- resolve段階: ファイル不在、permission denied、hash計算のread失敗は新設 `ErrorKind::Asset`

resolverの型は `BuildError` 固定にする。

```rust
pub struct ImageRequest<'a> {
    pub raw: &'a RawImagePath,
    pub line: usize,
}

pub fn resolve_image_paths<R>(
    deck: Deck<Checked<RawImagePath>>,
    resolver: R,
) -> Result<(Deck<Checked<ResolvedImagePath>>, Vec<ResolvedImageAsset>)>
where
    R: FnMut(ImageRequest<'_>) -> crate::Result<ResolvedImageAsset>;
```

CLI resolverは `fs::metadata` / `fs::read` / `fs::canonicalize` の `io::Error` をその場で `BuildError::new(ErrorKind::Asset, Some(request.line), ..., ...)` に変換する。`resolve_image_paths()` は checked slideを巡回しているので、resolverから返った `BuildError` に slide number/key を付けて返す。`io::Error` や `miette::Report` はcore境界を越えない。

## parserのイベント処理

pulldown-cmark 0.13 の本文parser (`parser_options`) だけを変更する。frontmatter検出とslide split用grammarは現状どおり分離し、metadata block設定をsplit側へ混ぜない。

画像単独段落:

```text
Start(Paragraph)
Start(Tag::Image { dest_url, title, id, .. })
Text("alt")
End(TagEnd::Image)
End(Paragraph)
```

これは `SourceFragment::image(paragraph_start_line, alt, RawImagePath::new(dest_url, line))` にする。Paragraphフラグメントは作らない。

段落内テキストのみ:

```text
Start(Paragraph)
Text(...)
End(Paragraph)
```

現状どおり `SourceFragment::paragraph`。

混在段落:

```text
Start(Paragraph)
Text("before ")
Start(Tag::Image { .. })
Text("alt")
End(TagEnd::Image)
Text(" after")
End(Paragraph)
```

当面は `unsupported construct 'mixed image paragraph'` でエラー。lineは段落開始または画像開始の早い方、helpは「画像だけの段落に分ける」。複数画像を1段落に並べるケースも同じくエラーにし、複数画像が必要なら別段落で複数Imageフラグメントにする。

alt収集は `Text` / `Code` / break を平文に畳む。image内で未対応tagが来たらエラーにし、外側の `Event::Start(Tag::Emphasis | Tag::Strong | Tag::Link)` の既存無視アームで誤って飲み込まない。

list内画像:

```text
Start(List)
Start(Item)
Start(Paragraph)
Start(Tag::Image { .. })
...
```

現状のlist処理は `list_depth > 0` で内部イベントを見ず、元Markdownを `SourceFragment::list` として保持する。ここで画像を許すと後段の `html::push_html` が raw path の `<img>` を生成できてしまうため、当面は `unsupported construct 'image inside list'` として明示エラーにする。

## ParagraphではなくImageに置き換える理由

段落内inlineとして扱うと `FragmentKind::Paragraph` のまま `blocks/body` に流れ、著者判断の「imageスロットのみ」を型チェックできない。画像単独段落を `FragmentKind::Image { alt, src }` に置き換えることで、mapping/checkが `Accepts::Image` 契約を直接検査できる。混在段落はinline画像の設計が決まるまで明示エラーにする。

## dispatch / mapping fallout

公開関数 `dispatch_by_convention()` と `map_by_convention()` は現状すでに `Result<Deck<Mapped>>` を返しているため、CLI (`crates/peitho/src/main.rs::build_artifacts`) と外部export (`crates/peitho-core/src/lib.rs`) のシグネチャは変わらない。変更するのは private `map_slide()` の戻り値を `Result<MappedSlide>` にする点と、その呼び出し側だけ。

影響範囲:

- `crates/peitho-core/src/mapping.rs::dispatch_slide`
  - explicit layout override: `map_slide(&slide, layout)?`。image slotエラーは指定layoutの確定エラーで、他layoutへfallbackしない
  - single layout: `map_slide(&slide, layout)?`。既存どおり、そのlayoutに対する最短エラーを返す
  - multi layout structural probe: layoutごとに `map_slide()` を試し、`map_slide` error または `check_slide` error をそのlayoutのrejectionとして扱う
- `crates/peitho-core/src/mapping.rs` tests
  - 既存dispatch testsは公開関数の `unwrap()` 形を維持できる。画像ありの structural match testsを追加する
- `crates/peitho-core/src/check.rs` / `render.rs` / `manifest.rs` tests
  - `map_by_convention(...).unwrap()` の呼び出しはシグネチャ上そのまま。`FragmentKind` payload化に伴うmatch修正だけが主なfallout
- `crates/peitho/src/main.rs::build_artifacts`
  - `core(peitho_core::dispatch_by_convention(parsed, &layouts))?` はそのまま。新しいmappingエラーも既存流路でmietteへ出る

structural matchへの画像影響:

- 画像を含むslide + image slotを持つlayout1つ + 持たないlayout複数 → image layoutだけがmatch
- 画像を含むslide + image slotを持つlayout複数 → ambiguous layout error
- 画像を含むslide + image slotを持つlayoutゼロ → no layout matches
- 画像を含まないslide → 既存の title/body/code/list 判定から変えない
- explicit layout指定時は structural probeを使わず、指定layoutにimage slotが無ければその場でlayout error

## アセット副作用フェーズ

build/present 共通pipeline:

```text
read markdown
parse_markdown
dispatch_by_convention
check_deck
resolve_image_paths(core traversal + CLI resolver)
build_manifest(images付き)
build_theme_css
render_deck(resolved only)
emit_distribution / emit_present_cache(copy assets)
```

CLI resolverの責務:

- `input.parent()` をdeck基準ディレクトリにする
- `deck_dir.join(raw.as_str())` を実ファイルへ解決し、存在しなければ raw path の行番号付きビルドエラー
- 拡張子は parserで保証済みなので、resolverは原則再判定しない。ただし `ResolvedImagePath::new` で `assets/` 配下のdist相対パスだけを受け付ける
- ファイル内容をhashし、`assets/<hash>-<basename>.<ext>` を返す
- hashごとの `ResolvedImageAsset` を `BTreeMap` 等で重複除去する。違うbasenameでも同一contentなら最初のdist名を再利用する

emit側の責務:

- `write_slide_fragments()` と同じく、`assets/` は毎回作り直してstale imageを残さない
- `emit_distribution()` と `emit_present_cache()` の両方で `copy_image_assets(out_or_cache, &artifacts.image_assets)` を呼ぶ
- `BuildArtifacts` に `image_assets: Vec<ResolvedImageAsset>` を追加する

## TDDタスクリスト

| Red test | Green production change | silent-drop対抗 |
|---|---|---|
| `parses_standalone_image_paragraph_as_image_fragment` | `domain.rs` に `RawImagePath` / `FragmentKind::Image { alt, src }` / `SourceFragment::image`、`parser.rs` に image単独段落処理 | `Tag::Image` を `unsupported_tag` から外した直後に専用matchを追加し、fallbackの `_` に流さない |
| `rejects_remote_image_url_with_line` / `rejects_absolute_image_path_with_line` | `RawImagePath::new` で scheme、`//`、absolute/root/prefix componentを拒否 | URLをParagraph markdownとして残さず parse error にする |
| `rejects_image_without_supported_extension` / `rejects_svg_until_policy_is_decided` | `RawImagePath::new` で許可拡張子 `.png/.jpg/.jpeg/.gif/.webp` を強制 | 非画像ファイルをassetsへcopyしてChrome失敗に遅延させない |
| `rejects_text_and_image_mixed_in_one_paragraph` / `rejects_two_images_in_one_paragraph_until_inline_design_exists` | `OpenBlock::Paragraph` に inline状態 (`Empty/TextOnly/PendingImage/SingleImage/Mixed`) を持たせる | mixedをParagraph化して `blocks` に流さない |
| `rejects_image_inside_list_before_markdown_rerender` | `parser.rs` で `list_depth > 0` のignoreより前に `Tag::Image` を検出してエラー | list markdownの後段レンダリングで raw `<img src>` を作らせない |
| `maps_image_to_unique_image_accepting_slot` | `mapping.rs::map_slide` が layout contractから唯一の `Accepts::Image` slotを選ぶ | `FragmentKind::Image` を `body` armから削除する |
| `rejects_image_when_layout_has_no_image_slot` / `rejects_image_when_multiple_image_slots_are_ambiguous` | `dispatch_slide` / `map_slide` を `Result` 化し、0件/複数件をline付きLayoutエラーにする | 「missing body」等の誤ったResidualContentにしない |
| `dispatch_selects_layout_with_image_slot_as_unique_structural_match` / `dispatch_rejects_two_image_layout_matches` | multi layout probeで `map_slide` errorをrejectionに含める | image slot要否を structural match の一意性判定から漏らさない |
| `check_accepts_image_fragment_in_image_slot` | `check.rs::accepts_fragment` の既存Image armをpayload対応に直す | `Accepts::Blocks` が imageを受けないことも同時にassertする |
| `render_deck_requires_resolved_image_paths` | `Deck<Checked<RawImagePath>>` から `render_deck` を呼べないcompile_fail doctest、`resolve_image_paths` 追加 | raw pathをrender関数の型引数に通さない |
| `renders_image_with_resolved_src_and_escaped_alt` | `render.rs::render_slot` に image branch、`render_image_fragment(&FragmentKind<ResolvedImagePath>)` を追加 | Markdown再レンダリングに任せず、`RawImagePath` accessorをrender側へ公開しない |
| `build_copies_markdown_image_to_dist_assets` | `main.rs::build_artifacts` にCLI resolver注入、`emit_distribution` に `copy_image_assets` | HTML内に元の `images/foo.png` が残っていないこともassertする |
| `build_fails_for_missing_markdown_image_with_line_and_help` / `build_fails_for_unreadable_markdown_image_with_line_and_help` | CLI resolverがI/Oエラーを `BuildError(ErrorKind::Asset)` に変換し、coreがslide contextを付ける | missing/permission errorをcopy時panicやpublish時欠落に遅延させない |
| `build_deduplicates_images_by_content_hash` | resolverがcontent hash mapを持ち、同じ内容へ同じ `ResolvedImagePath` を返す | basename違いで二重copyしない |
| `manifest_serializes_images_array` / `deserializes_manifest_missing_images_as_empty` | `manifest.rs` に `ManifestImage` / `images`、bindings test更新 | 旧manifest publish validationを壊さず、画像ありmanifestは必ず列挙する |
| `publish_rejects_missing_manifest_image_reference` / `publish_rejects_manifest_image_reference_outside_dist` | `validate_manifest_image_refs` を追加し、slide src検証helperを共有する | publish commandへ欠落distを渡さない |
| `present_cache_copies_markdown_images` | `emit_present_cache` も `copy_image_assets` を呼ぶ | `peitho present` だけ画像が404になる経路を潰す |
| `feature_tour_or_markdown_image_example_builds` | `examples/` に image slot layout、PNG fixture、deckを追加 | example HTMLとmanifestで raw path不在・assets存在をassertする |

## manifest / publish

`manifest.json` は additive にする。

```json
{
  "images": [
    { "src": "assets/4f8c...-diagram.png" }
  ]
}
```

- `images` は常にserialize、deserializeは `#[serde(default)]`
- publish validationは `slides[*].src` と同じ規則で `images[*].src` を検査する: 空文字、absolute、`..`、root/prefix componentはエラー
- `dist/assets/` 自体は画像が無いdeckでは不要。`images` が非空なら各srcの実ファイル存在を必須にする

## bindings / shell / gates

- `Manifest` に `images`、`ManifestImage` を追加するので `bindings/Manifest.ts` と新規 `bindings/ManifestImage.ts` を再生成してコミットする
- `FragmentKind` は現状TS export対象ではないが、domain generic化でbindings testが壊れないことを確認する
- present shellはmanifest型を読むので `packages/peitho-present` の typecheck を必ず通す。runtimeで images を使わないなら `shell.js` 差分はゼロ想定だが、drift gateは必須

Gate:

```text
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
```

画像入りexampleについては `peitho build examples/<image-example>/deck.md` で `dist/assets/`、slide HTML、manifestを確認する。

## Undecided

- 混在段落 (`text ![alt](x.png) text`) を将来Paragraph inlineとして扱うか、slot指定記法と一緒に設計するか
- SVGを将来 `<img>` のopaque assetとして許可するか、sanitize/拒否を続けるか。MVPでは `.svg` は許可リスト外として明示拒否する
- 画像サイズhint (`![alt](x.png){width=...}` 等) をMarkdown側に置くか、CSSだけに寄せるか
- 複数 `accepts="image"` slot があるlayoutで、Markdownからどのslotへ入れるかを指定する記法
- alt内Markdown装飾をどこまで平文化するか
- hash算出後からemit copyまでに画像ファイルが変わるTOCTOUを、asset bytes保持やopen file handle設計で潰すか
- `--watch` で参照画像の変更を動的watchする実装方式。最低限、画像変更がsilent staleにならないテストを追加してから判断する
