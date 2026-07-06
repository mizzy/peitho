# PDF export: 印刷時box-shadowフラット化 (Issue #150)

<!-- derived-from ./2026-07-06-pdf-gradient-flatten.md -->
<!-- constrained-by ../../CLAUDE.md -->

## Summary

`peitho export pdf` が生成するPDFをmacOS Preview.app / Quick Look / `sips`で開くと、`box-shadow`付き要素の周囲に硬い黒矩形が出る。ブラウザ表示とpoppler/Chrome PDFビューアでは出ない。根本原因はChrome `--print-to-pdf`がぼかし付き`box-shadow`を**黒塗り + `/SMask /S /Luminosity` ExtGState + DeviceGrayマスク画像つきForm XObject**としてPDF化し、Quartz系レンダラがLuminosity SMaskを誤合成するため。Issue #142のグラデーションフラット化と同じく、PDF export専用の`pdf.html`内スクリプトで**box-shadowだけをRGBA PNGへラスタライズして置換**する。本文テキストと通常DOMはベクターのまま維持する。

## 計測事実 (2026-07-06、再調査不要)

- デッキCSSから`box-shadow`を外すとPreview.appの黒矩形は消える。HTML表示には最初から出ない
- Chrome PDFはぼかし影を`/SMask /S /Luminosity`として出し、QuartzレンダラAPIだけがBBox境界まで黒く描く。poppler/Chrome PDFビューアは正しく描く
- シャドウをcanvasでRGBA PNG化し、要素背面の`<img>`として敷いてからChromeでprintすると、Quartzでも柔らかい影として正しく描画される
- RGBA PNGのPDF化では画像XObject自身に`/SMask`が付くことがある。テストで禁止するのは`/SMask`全般ではなく、Quartz誤合成の原因である`/S /Luminosity`
- 既存の`--virtual-time-budget=10000`で`pdf_flatten.js`のasync処理がprint前に収束することはグラデーション対応で確認済み

## Design decisions

- **既存flattenの拡張にする**。対象は`crates/peitho-core/src/pdf_flatten.js`のみ。`render_pdf_document`が`include_str!`で`pdf.html`に埋め込むPDF export専用JSで、npm package / TS bundleには入れない
- **実行順は gradient → shadow**。inset shadowは`background-image`の先頭レイヤーへprependするため、グラデーションflatten後のbackgroundを書き換える。top-levelは`waitForStableLayout()`を一度だけ待ち、`flattenGradients()`後に`flattenBoxShadows()`を呼ぶ
- **computed `boxShadow`を自前で小さく解析する**。Chrome computed形式は例: `rgba(0, 0, 0, 0.45) 0px 18px 48px 0px`。カンマ分割は括弧深度を見る。`inset`を分類し、長さはpxのみサポートする。`rgba()`内カンマ、複数shadow、負のoffset/spreadを扱う。未対応値は要素単位で`console.error`して元の`box-shadow`を残す
- **複数shadowはCSSの重なり順を守る**。CSSはリスト先頭が最前面。canvasは後から描いたものが上に来るため、同一canvasへ描くときはshadowリストを後ろから先に描く
- **外側shadowはスライド子の絶対配置PNGにする**。対象要素の`.peitho-slide`を見つけ、`slideRect.width / slide.offsetWidth`をscaleとして、`(elementRect - slideRect) / scale - slide.clientLeft/clientTop`でpadding box基準のローカル座標を得る。`.peitho-slide`の`transform: scale(...)`はstacking contextかつabsolute配置の包含ブロックなので、`position:absolute; z-index:-1; pointer-events:none`の`<img>`をslide直下に挿入し、元要素の`box-shadow`を`none !important`にする
- **外側shadowの近似限界を受け入れる**。実際のCSS `box-shadow`は要素自身のペイント位置に属するが、置換PNGはslide直下の背面レイヤーになる。通常のカード影では一致するが、兄弟要素の不透明背景や複雑なz-indexが絡むと、兄弟との重なり順が変わり得る。Preview.app黒矩形の除去を優先し、該当ケースはフォールバック対象または既知の近似差分とする
- **inset shadowは背景最上層PNGにする**。inset shadowは背景の上、コンテンツの下に描かれるため、`background-image: url(data), <old>`としてprependし、`background-size/position/repeat/origin/clip`も同じく先頭レイヤーを追加する。`background-attachment: fixed`、非`normal`の`background-blend-mode`、安全にCSSリスト操作できない値は`console.error`して元のまま残す
- **シャドウ描画はcanvas APIだけで行う**。外側shadowは`ctx.shadowColor/shadowBlur/shadowOffset*`と「矩形を十分遠くに置き、shadowだけcanvas内へ落とす」トリックを使う。`shadowBlur`/`shadowOffsetX/Y`はcanvas CTMの影響を受けないため、`context.scale(SCALE, SCALE)`には依存せず、矩形座標・radius・blur・offset・farAwayをすべて明示的に`SCALE`倍して描く。`border-radius`はcomputed corner radiusが単一px値のときだけ`roundRect`で反映する。spreadは矩形の外側inflateとradius調整でエミュレートする。canvas余白は軸ごとに`blur * 2 + Math.abs(offset) + Math.max(0, spread)`を確保する。`SCALE = 2`とcanvas上限は既存グラデーションflattenに合わせる
- **要素単位でall-or-nothing**。outer/insetのどちらか、または複数shadowの一つでも未対応なら、その要素はDOMを書き換えない。適用前にPNGをすべて作り、最後にlayer追加 / background prepend / `box-shadow:none`をまとめて行う。途中失敗時は追加済みnodeを取り除く
- **スキップ条件は明示ログつきフォールバック**。`getClientRects().length !== 1`、対象要素から`.peitho-slide`直下までの祖先チェーンに`transform !== "none"`がある、`.peitho-slide`が見つからない、サイズ0、px以外のborder radius、canvas上限超過は`console.error("peitho pdf shadow flatten:", describeElement(...), reason)`して現状維持する

## Non-goals

- `text-shadow`と`filter: drop-shadow(...)`はIssue #150では扱わない。どちらもQuartz系PDF問題の同族だが、フォントグリフや任意アルファシルエットのラスタライズが必要で、矩形box-shadowとは別問題
- ページ全体ラスタライズや`--rasterize`オプションは導入しない。ベクターテキストを維持する
- CSS Shadows Level 4相当の全構文互換は目指さない。PDF exportで安全に置換できるpx解決済みcomputed shadowだけを対象にする

## 実装タスク (TDD)

### Task 1: Red E2Eを追加

- 対象: `crates/peitho/tests/export_pdf.rs`
- 既存`export_pdf_flattens_gradient_backgrounds_to_images`をモデルに、Chrome必須の`#[ignore]`テストを先に追加してredを確認する
- テスト名: `export_pdf_flattens_box_shadows_without_luminosity_smask_to_images`
- deck adjacent `css/shadow.css`の最小CSS。生HTMLブロックは`process_html_chunk`で`unsupported html`になるため使わず、既存のMarkdown由来DOMへCSSだけでshadowを当てる:

```css
.peitho-slide {
  background: #f8fafc;
  color: #111827;
}
.peitho-slide h1 {
  display: inline-block;
  padding: 24px 48px;
  border-radius: 28px;
  background: white;
  box-shadow: rgba(0, 0, 0, 0.45) 0px 18px 48px 0px;
}
```

- deck本文は通常Markdownだけにする:

```markdown
# Vector Text

Box shadow PDF export
```

- 実装前redの理由: 現状Chromeは`h1`の`box-shadow`をLuminosity SMaskとして出すため、次のassertが失敗する。`h1`自体をshadow対象にしておくと、誤って要素ごとラスタライズした実装では`/Font` canaryが消えて検知できる

```rust
assert!(!pdf_bytes_contain(&bytes, b"/S /Luminosity"));
assert!(
    pdf_bytes_contain(&bytes, b"/Subtype /Image")
        || pdf_bytes_contain(&bytes, b"/Subtype/Image")
);
assert!(pdf_bytes_contain(&bytes, b"/Font"));
```

- 注意: `assert!(!pdf_bytes_contain(&bytes, b"/SMask"))`は書かない。RGBA画像XObjectのalpha maskまで禁止してしまうため

### Task 2: `pdf_flatten.js`のtop-levelを二段flattenへ整理

- 対象: `crates/peitho-core/src/pdf_flatten.js`
- `waitForStableLayout()`はtop-level orchestrationに移し、既存gradient処理は待機済み前提の内部関数にする
- コード断片の形:

```js
async function flattenPdfArtifacts() {
  await waitForStableLayout();
  var gradientCount = await flattenGradients();
  var shadowCount = await flattenBoxShadows();
  document.documentElement.setAttribute("data-peitho-pdf-flattened", String(gradientCount + shadowCount));
  document.documentElement.setAttribute("data-peitho-pdf-shadow-flattened", String(shadowCount));
}
```

- 既存の`try/catch`方針は維持し、top-level failureでも属性を設定する
- 対象: `crates/peitho-core/src/render.rs`
  - 既存unit testを`pdf_document_embeds_pdf_flattening_script_after_slides`相当に広げる
  - assertion例: `assert!(html.contains("flattenGradients")); assert!(html.contains("flattenBoxShadows"));`
  - `PDF_FLATTEN_JS`が`</script`を含まないテストは維持

### Task 3: `box-shadow` parserと対象収集

- 対象: `crates/peitho-core/src/pdf_flatten.js`
- 追加する小ヘルパー:

```js
function splitCssList(value) { /* comma split with parentheses depth */ }
function parseSignedPixel(value) { /* /^-?\d+(\.\d+)?px$/ */ }
function parseShadowList(boxShadow) { /* [{ inset, color, offsetX, offsetY, blur, spread }] */ }
function parseCornerRadius(value) { /* "12px" only; reject "12px 8px" and non-px */ }
```

- parser policy:
  - `none`または空文字は対象外
  - `inset` keywordを外す
  - Chrome computed前提で先頭の`rgb(...)` / `rgba(...)`を色として扱う
  - 残りは`offsetX offsetY blur? spread?`。offsetは必須、blur/spread省略時は0
  - blurは負数不可。spreadは負数可
  - ひとつでもparse不能なら要素単位でskip
- `.peitho-slide`解決、skip条件、座標:

```js
var slide = element.closest(".peitho-slide");
if (!slide) throw new Error("element is outside .peitho-slide");
if (hasTransformBeforeSlide(element, slide)) throw new Error("transformed element ancestor");
if (element.getClientRects().length !== 1) throw new Error("fragmented element");

var slideRect = slide.getBoundingClientRect();
var elementRect = element.getBoundingClientRect();
var scale = slideRect.width / slide.offsetWidth;
var localX = (elementRect.left - slideRect.left) / scale - slide.clientLeft;
var localY = (elementRect.top - slideRect.top) / scale - slide.clientTop;
var width = elementRect.width / scale;
var height = elementRect.height / scale;
if (width <= 0 || height <= 0) throw new Error("zero-sized element");
```

- `hasTransformBeforeSlide`は対象要素自身から親へ歩き、`.peitho-slide`に到達したら止める。slide自身のprint用`transform: scale(...)`は許容するが、中間祖先のtransformはローカル座標を歪めるためskipする:

```js
function hasTransformBeforeSlide(element, slide) {
  for (var node = element; node && node !== slide; node = node.parentElement) {
    if (getComputedStyle(node).transform !== "none") return true;
  }
  return false;
}
```

### Task 4: 外側shadowをPNG layerへ置換

- 対象: `crates/peitho-core/src/pdf_flatten.js`
- outer shadowだけをまとめて1枚のtransparent PNGへ描く。canvas寸法は`(width + padLeft + padRight) * SCALE` / `(height + padTop + padBottom) * SCALE`
- 描画順とspread/radius調整。`shadowBlur`/`shadowOffsetX/Y`はCTMでscaleされないため、`context.scale(SCALE, SCALE)`は使わず、canvas座標系へ明示変換する:

```js
var s = SCALE;
outerShadows.slice().reverse().forEach(function (shadow) {
  var inflated = scaleRect(inflateRect(baseRect, shadow.spread), s);
  var radius = scaleRadius(adjustRadius(cornerRadii, shadow.spread), s);
  var far = farAway * s;
  context.shadowColor = shadow.color;
  context.shadowBlur = shadow.blur * s;
  context.shadowOffsetX = shadow.offsetX * s + far;
  context.shadowOffsetY = shadow.offsetY * s;
  fillRoundedRect(context, inflated.x - far, inflated.y, inflated.width, inflated.height, radius);
});
```

- `roundRect`がないChrome環境に備えて、`ctx.roundRect`がなければ小さなpath helperを使う
- 適用断片:

```js
var image = document.createElement("img");
image.src = dataUrl;
await image.decode();
image.setAttribute("data-peitho-pdf-shadow", "outer");
Object.assign(image.style, {
  position: "absolute",
  left: (target.localX - padLeft) + "px",
  top: (target.localY - padTop) + "px",
  width: cssWidth + "px",
  height: cssHeight + "px",
  zIndex: "-1",
  pointerEvents: "none",
  maxWidth: "none"
});
target.slide.appendChild(image);
```

- `await image.decode()`はDOM挿入前に行う。既存`loadImage` helperを再利用してもよいが、virtual time下のprintタイミングに対して画像decode完了を決定的にする
  - **【2026-07-06に廃止 — Issue #155】** この`decode()`推奨はLinuxのheadless Chrome + `--virtual-time-budget`でresolveもrejectもされず無音ハングすることが判明し、`loadImage`（loadイベント待ち）へ一本化された。`decode()`を再導入しないこと。詳細: `docs/plans/2026-07-06-pdf-flatten-linux-decode-hang.md`
- all-or-nothingのため、`box-shadow:none`はこの時点ではまだ設定しない

### Task 5: inset shadowをbackground先頭レイヤーへ置換

- 対象: `crates/peitho-core/src/pdf_flatten.js`
- inset shadowだけを要素border boxサイズのtransparent PNGへ描く。ここでも`context.scale(SCALE, SCALE)`には依存せず、clip path、ring path、radius、blur、offsetをすべて明示的に`SCALE`倍する
- 描画モデルはCSSと同じく「ボックスの外側全体が内側に影を落とす」。offsetの符号は反転しない。手順:
  - border-boxの角丸矩形でclipする
  - 「巨大な外周矩形 + spread分deflateした内側角丸穴」のeven-odd中空パスを作る
  - `ctx.shadowColor/shadowBlur/shadowOffsetX/Y`へ同符号のoffsetを設定してfillする
  - 中空リングが落とす影だけがclip内へ入り、背景最上層PNGになる
- 描画断片:

```js
var s = SCALE;
clipRoundedRect(context, scaleRect(borderBox, s), scaleRadius(cornerRadii, s));
insetShadows.slice().reverse().forEach(function (shadow) {
  var inner = scaleRect(deflateRect(borderBox, shadow.spread), s);
  var innerRadius = scaleRadius(adjustRadius(cornerRadii, -shadow.spread), s);
  context.shadowColor = shadow.color;
  context.shadowBlur = shadow.blur * s;
  context.shadowOffsetX = shadow.offsetX * s;
  context.shadowOffsetY = shadow.offsetY * s;
  context.beginPath();
  context.rect(-huge * s, -huge * s, (width + huge * 2) * s, (height + huge * 2) * s);
  appendRoundedRectPath(context, inner, innerRadius);
  context.fill("evenodd");
});
```

- background list操作は既存computed値をsnapshotしてから行う。安全条件:
  - `backgroundAttachment`に`fixed`が含まれない
  - `backgroundBlendMode`は全レイヤー`normal`
  - `splitCssList`で`backgroundImage/Size/Position/Repeat/Origin/Clip`を分割できる
- prepend断片:

```js
setImportant(style, "background-image", 'url("' + dataUrl + '"), ' + old.backgroundImage);
setImportant(style, "background-size", width + "px " + height + "px, " + old.backgroundSize);
setImportant(style, "background-position", "0 0, " + old.backgroundPosition);
setImportant(style, "background-repeat", "no-repeat, " + old.backgroundRepeat);
setImportant(style, "background-origin", "border-box, " + old.backgroundOrigin);
setImportant(style, "background-clip", "border-box, " + old.backgroundClip);
```

- `old.backgroundImage === "none"`の場合はtailなしの単一レイヤーにする
- outer/insetの適用がすべて成功した後だけ:

```js
setImportant(target.element.style, "box-shadow", "none");
```

### Task 6: ドキュメント更新

- 対象: `CLAUDE.md`
- Pitfallsに1行追加:

```markdown
- **PDF export flattens box-shadow at print time**: Chrome emits blurred CSS box-shadows as `/S /Luminosity` soft masks that Quartz renders as hard black rectangles, so `pdf_flatten.js` rasterizes supported shadows to RGBA PNGs for PDF export (measured 2026-07-06). Design record: `docs/plans/2026-07-06-pdf-shadow-flatten.md`
```

- 本ファイルをIssue #150のdesign recordとする。README更新は不要

## 検証手順 (verify時)

1. Red確認: `PEITHO_CHROME_PATH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" cargo test -p peitho --test export_pdf export_pdf_flattens_box_shadows_without_luminosity_smask_to_images -- --ignored`
2. Green後E2E: `cargo test -p peitho --test export_pdf -- --ignored`
3. 全ゲート:

```bash
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/measure.js
```

4. Quartz視覚A/B確認。popplerではなく`pdfseparate`でページ分割後、`sips`でQuartzレンダリングする:

```bash
mkdir -p /tmp/peitho-shadow-ab
peitho export pdf path/to/shadow-deck.md -o /tmp/peitho-shadow-ab/before.pdf
# 実装後:
cargo run -p peitho -- export pdf path/to/shadow-deck.md -o /tmp/peitho-shadow-ab/after.pdf
pdfseparate /tmp/peitho-shadow-ab/before.pdf /tmp/peitho-shadow-ab/before-%02d.pdf
pdfseparate /tmp/peitho-shadow-ab/after.pdf /tmp/peitho-shadow-ab/after-%02d.pdf
sips -s format png /tmp/peitho-shadow-ab/before-01.pdf --out /tmp/peitho-shadow-ab/before-01-quartz.png
sips -s format png /tmp/peitho-shadow-ab/after-01.pdf --out /tmp/peitho-shadow-ab/after-01-quartz.png
```

5. PDF byte確認:

```bash
grep -a "/S /Luminosity" /tmp/peitho-shadow-ab/after.pdf
grep -a "/Subtype */Image\\|/Subtype/Image" /tmp/peitho-shadow-ab/after.pdf
```

期待値: afterでは`/S /Luminosity`が0件、画像XObjectが存在し、Quartz PNGで黒矩形が消えて柔らかい影だけが残る。
