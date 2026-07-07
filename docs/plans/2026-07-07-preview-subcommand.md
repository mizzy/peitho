# `peitho preview` — 編集ループの1コマンド化 (Issue #170)

設計確定コメント: https://github.com/mizzy/peitho/issues/170#issuecomment-4904080199

## ゴール

`peitho preview [deck.md]` 一発でwatch + 再ビルド + 配信 + ブラウザ自動リロードの編集ループが回る。プレビューページはDeckset的に「単一スライド大表示 ⇄ 全スライドタイル一覧」を`o`で行き来できる。

## スコープ外

- 発表者ビュー付き編集ループ (`present --watch`、案C) — 別issue
- スピーカーノートのpreview表示
- フラグメント差し替えによる部分更新 (v1はフルリロード + 状態復元)

## アーキテクチャ

```
peitho preview deck.md
  ├─ watch登録 (build --watchと共有の配管。初回ビルドより前に登録し、起動中の保存の取りこぼしを塞ぐ)
  ├─ build_artifacts → .peitho/preview-cache/build-<generation>/ にemit
  │    fragments + manifest.json + peitho.css + assets/ + fonts/ + index.html(previewシェル入り) + preview.js
  ├─ PresentServerで配信 (rootは世代ディレクトリ。emit完了後にswap、直前世代は in-flight リクエスト用に1つ保持)
  ├─ デフォルトブラウザで1タブ開く (macOS: open / Linux: xdg-open。失敗は警告のみでserve継続。--no-openで抑止)
  └─ watchスレッド: 再ビルド成功ごとに新世代へemit → root swap → generationインクリメント
```

## 設計判断 (確定済み)

1. **preview専用シェルページ**: 素のdist index.htmlは配信しない。`packages/peitho-present`に`preview.ts`エントリを追加し、esbuildで`dist/preview.js`に束ねる。`dist/shell.js`と同じ流儀でコミット + バイナリ埋め込み (`include_str!`) + CI driftチェック
2. **リロード検知はgeneration方式 (絶対状態)**: 当初の`{"reload":true}` transientメッセージ案は、コアレスされるチャネル + ロード中未購読の窓でイベントが黙って消える (display swapで学んだ既知の失敗パターン)。サーバが`generation: u64`を持ち、**すべての**/sync GET応答 (handshakeもpollも) に`"generation":N`を載せる。クライアントはコンテンツfetchの**前に**handshakeして基準Gを記録し、以降の応答でgeneration != Gを見たら状態保存してlocation.reload()。イベント(edge)でなく状態(level)の比較なのでタイミングの穴が構造的にない。POST /syncでのreload注入は不可 (400)
3. **§16契約**: キー入力はrequestイベントemit → previewシェルが遷移実行。`peitho:overviewrequest` (toggle/enter/exit/activate) を追加。スライド移動は既存の`peitho:navigate`を再利用
4. **キー割り当て**: `o` = 単一⇄タイルのトグル (reveal.js / Slidev慣例)。タイル中Esc = 単一へ戻る。←/→ = 単一モードのスライド移動、タイルモードでは選択移動、Enterで選択スライドを単一表示。全ショートカットは既存`hasChordModifier`ガードを通す
5. **状態復元**: `sessionStorage`に`{mode, index}`を保存し、リロード後に復元 (タブ単位・リロード生存が要件に一致。URL hashは使わない — swapのChrome placementキー問題のようなURL副作用を避ける)
6. **タイル描画**: 全フラグメントを単一表示と同じDOM構造 (present shellと同じshadow DOM + CSS注入) で描き、CSS `transform: scale()`で縮小。aspect_ratioはmanifestから。専用サムネイル生成はしない (レンダリング経路を1本に保つ)。フラグメントfetchはPromise.allで並列
7. **世代ディレクトリ配信**: 配信中ディレクトリをremove_dir_allしない。ビルドごとに`build-<generation>/`へemitし、サーバroot (Arc<RwLock<PathBuf>>) をswap。直前世代を1つ残して古い世代をprune。'/'はpreviewではindex.html (デフォルトドキュメントはPresentServer::bindのパラメータ)
8. **壊れたデッキでもループ開始**: 初回ビルド失敗時はエスケープ済みエラーテキスト + generationポーリングの最小ページを配信し、次の保存成功で自動復帰 (build --watchが最初から壊れたデッキでもwatchを続けるのと同じ規律)。2回目以降の失敗は最後の成功ビルドを表示し続けstderrに報告
9. **watch配管はbuild --watchと共有**: 再ビルドアクションをクロージャ注入する共有ループ1本 (WatchRuntime)。watcher自体の継続不能エラーはprocess exit(1)で可視化 (スレッドの黙死をさせない)
10. **`build --watch`は存置**: READMEで「出力を外部サーバ/パイプラインに供給するプリミティブ」に位置づけ変更
11. **dist/汚染なし**: previewは自前キャッシュのみ。publishのcontamination checkには一切触れない

## 検証

- 全gates (cargo test x3 / clippy / fmt / bindings drift / npm build+test+typecheck / shell.jsとpreview.jsのdrift)
- 実ブラウザE2E (実施済み・全パス):
  1. タブが開き単一表示 → `o`でタイル⇄単一、選択枠・矢印・Enter・クリック遷移
  2. deck.md編集保存 → 自動リロード + モード/位置維持
  3. 連続保存 → 最新generationに収束 (取りこぼしなし)
  4. 壊れたデッキで起動 → 行番号付きエラーページ → 修正保存 → 自動復帰
  5. '/'でindex配信、consoleエラーなし
