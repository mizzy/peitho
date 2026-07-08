# デモサイト (peitho.gosu.ke) のデプロイフロー整備

日付: 2026-07-09
参考: `mizzy/decks` リポジトリのデプロイフロー (`.github/workflows/deploy.yml` / `preview-cleanup.yml`)

## 現状と問題

- https://peitho.gosu.ke/ (Cloudflare Pagesプロジェクト `peitho`) は `envchain peitho make deploy-demo` の手動デプロイのみ
- examplesやレンダラを変更してもデプロイを忘れるとデモサイトが古いまま残る
- decks.gosu.ke側は既にGitHub Actionsで自動化済みなので、同じ形に揃える

## 方針

decksのフローを移植する。ただしdecksと違いこのリポジトリはpeitho本体なので、
リリースバイナリをダウンロードするのではなく**ソースからビルドした今のpeithoでデモサイトを組む**
(既存の `make demo-site` をそのまま使う。`dist/shell.js` は埋め込み済み・コミット済みなのでnpmビルド不要)。

1. `.github/workflows/deploy-demo.yml`
   - `main` push / `workflow_dispatch` → `make demo-site` → `wrangler pages deploy .demo-site --project-name=peitho --branch=main`
   - `pull_request` (同一リポジトリのみ、forkはSecretsを参照できない) → プレビューデプロイ + PRコメント (decksと同じmarker方式で1コメントを更新)
   - concurrencyはdecks同様 `deploy-demo-<PR番号 or ref>` でgroup化し `cancel-in-progress: true`
   - Rustビルドは `Swatinem/rust-cache` (shared-key: demo) でキャッシュ
2. `.github/workflows/demo-preview-cleanup.yml`
   - PR close時にそのブランチのプレビューデプロイをCloudflare APIで削除 (decksのpreview-cleanup.ymlと同じ。issue mizzy/decks#32のconcurrency知見も踏襲)
3. GitHub Secrets: `CLOUDFLARE_API_TOKEN` (envchain `peitho` のものを登録) / `CLOUDFLARE_ACCOUNT_ID`
   - プロジェクト名 `peitho` はMakefileで既に公開情報なのでワークフローに直書き
4. `make deploy-demo` は手動デプロイ用にそのまま残す
5. CLAUDE.md にデプロイフローを1行追記

## decksから引き継ぐ罠

- Cloudflare API Tokenには `Account: Cloudflare Pages: Edit` / `Account: Account Settings: Read` / `User: User Details: Read` の3つ全部が必要。欠けると `wrangler pages deploy` が `Authentication error [code: 10000]` で落ちる
- preview-cleanupのconcurrency groupはPR番号でgroup化する (`github.ref` はマージ済みPRのclosedイベントでbaseブランチに解決され、本番デプロイのgroupと衝突する)

## 検証

- `make demo-site` がworktreeで通ること (デモサイトの組み立て+publish contamination check)
- workflow YAMLの構文チェック (actionlint があれば)
- マージ後、Actionsの実走行とpeitho.gosu.keの更新を確認
