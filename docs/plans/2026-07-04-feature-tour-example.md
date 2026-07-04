# feature-tour example実装計画 (2026-07-04)

## 動機

既存4例のカバレッジ棚卸しで、どのexampleも使っていない機能が見つかった:

1. 明示レイアウト指定 `{"layout":"name"}`（ハイブリッドディスパッチの最優先規則なのに使用例ゼロ）
2. `accepts="list"` スロット
3. 1スライド複数ノートコメント（空行連結）
4. Rust以外のシンタックスハイライト（全コードブロックが `rust`）
5. インライン装飾（強調・リンク）とネストしたリスト
6. 3枚以上のレイアウト間ディスパッチ

これらを1つのデッキで網羅する5つ目のexample `examples/feature-tour/` を追加する。
題材はpeitho自身の機能紹介（自己言及型・英語）。著者確認済み (2026-07-04)。

なお調査の副産物として、`Accepts::Image`/`Accepts::Text` はパーサがImage/Text
フラグメントを生成する経路が無く（Markdown画像は `unsupported construct` エラー）、
現状Markdownから到達不能な語彙であることを確認した。これはexampleでは埋められない
著者判断マター（画像対応の是非）なので本計画のスコープ外。

## デッキ設計（7スライド・4レイアウト）

frontmatter `time: 8m`。セクション: Basics 2m（S1-2）/ Contracts 3m（S3-4）/
Presenting 3m（S5-7）。合計8m = frontmatter値（パース終端の検証を通す）。

レイアウトと契約:

| layout | slots | 構造マッチする形 |
|---|---|---|
| `cover` | title inline 1 | 見出しのみ |
| `topic` | title inline 1, body blocks 1..* | 見出し+段落/リスト |
| `agenda` | title inline 1, body list 1..* | 見出し+リストのみ |
| `code-demo` | title inline 1, body blocks 0..*, code code 1..* | コードを含む |

**設計の要**: リストのみのスライドは `topic`（blocksはListを受ける）と `agenda` の
両方に構造マッチして曖昧エラーになる。S3はこれを意図的に踏み、
`{"layout":"agenda"}` の明示指定で解決する — 「曖昧は黙って解決しない、明示が要る」
という規則を実デッキで体験させる。

スライド:

1. cover構造マッチ。ノートコメント**2つ**（空行連結の実演）。key+section
2. topic。**強調**・*イタリック*・[リンク]・ネストリストを本文に使用
3. agenda明示指定（上記）。checkが捕まえるものの一覧
4. code-demo。**コードブロック2つ**（rust + typescript）— arity 1..* とts-rs対比の自己言及
5. code-demo。bashのCLI実演（3コマンド）
6. topic。時間管理・セクション・ノートの説明（このデッキ自身の設定を引用）
7. cover。クロージング（ノート無し = presenterのdimmedプレースホルダも見える）

シンタックスハイライト言語: rust / typescript / bash（syntect認識トークン。
未知タグはビルドエラーなので `peitho build` で検証）。

## テーマ

既存4例（ivoryデフォルト/ダークポスター/ターミナル/クリームセリフ）と被らない
「ライトなプロダクトツアー」調: 白背景+インディゴアクセント+システムサンセリフ。
`.peitho-slide` 1280x720規約に従う。

CSS検証の実演:
- `base.css`: bare `.slot-*`（全レイアウトのスロット和集合に対して検証される）
- `overrides.css`: keyed selector 2つ以上（`[data-slide-key="..."]` はそのスライドの
  レイアウトのスロットに対して検証される）

## 変更ファイル

- `examples/feature-tour/deck.md`
- `examples/feature-tour/layouts/{cover,topic,agenda,code-demo}.html`
- `examples/feature-tour/css/{base,overrides}.css`
- `crates/peitho/tests/build.rs` — 既存のlightning-talkパターンに倣い、
  スライド数・sections・notes.jsonを固定する統合テスト
- `Makefile` — `feature-tour`(+`-windowed`)ターゲット、help、DEMO_DECKS、demo-site
- `demo/index.html` — デッキカード追加
- `README.md` — examplesテーブル行 + スクショ1枚
- `scripts/take-screenshots.sh` — DECKSに追加

## ゲート

CLAUDE.mdの全ゲート + `peitho build examples/feature-tour/deck.md` 成功 +
スクショでの目視確認。
