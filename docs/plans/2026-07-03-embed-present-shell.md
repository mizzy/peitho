# 発表シェル(shell.js)のバイナリ内蔵

## 目的

テンプレート・テーマ内蔵後も、`present`だけは`packages/peitho-present/dist/shell.js`というリポジトリ相対パスに依存しており、インストールしたバイナリでリポジトリ外から`peitho present`できなかった。シェルも内蔵して3コマンド全てをスタンドアロンにする。

## 方針: bindings/と同じ「生成物をコミット+CI drift検査」

- build.rsでnpmを叩く案は不採用: `cargo build`にnode/npm依存を持ち込み、純Rustビルドを壊す
- 採用: `dist/shell.js`をコミットし、`include_str!`で内蔵。CIのnodeジョブが`npm run build`後に`git diff --exit-code dist/shell.js`でdriftを検査（bindings/のTS型と同じ規律。esbuildはpackage-lockで固定されるため出力は決定的）
- `.gitignore`は`dist/`→`dist/*`+`!dist/shell.js`に変更（親ディレクトリ除外だと再includeが効かないgitの仕様のため）。sourcemapはignoreのまま
- `--shell`は`Option<PathBuf>`化。未指定=内蔵シェルをpresent-cacheへ書き出し、指定=従来どおりファイルコピー（"shell bundle not found"エラーは明示パス時のみ）
- 開発フロー: TSを直したら`npm run build`→`include_str!`の依存追跡でcargoが再コンパイルし内蔵も更新される。Makefileの`shell`依存は従来どおり

## 検証

- 単体/統合: `--shell`未指定でpresent-cacheのshell.jsが内蔵内容で生成されること、明示パスの従来動作、missing時のエラーが明示パス限定になること
- E2E: リポジトリ外の一時ディレクトリで`peitho present deck.md --no-open --port固定`が起動し、/shell.jsが配信されることを確認
