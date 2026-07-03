# デフォルトテンプレート・テーマのバイナリ内蔵

## 目的

`--template`/`--base-css`のデフォルトがリポジトリ相対パス（`templates/`・`themes/`）なので、インストールしたバイナリをデッキだけのディレクトリで実行すると即file not foundになる。デフォルトアセットをバイナリに内蔵し、`peitho build deck.md`をどこからでも動くようにする。

## 方針

- `include_str!`でビルド時に`templates/title-body-code.html`と`themes/base.css`を埋め込む。**リポジトリのファイルが単一ソースのまま**（コンパイル時に同一ファイルを取り込むためdrift不可能）
- CLIの`--template`/`--base-css`/`--overrides-css`は`Option<PathBuf>`にし、未指定なら内蔵デフォルト（overridesは空文字列）。パスを渡せば従来どおりファイル優先
- `--watch`は明示されたファイルだけを監視する（内蔵アセットに監視対象は存在しない）
- shell.js（present用TSバンドル）はnpmビルド生成物で、内蔵するにはRust/npmのビルドパイプライン統合が要るため対象外（従来どおり`--shell`/リポジトリ相対デフォルト）

## 検証

- 単体: watch_pathsのOption対応、CLIパース（未指定=None/指定=Some）
- E2E: 一時ディレクトリにdeck.mdだけ置いて`peitho build deck.md`が成功し、出力のpeitho.cssが内蔵baseテーマと一致することを確認
