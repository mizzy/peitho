# PowerPoint export (Issue #140)

## Summary

`peitho export pptx <deck.md> -o out.pptx` を追加する。エンジンは**Option B: 完全構造変換**(著者判断 2026-07-06)。スライドのスロット内容をpptxのテキストボックス・画像として構造的にマッピングし、受け取った側がPowerPoint/Keynoteで編集・再利用できるファイルを生成する。ジオメトリとタイポグラフィはヘッドレスChromeで実測する(後述)。pptx生成はライブラリに依存せず手書きzip+XML(著者判断 2026-07-06)。

著者判断まとめ(2026-07-06、AskUserQuestionで確定):

1. エンジン: **Option B(完全構造変換)** — Issue動機「受け取った側が編集・再利用できる」を採る
2. Keynote: **pptxのみ出荷** — `.key`直接生成はしない。KeynoteのpptxインポートをREADMEで案内。`export keynote`サブコマンドは作らない
3. Speaker notes: **pptxのノートペインに含める(デフォルト、フラグなし)** — pptxは「視聴者向け配布物」ではなく「編集可能なソース」を渡す用途。「notesはdist/に混ぜない」不変条件は視聴者向け配布物(dist/, PDF)への混入禁止と解釈し、pptxのノートペインはpptxの正規機能として含める
4. pptx生成: **手書きzip+XML** — 依存は`zip`クレートのみ。ppt-rs(0.2.x、2025-11生まれ)は若く出力品質未検証のため採用しない

## Motivation

- 企業環境でPowerPoint納品が必要なケース、Keynoteチームへの共有
- PDF(#109)と違い、受け取った側がテキストを編集・スライドを再利用できる
- §13「同じ中間表現に対するemit追加」の2例目。`export`サブグループ(#109で導入)に`pptx`を足す

## 中心設計: DOM-walk measurement(単一ソース方式)

Option Bの最大の課題は「CSSレイアウトとpptxレイアウトのギャップ」。これを**レイアウトが実際に起きる場所=ブラウザ**で解決する:

1. coreが**計測用HTML**(`render_measure_document`)を生成する。全スライドの`<section>`を論理キャンバスサイズ(`aspect_ratio().width() x height()` px)で並べ、計測スクリプト(`measure.js`)を埋め込む
2. ヘッドレスChromeを`--headless=new --dump-dom --virtual-time-budget=<N>`で起動。`measure.js`が`document.fonts.ready`後にDOMを歩き、スライドごとに:
   - `class^="slot-"`要素とその中のブロック要素・テキストラン(text node単位)の**rect + computed style**
   - `<img>`のrect + src + alt
   - `<section>`のbackground-color
   を収集し、JSON化して`<script type="application/json" id="peitho-measure">`としてbodyに追記する
3. CLIがdump-domの出力からJSONを抽出し、coreのpptx builderに渡す
4. coreが計測JSON + `Deck<Rendered>`(notes, SlideKey) + `ResolvedImageAsset`(画像バイト)からpptx zipを組み立てる

### なぜIR+ジオメトリ突合ではなくDOM-walkか(3レンズ一致 → 著者確認なしで採用)

代替案は「Checked IRのslot→fragment構造を主とし、Chromeはジオメトリだけ測って突合する」だったが:

- **long-term**: DOM-walkはカスタムレイアウト(pillar ②: レイアウト=ユーザーが書くHTML+CSS)に自動追従する。IR突合は「このfragmentはDOMのどの要素か」という対応規則を持ち、レイアウトが増えるたびに突合ルールのcarve-outが増える
- **type-safety**: 計測JSONスキーマが唯一の契約になり、ts-rsでRust⇔TSを単一ソース化できる(bindings/ドリフトチェックに乗る)。二重ソース(IR構造+計測)は対応ズレがランタイムでしか見えない
- **root-cause**: 「CSSレイアウトの結果を知らない」ことがOption Bの根本課題であり、ブラウザに計測させるのはその根本の解消。IR側でレイアウトを推測するのは症状対処

DOM(rendered HTML)はRendered IRの決定的射影なので、これは依然として「IRからの構造変換」である。テキストランはtext node単位で取るため、PowerPoint上で再折返し可能(行折返しでrunは分割されない)。

### 計測JSONスキーマ(契約、ts-rs対象)

peitho-core `domain.rs`(または新モジュール)にserde + ts-rs型として定義し、`bindings/`に生成・コミット(既存ドリフトチェックに乗せる):

```
MeasuredDeck    { canvasWidth, canvasHeight, slides: Vec<MeasuredSlide> }
MeasuredSlide   { key: String, backgroundColor: String, boxes: Vec<MeasuredBox>, images: Vec<MeasuredImage> }
MeasuredBox     { slot: String, rect: MeasuredRect, style: MeasuredBoxStyle, paragraphs: Vec<MeasuredParagraph> }
MeasuredBoxStyle{ backgroundColor, borderColor, borderWidth, borderRadius }   // 視覚chrome再現用
MeasuredParagraph { align, bulletLevel: Option<u8>, numbered: bool, runs: Vec<MeasuredRun> }
MeasuredRun     { text, color, fontFamily, fontSizePx, bold, italic, underline, monospace }
MeasuredImage   { src, alt, rect: MeasuredRect }
MeasuredRect    { x, y, w, h }   // 論理キャンバスpx、section左上原点
```

- rect単位は論理キャンバスpx。EMU変換は`px * 9525`(96dpi)。**16:9キャンバス1280x720 → 12192000x6858000 EMU = PowerPoint標準16:9とexact一致**。4:3の960x720 → 9144000x6858000 = 標準4:3と一致。この一致が「論理キャンバスpxを座標系にする」選択の裏付け
- `resolution` frontmatterキーは**pptxでは使わない**(PDF専用のraster物理サイズ。pptxはベクター+実測px座標なので不要)。Issueのopen question「resolutionとの関係」への回答
- codeブロック(`<pre>`)内の`\n`はスクリプト側でparagraph分割する。syntax highlight色は`hl-*` spanのcomputed colorがそのままrunに乗る(theme CSS→ブラウザcascade→実測、CSSパース不要)
- リストは`<li>`→`bulletLevel`(ネスト深さ)と`numbered`(最寄りlist ancestorが`ol`ならtrue)。見出し・段落はcomputed font-size/weightがrunに乗る

### measure.jsの置き場所と契約チェック

`packages/peitho-present/src/measure.ts`として書き、`dist/measure.js`にビルドして**コミット**し、coreに`include_str!`で埋め込む — shell.jsと同一パターン(ビルド成果物だがコミット+CIドリフトチェック)。measure.tsは`bindings/MeasuredDeck.ts`等をimportして型チェックされるので、スキーマのRust⇔TSドリフトはコンパイルで落ちる。

### pptx zip構成(手書きXML)

必要part(最小構成):

```
[Content_Types].xml
_rels/.rels
docProps/core.xml, docProps/app.xml
ppt/presentation.xml (+ _rels)          … sldSz = キャンバスpx*9525 EMU
ppt/slideMasters/slideMaster1.xml (+ _rels)
ppt/slideLayouts/slideLayout1.xml (+ _rels)   … blankレイアウト1枚
ppt/theme/theme1.xml
ppt/slides/slideN.xml (+ _rels)         … 1 IRスライド = 1 slide part
ppt/notesMasters/notesMaster1.xml (+ _rels)   … notesがあるデッキのみ
ppt/notesSlides/notesSlideN.xml (+ _rels)     … notesを持つスライドのみ
ppt/media/*                             … ResolvedImageAsset.source_absから読んだバイト
```

- テキスト: 1スロット = 1 `<p:sp>`テキストボックス(rect位置、autofitなし)。`MeasuredBoxStyle`からsolidFill/線/角丸を再現。paragraph→`<a:p>`(align, buChar+indent)、run→`<a:r><a:rPr sz b i u><a:solidFill>`。フォントサイズは`px * 0.75 * 100`(px→pt→1/100pt)。フォントはfont-family先頭ファミリー名
- 画像: Markdown由来の解決済みcontent image(`src`が`assets/`で始まるもの)だけを`<p:pic>`化し、rect位置、`ppt/media/`にコピーしrelで参照。altは`<p:nvPicPr>`のdescr
- notes: `RenderedSlide::notes()`をnotesSlideのbody placeholderにplaintextで
- スライド背景: sectionのbackground-colorをsolidFillで
- XML組み立ては既存の`html-escape`等と同様に文字列テンプレート+escape関数。`zip`クレート(workspace依存に追加、core側)でVec<u8>を返す

### CLI shape

```
peitho export pptx <deck.md> -o out.pptx
peitho export pptx <deck.md>              # -o省略時は<deck>.pptx(deck.mdの隣)
```

- `ExportCommand`(main.rs)に`Pptx`variantを追加。`export_pdf`と同型の`export_pptx`: `build_artifacts` → tempdirに計測workspace(measure.html + peitho.css + assets/) → `locate_chrome`(既存再利用) → `--dump-dom --virtual-time-budget` → JSON抽出(`id="peitho-measure"`マーカー間) → core pptx builder → 書き出し
- Chrome不在・dump失敗・JSON抽出失敗はPDFと同水準の明確なエラー

## Non-goals

- **`.key`直接生成・`export keynote`サブコマンドは作らない**(著者判断)。READMEにKeynoteでpptxを開く案内を書く
- v1では**ハイパーリンクのhref**は持ち越さない(リンクは実測スタイルの色付きテキストになる)。将来slide relsで非破壊追加可能
- v1では**グラデーション・背景画像**は非対応(背景はsolid colorのみ)。スロット外の装飾要素(レイアウト固有の飾りdiv等)も持ち越さない
- v1では**layout-baked/remote `<img>`**はpptxへ持ち越さない。Markdown content imageとして解決され、`assets/`パスになった画像だけを編集可能なpicture partとして出力する
- v1では**フォント埋め込み**はしない(typeface名の指定のみ。閲覧側で代替される可能性は受容)
- `resolution`キーはpptxでは消費しない(上記)

## Spike結果(2026-07-06実測、Chrome 149.0.7827.201 / macOS 15.7.7)

計測パイプラインの成立を実Chromeで検証済み:

- `--headless=new --virtual-time-budget=5000 --dump-dom`で、ページJS(`document.fonts.ready`待ち→DOM歩行→`<script type="application/json" id="peitho-measure">`をbodyへ追記)の**実行後DOMが正しくdumpされる**。所要約3秒。rect(x:96, y:80, w:335.9, h:84)・computed font-size(56px)・color・font-weightすべて期待値と一致
- **重大な発見: Chrome 149はワンショット完了後にプロセスが終了しない**。完了直後にmacOSのGoogleUpdater(`--wake-all`)を起こし、それが親プロセスを延命させる。`--disable-background-networking --disable-component-update`でも防げない(実測)
- この結果、**landed済みの`peitho export pdf`(#109/#139)は現環境でハングする**(実測: PDFは書けるが`.output()`が永遠にブロック)。Chrome自動更新による現場リグレッション
- 完了シグナルは取れる: `--print-to-pdf`はstderrに`N bytes written to file <path>`、`--dump-dom`はstdoutの`</html>`終端

### 根本修正: 共有ワンショットChromeランナー

「ワンショットheadless Chromeの実行と終了」が共有シームであり、pdf/pptxの両consumerが同じ壊れ方をするので、ここを1箇所で直す:

- `run_chrome_print`の`.output()`(exit待ち)をやめ、**spawn + stdout/stderrパイプ読み取り + 完了シグナル検知 + タイムアウト(既定60s程度) + 検知後にchildをkill/wait**するランナーに置き換える
- 完了述語: pdf = stderrの`bytes written to file`(かつ出力ファイル非空)、dump-dom = stdoutの`</html>`終端
- childは使い捨てtempdirプロファイルなのでkillしてよい(pitfallsの「SIGTERMはクラッシュ扱い」はユーザーの永続プロファイルの話。使い捨てプロファイルにcrash recoveryの害はない)
- タイムアウトは「壊れたページで永遠に待つ」一般故障モードも同時に塞ぐ
- `export pdf`をこのランナーに乗せ替える(同一根本原因の同時修正。per-consumerのcarve-outはしない)
- CLAUDE.md pitfallsにこの実測事実を追記する

jsdomはレイアウトを持たないためrectは常に0 — measure.tsのvitestはDOM構造の歩き方・run抽出・paragraph分割のロジックのみを検証し、ジオメトリはChrome-gated E2E(`#[ignore]`、export_pdf.rs型)で検証する。E2E後、実際にPowerPoint/Keynoteで開く手動確認を1回行う。

## Tasks (TDD)

1. ~~**Spike**~~ 完了(上記「Spike結果」)
2. **共有Chromeランナー**(crates/peitho): spawn+完了シグナル+タイムアウト+kill。`export pdf`を乗せ替え(ハング修正)。unit test(完了述語、タイムアウト、非空チェック)。fake chromeスクリプトでのテストはexport_pdf既存テストの手法に準拠
3. **計測スキーマ**: core domainに`Measured*`型(serde + ts-rs)、bindings生成・コミット
4. **`render_measure_document`**(core render.rs): 計測用HTML生成。unit test(キャンバスサイズ、section列挙、measure.js埋め込み、JSON marker要素)
5. **measure.ts**(packages/peitho-present): DOM-walk実装。vitest(構造歩行・run統合・pre改行分割・liバレルレベル・hl-span色取得のロジック。jsdomはレイアウトを持たないのでrect値は対象外)。dist/measure.jsビルド+コミット+ドリフトチェックをCIに追加
6. **pptx writer**(core 新モジュール`pptx.rs`): 計測JSON+Rendered+画像→zip bytes。unit testはzipを読み戻してXML断片をassert(slide数、sldSz EMU、run色/サイズ、bullet、notesSlide、media、Content_Types)
7. **CLI `export pptx`**(main.rs): workspace emit→Chromeランナーでdump-dom→JSON抽出→書き出し。エラーパスのunit test(Chrome不在等はexport_pdfのテストに準拠)
8. **E2E**(crates/peitho/tests/export_pptx.rs、`#[ignore]` Chrome-gated): サンプルデッキ→pptx→unzipしてテキスト・画像・notes存在をassert。export_pdfのE2Eも現環境で通ることを確認(ランナー修正の回帰確認)
9. **Docs**: README(export pptx、Keynoteはpptxインポート案内)、CLAUDE.md構造節+pitfalls(Chrome 149非終了の実測)追記

## Related

- Issue #140 / #109 (PDF export) / #23 (aspect_ratio)
- `docs/plans/2026-07-05-pdf-export.md`(export サブグループとChrome起動の先行例)
- §13 of `docs/PEITHO_KICKOFF.md`
