# PDF export: Linux headless Chromeでの`image.decode()`無音ハング修正 (Issue #155)

## 背景

#153 / v0.8.1で修正したはずの#150（Preview.appでbox-shadow周りに黒矩形）が、Linux（decks.gosu.keのCI = ubuntu-latest）でexportしたPDFでは直っていなかった。切り分けの結果、`pdf_flatten.js`の`applyOuterShadow`にある`await image.decode()`が、**Linuxのheadless Chrome + `--virtual-time-budget`環境下でresolveもrejectもされず永遠にハングする**ことが根本原因と確定している（Issue #155にDockerでの計装トレースあり）。

- rejectされないため`catch`も走らず、`console.error`も出ない（サイレント）
- flatten全体がそこで停止し、仮想時間バジェット切れでChromeが未処理のbox-shadowのままPDFを印刷する
- macOSでは`decode()`が仮想時間切れより先に完了するため、たまたま動いていただけ

## 根本原因の範囲

壊れている不変条件は「**`--virtual-time-budget`下のheadless Chromeで実行されるin-pageスクリプトは`image.decode()`に依存してはならない**」。

計画時点では`packages/peitho-present/src/measure.ts`の`waitForImage`（PPTX export計測、`--virtual-time-budget=20000`で実行）にも同一クラスの`decode()`依存があり、本PRで同時に除去した。その後PR #156（Issue #152）でPPTX export自体が削除されたため、rebase後の最終形では`pdf_flatten.js`のみが対象になっている。

## 修正内容

### 1. `pdf_flatten.js` — `applyOuterShadow`のdecode分岐廃止

```js
// before
var image = document.createElement("img");
image.src = raster.dataUrl;
if (image.decode) {
  await image.decode();
} else {
  await loadImage(raster.dataUrl);
}

// after
var image = await loadImage(raster.dataUrl);
```

既存の`loadImage`（`new Image()` + load/errorイベント待ち）が返すimg要素をそのまま使う。Issueの検証パッチ（`image.src`設定 + 別Imageで`loadImage`待ち）よりさらに一本化した形で、appendする要素自体のloadを待つ。Issue著者がLinuxコンテナで`--print-to-pdf`し`/Luminosity` 8件→0件を確認済みのアプローチと同一の機構（loadイベント待ち）。

### 2. CI — Linux E2Eジョブ追加（再発防止）

この問題がリリースまで検出されなかったのは、Chrome実行E2E（`export_pdf.rs`の`/Luminosity`アサート含む）が全部`#[ignore]`で、CIは`cargo test --workspace`のみのためLinuxで一度も実行されていないから。

`.github/workflows/ci.yml`に`e2e`ジョブを追加する:

- ubuntu-latest（GitHub hosted runnerはGoogle Chrome stableプリインストール）
- ジョブ環境変数で`PEITHO_CHROME_PATH: /usr/bin/google-chrome`を明示指定する。テストヘルパは「`PEITHO_CHROME_PATH`が設定されているのに実在しない場合はpanic」に変更（プロジェクトの「明示パスの不存在はエラー、無音フォールバック禁止」の原則）。これによりCIではChrome欠落が必ず音を立てて落ち、E2Eの無音スキップによる空振りgreenは起こらない。env未設定のローカルでは従来どおり自動検出→なければスキップ
- `google-chrome --version`ステップをコンパイル前の早期ガードとして置く（panicより先に、速く明確に落とすため）
- `cargo test --workspace -- --ignored --test-threads=1`で`#[ignore]`のE2E（`export_pdf.rs`の4本）を実行。`--test-threads=1`は4-vCPU runnerでheadless Chromeの並列起動が60秒one-shotタイムアウトに触れるflakeを避けるため
- rust-cacheは`shared-key: tests`で既存`test`ジョブと共有し、workspaceの二重コンパイルを避ける

テストヘルパ`test_chrome_path`/`find_in_path`は`crates/peitho/tests/util/mod.rs`（サブディレクトリモジュール。tests/直下の.rsは独立バイナリになるため）に置き、panic化はそこ1箇所で行う。

レビューで`/S /Luminosity`の否定アサートが空白あり直列化しかマッチしないことが判明したため、`/Luminosity`単独の不在チェックに強化した（将来Chromeが`/S/Luminosity`とコンパクトに直列化してもtripwireがすり抜けない）。

## 検証

- 全ゲート（cargo test x3 / clippy / fmt / bindings drift / npm build+test+typecheck / dist drift）
- macOSローカルで`cargo test -- --ignored`（既存E2E、リグレッション確認）
- Docker（Linuxコンテナ + Playwright chromium）で`cargo test -- --ignored`を実行し、box-shadow E2Eが修正前fail（Issue #155再現）→修正後passになることを確認（Issueの再現手順と同型）
