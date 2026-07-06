# PDF export: 印刷時グラデーションフラット化 (Issue #142)

## Summary

`peitho export pdf` が生成するPDFをmacOSプレビュー.appで開くと、各ページが真っ白の状態から数秒〜十数秒かけて部分的に描画される。根本原因は**CSSグラデーション**で、ChromeのSkiaバックエンドがこれをType 4シェーディング(PostScript計算関数)+ソフトマスクとしてPDF化し、CoreGraphicsのピクセル毎関数評価が極端に遅いため。対策として、印刷用`pdf.html`に埋め込むスクリプトで**グラデーション背景だけをChrome自身にラスタライズさせてdata URL画像に置き換える**。テキストは完全にベクターのまま(選択・検索可能)。

## 計測事実 (2026-07-06、再調査不要)

実デッキ(decks/carina、11スライド、1.73MB)での実測。ベンチはPDFKit(プレビュー.appと同じレンダラー)でオフスクリーン描画:

| 指標 | 現状(ベクターシェーディング) | フラット化プロトタイプ |
|---|---|---|
| 1ページ目(星空カバー) | 14,204ms | 29ms (約490倍) |
| 全11ページ合計 | 15,109ms | 281ms |
| `/Shading` オブジェクト | 19 | 0 |
| `/SMask` | 9 | 0 |
| 埋め込み画像 | 0 | 2個 / 789KB |

- **遅さの原因はグラデーションのみ**。グラデーションだけをフラット色に置換した対照実験では、Type3フォント40個が残っていても5.8ms/ページ(シェーディング0)。フォントは速度に実質無関係
- Type 4シェーディングはプレビュー系レンダラーで「遅い」だけでなく**正しく描画されない**(radial-gradientで描いた星がほぼ消える)。フラット化版の方が画面表示に忠実
- 同一グラデーション背景を複数スライドが共有する場合、同一data URLをSkiaが重複排除するため画像は実質1枚分(10スライド共有で2オブジェクトのみ)
- `--virtual-time-budget`でスクリプト完了後の印刷が動作することをプロトタイプで確認済み(Chrome 149)
- 副次的発見(**本PRのスコープ外**、別根本原因): PDFサイズ1.7MBの内1.37MBはOsaka-Monoの丸ごと埋め込み。monospace指定のないコード要素がChrome既定等幅フォント(日本語環境=Osaka-Mono)に落ち、Skiaがサブセット化に失敗する。組み込みテーマ`.slot-code`にも同じ穴がある → 別Issueで対応

## Design decisions

- **フラグなしのデフォルト動作**にする(zero-config方針)。悪化するケースが計測上見当たらない: 視覚的にはより忠実、テキストはベクター維持、サイズ増(+0.77MB)は許容範囲でOsaka-Mono問題の解決後は現状より小さくなる
- **Chrome自身にラスタライズさせる**(SVG `foreignObject` → `<img>` → canvas → `toDataURL`)。CSSグラデーション構文をRust側でパース・描画する方式(tiny-skia等)は、`background-position`/`size`/多層背景/conic等の互換マトリクスを自前実装することになり採らない。Chromeの実レンダリングを使えば解釈のズレが原理的に生じない
- スクリプトは`render_pdf_document`が生成する`pdf.html`にインライン埋め込む。JSアセットは`crates/peitho-core/src/pdf_flatten.js`に置き`include_str!`で取り込む(shell.jsのようなビルド産物ではなく、ソースそのもの。TSビルドチェーンには入れない — 印刷専用の小さな補助スクリプトで、§16イベント契約とも無関係)
- キャプチャ解像度は**要素CSSピクセルの2倍**固定。1280px幅スライドで2560px — 1920px出力ページに対し1.33倍のオーバーサンプリング。グラデーションは低周波なので印刷でも十分(プロトタイプで目視確認済み)
- 対象は`getComputedStyle`の`background-image`に`gradient(`を含む**実要素と`::before`/`::after`疑似要素**。疑似要素は要素側スタイルの直接書き換えができないため、生成クラス+`!important`ルールを注入して置換する。疑似要素の寸法がpxに解決できない場合はその疑似要素をスキップ(ベクターのまま=遅いが正しい、にフォールバック)
- 忠実なキャプチャを保証できない対象はフラット化しない: `url()`レイヤー混在、`background-attachment: fixed`、`background-clip: text`、非`normal`の`background-blend-mode`、完全透明でない`background-color`+非`border-box` clip+非ゼロinset、複数フラグメントに分かれるインライン要素(`getClientRects`複数)、pxに解決できない要素寸法、padding/borderを持つ疑似要素、Chromeのcanvas上限を超える巨大背景はスキップし、元のベクターグラデーションを残す
- ターゲット収集前に`document.fonts.ready`と`load`完了を待ち、Webフォントや画像によるリフロー後の最終ジオメトリをキャプチャする(`--virtual-time-budget`でこの待機を許容)
- 要素毎に`try/catch`し、失敗時は元のグラデーションを残す(**劣化方向は常に「遅いが正しい」**。真っ白なPDFや欠けた背景は絶対に作らない)
- `run_chrome_print`に`--virtual-time-budget=10000`を追加。仮想時間なので静的ページでは実時間コストは僅少(グラデーションなしデッキのエクスポートが遅くならないことをE2Eで確認する)
- 置換後は`background-size: {w}px {h}px` / `background-position: 0 0` / `background-repeat: no-repeat`に加え`background-origin: border-box` / `background-clip: border-box`を明示し、ラスターが表現するborder-box領域に画像が正確に重なるようにする(元の多層背景のレイアウト規則が単一画像に誤適用されるのも防ぐ)

## Non-goals

- `box-shadow`/`filter`のフラット化はしない。Skiaがそれら自体を既に画像化するため(シェーディングとして残るのはグラデーションだけ)
- Osaka-Mono丸ごと埋め込み問題(上記)は別Issue
- ラスタライズページ全体化オプション(`--rasterize`)は導入しない。本方式でベクターテキストを保ったまま解決するため

## 実装タスク (TDD)

### Task 1: Chrome引数構築の純関数化 + `--virtual-time-budget`

- `run_chrome_print`の引数組み立てを純関数(例: `chrome_print_args(profile, out, url) -> Vec<OsString>`)に抽出
- Red: 引数列に`--virtual-time-budget`が含まれることを検証するunit test
- Green: フラグ追加
- 既存の`--headless=new`等の並びも同テストで固定する

### Task 2: `pdf_flatten.js` + `render_pdf_document`への埋め込み

- Red: `render_pdf_document`の出力に`<script>`とフラット化スクリプトの識別子(関数名等)が含まれることを検証するunit test。スライドHTML・`@page`等の既存アサーションが壊れないこと
- Green: `crates/peitho-core/src/pdf_flatten.js`を新設し、`include_str!`でテンプレートに埋め込む
- スクリプト仕様:
  - 全要素+`::before`/`::after`を走査、`background-image`に`gradient(`を含むものを収集
  - 各対象: 計算済み`background-image`/`position`/`size`/`repeat`/`origin`/`clip`+ジオメトリ(padding/border幅/スタイル、透明border-color)をスナップショットしてコピーしたdivをSVG `foreignObject`に包み、data URL経由で`<img>`にロード、canvasに2倍で描画、`toDataURL('image/png')`。`background-color`は意図的にコピーしない(要素自身の色がラスターの下でそのまま描画され、二重合成を防ぐ)。スナップショット(live `CSSStyleDeclaration`を保持しない)は先行ターゲットのDOM書き換えによる汚染を防ぐ
  - 実要素はinline styleで置換、疑似要素は生成クラス+注入`<style>`ルール(`!important`)で置換
  - 要素毎`try/catch`、失敗時は無変更
  - 完了を`document.documentElement`の`data-peitho-pdf-flattened`属性で観測可能にする(E2E/デバッグ用)。`document.title`はPDFメタデータの`/Title`になるため使わない

### Task 3: E2E test (ignored, Chrome必須)

- 既存`crates/peitho/tests/export_pdf.rs`の隣に追加
- グラデーション背景を持つデッキ(deck adjacent `css/`)をエクスポートし:
  - PDFバイト列に`/Shading`が**出現しない**こと
  - `/Subtype /Image`(または`/Subtype/Image`)が出現すること(フラット化が実行された証拠)
  - フォントオブジェクトが残っていること(テキストがベクター維持)
- 既存のexport E2E(グラデーションなし)が引き続き通ること

### Task 4: ドキュメント

- CLAUDE.mdのPDFエクスポート関連記述にフラット化を一行追記
- 本plan文書をdesign recordとして参照

## 検証手順 (verify時)

1. 全ゲート(workspace test x3 / clippy / fmt / bindings drift / npm build+test+typecheck / shell.js drift)
2. E2E: `PEITHO_CHROME_PATH`設定の上`cargo test -- --ignored`
3. 実デッキ計測: decks/carinaをエクスポートし、`/Shading`が0であること、プレビュー.appで即座に描画されること(スクリーンショットで確認)
4. グラデーションなしデッキ(examples/deck.md)のエクスポート時間が悪化していないこと
