# 複数レイアウトとハイブリッドディスパッチ

## 目的

現状は1ビルド=レイアウト1枚で、デッキ内でレイアウトを切り替えられない（§18未決定事項だった）。著者判断が出た:

- 方式は**ハイブリッド**: 基本は型駆動（スライドの内容の形とスロット契約の構造マッチ）で自動選択し、曖昧なときはビルドエラーにして明示指定で解消させる
- [k1LoW/deck](https://github.com/k1LoW/deck)を参考にする（ページ設定はHTMLコメント内JSON、`layout`キーで明示指定。deckのCEL式defaultsに相当する部分を、peithoではスロット契約の構造マッチで置き換える）
- 用語は`--template`ではなく`--layout`に改める

## Phase A: リネーム（先行PR）

deckに合わせ、ユーザー向け用語を「レイアウト」に統一する。

- CLI: `--template` → `--layout`
- ディレクトリ: `templates/` → `layouts/`、サンプルの`template.html` → `layout.html`
- コア: `Template`型 → `Layout`、`parse_template` → `parse_layout`、`template.rs` → `layout.rs`（エラーメッセージは元々"layout"表記なので整合する）
- README / CLAUDE.md / Makefile追随。過去のdocs/plans/は履歴なので書き換えない

## Phase B: 複数レイアウト+ディスパッチ

### 構文（deck互換のページ設定コメント）

```markdown
<!-- {"key":"cover","layout":"cover"} -->
```

`layout`は任意。指定があればそのレイアウトを使う（未知の名前は既知レイアウト一覧つきビルドエラー）。

### CLI

- `--layout <path>`を複数回指定可能（`Vec<PathBuf>`）。レイアウト名はファイルstem。名前重複はエラー
- 未指定なら従来どおり内蔵`title-body-code`1枚

### ディスパッチ規則（決定論的）

1. スライドに明示`layout`があればそれを使用。契約違反は従来どおりの行番号付きエラー
2. レイアウトが1枚しかなければ無条件にそれを使用（現行動作の完全保存。エラーメッセージも従来のまま）
3. 複数枚で明示なしの場合、各レイアウトに対して規約マッピング+契約検査（accepts/arity/未割当）を試行:
   - ちょうど1枚通る → それを採用
   - 複数通る → 曖昧エラー（候補一覧+「`{"layout":"…"}`で明示せよ」help）
   - 0枚 → 全滅エラー（レイアウトごとの不一致理由を列挙）
4. 順序はCLI指定順で安定

### 型の通し方

`MappedSlide`が自分の`Layout`を保持する（dispatch時に解決済みのcloneを持たせる）。check/renderはレジストリ再参照をしない=後段でのlookup失敗経路を作らない。typestate `Parsed→Mapped→Checked→Rendered`は不変。

### テーマ検証

overrides.cssの`[data-slide-key="k"] .slot-x`は「スライドkのレイアウトが持つスロット」に対して検証。キー無しセレクタのスロットクラスは全レイアウトの和集合に対して検証。

### サンプル

keynoteを2レイアウト構成にする: `cover.html`（titleのみ）と`statement.html`（title+body必須）。表紙はタイトルだけ→型駆動でcoverに、本文スライドはstatementに一意に落ちる（構造マッチの実演）。明示指定の構文はREADMEに記載。

## 検証

- 単体: 明示/一意/曖昧/全滅/1枚時の完全互換、パーサのlayoutフィールド、テーマのレイアウト別検証
- E2E: keynote 2レイアウト構成をビルド+実ブラウザで確認。既存サンプル・既存テストが無変更のまま通ること（1枚時の互換保証）
