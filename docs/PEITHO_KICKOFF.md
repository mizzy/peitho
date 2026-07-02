# Peitho — キックオフ仕様書

> Claude Code へのハンドオフ用。この文書は設計フェーズで確定した判断を凝縮したもの。
> 実装はここから始める。設計が揺れたと感じたら勝手に再解釈せず、「未決定事項」節を参照するか著者に確認すること。

---

## 0. プロジェクト概要

- **名前**: `peitho`（全小文字。神話ライン argus / iris と揃える）
- **由来**: Peitho ＝ ギリシャ神話の「説得」の女神。プレゼン＝聴衆を説得する行為、という機能の核に対応。
- **リポジトリ**: `mizzy/peitho`
- **公開先**: `peitho.gosu.ke`
- **正体**: web / HTML ネイティブなプレゼンツール。**Markdown を source of truth** とし、決定論的レンダラで安定キー付き HTML に変換し、ブラウザで投影する。
- **実装言語（確定）**: ビルドコアは Rust（typestate を本気で使うため、および Carina との思想的一貫性のため）。発表ランタイム（発表シェル）は TS。**契約（ドメイン型・manifest スキーマ）は Rust の `peitho-core` を唯一の source of truth とし、`ts-rs` / `schemars` で TS 型を生成して drift を防ぐ**（詳細は §17）。「言語の統一」ではなく「契約の単一 source of truth」を守るのが要点。

---

## 1. 設計の三本柱

1. **コンテンツとデザインの分離**（k1LoW/deck 由来）
   コンテンツは Markdown、デザインはテンプレートと CSS。両者を混ぜない。
2. **HTML ネイティブで git 管理可能なテンプレート**
   デザイン成果物は HTML/CSS。git で管理でき、diff が取れ、レビューできる。deck が Google Slides の proprietary な世界にデザインを閉じ込めるのに対し、Peitho はここで差別化する。
3. **型検査されるスロット契約とキー付き override**（Carina 由来）
   スロットの過不足・型不整合・参照切れをビルド時に検出する。deck の「プレースホルダ不足でサイレントドロップ」の対極。

---

## 2. パイプライン

```
Markdown (source of truth)
    │  ← 著者が手で書く。内容と構造のみ。
    ▼
Peitho renderer (決定論的・純粋関数)
    │  ← 同じ入力なら常に同じ HTML。隠れ状態を持たない。
    ▼
HTML (data-slide-key + .slot-* 付き)
    │  ← base テーマ CSS を適用。
    ▼
ブラウザで投影 / peitho.gosu.ke でホスト
```

**Claude Design の位置づけ**: 毎デッキの描画には関与しない。上流で「たまに呼ぶデザイナー」として、(a) スロットを持つ base テーマ、(b) スライドキー起点の per-slide override CSS、を作る。**CSS だけを触り、HTML 構造には触らない**（詳細は §7）。

---

## 3. スロット契約

**テンプレート自身がスキーマを兼ねる。** 契約を別ファイルに分けない（二重管理は必ず drift するため）。アナロジーは Web Components の `<slot name>`。レイアウトは「名前付きスロットを持つカスタム要素」と考える。

テンプレート例:

```html
<!-- templates/title-body-code.html -->
<section class="slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body">
    <slot name="body" accepts="blocks" arity="0..*"></slot>
  </div>
  <figure class="code">
    <slot name="code" accepts="code" arity="0..1"></slot>
  </figure>
</section>
```

Peitho はテンプレートをパースして `<slot>` 群から契約を抽出し、流し込むコンテンツをそれに対して型検査する。

### コンテンツ型（`accepts`）

小さな束で足りる:

- `inline` — インラインのみ（強調・コードスパン・リンク可、ブロック改行不可）
- `blocks` — ブロックフロー（段落・リスト・引用など）
- `text` — マークアップなしの生テキスト
- `code` — 言語付きコードブロック
- `image` — 画像参照
- `list` — リスト限定

### arity（Rust 的レンジ記法）

- `1` — 必須・ちょうど1
- `0..1` — 省略可
- `1..*` — 1個以上
- `0..*` — 任意

---

## 4. マッピング（Markdown → スロット）

**規約デフォルト ＋ 明示エスケープ** の二段構え。

- **規約（deck 踏襲）**: スライド内で最も浅い見出しレベル＝タイトル、その次＝サブタイトル、残り＝ボディ。タイトル＋本文の大多数のスライドは素の Markdown だけで通る。
- **明示割り当て**: 2カラム等、規約だけでは曖昧なレイアウト向けに fenced div で明示する。

```markdown
# アーキテクチャ

::: {slot=left}
- Markdown = source of truth
- 決定論的レンダリング
:::

::: {slot=right}
​```rust
enum Phase { Parsed, Mapped, Checked }
​```
:::
```

規約で済む9割は素の Markdown、凝った1割だけ明示タグ。

---

## 5. 検査パス（4段・反サイレントドロップ）

deck はプレースホルダが足りないと余りを黙って捨てる。**Peitho は捨てずにビルド時エラーにする。**

1. コンテンツ片をスロットに割り当て（規約 or 明示タグ）
2. 各スロットの中身が `accepts`（型）を満たすか
3. `arity` を満たすか
4. **未割り当てのコンテンツが残っていないか**（← 反サイレントドロップの肝）＋ 必須スロットが埋まっているか

エラーは行番号付きで、次のアクションまで示す:

```
error: slide 4 に code スロットが2つ必要だが layout 'title-body-code' は 0..1 まで
  = help: layout 'code-2col' を使うか、明示スロットで振り分けてください
```

### typestate

ビルドを相（phase）として型で表現する:

```
Parsed → Mapped → Checked → Rendered
```

**未検査のスライドはレンダリング関数に渡せない** ようにする（コンパイル時保証）。Carina の phase-based structs をそのままスライドに適用する。「検査を飛ばして描画」が型レベルで不可能になる。

---

## 6. 安定キー

- **著者が明示したキーを一次ソースにする**: `<!-- {"key":"arch-1"} -->`
- 明示がなければ派生キー（スラグ等）を振るが、**override を当てる価値のあるスライドには明示キー必須** の運用。
- 派生キーがコンテンツ依存（例: タイトルをスラグ化）だと、タイトルを一文字直した瞬間にキーが変わり、当てた CSS が対象を見失う ＝ まさに避けたい「サイレントに剥がれる」失敗モード。だから override 対象は明示キーで固定する。
- キーは **CSS のターゲティングフック** と **将来 AI がスライドを参照するハンドル** を兼ねる。
- 型検査: 存在しないキーを狙った CSS セレクタはビルドエラー。手調整が剥がれたら黙って崩れるのではなく赤で止まる。

---

## 7. ページ単位の位置・サイズ調整（Model B ＝ 採用案）

Peitho は **純粋レンダラのまま**（deck 式のステートフルターゲットは採らない）。ページ個別のジオメトリ調整は、安定キー起点の CSS override で行う。

```css
/* themes/overrides.css */
[data-slide-key="arch-1"] .slot-code {
  grid-column: 2 / 3;
  width: 60%;
}
[data-slide-key="intro"] .slot-title {
  font-size: 4rem;
  align-self: end;
}
```

- コンテンツ（Markdown）は一切汚さない。調整はカスケードに任せる。
- 全部 git 管理・diff 可能・完全再現。再レンダリングすれば自然に再適用される（純粋関数なのに「調整が生き残る」を、ソース側 CSS で実現）。
- これは HTML/CSS ネイティブだからこそ可能。deck は Google Slides にカスケードがないため、状態をターゲットに溜めるしかなかった。Peitho は状態を溜めずに同じ結果を出せる。
- **override 検査**: 存在しないスロットを狙った override はコンパイルエラー。存在しないスライドキーへの参照もエラー。エスケープハッチすら型で守る。

---

## 8. Claude Design への受け渡し境界

B が成立する条件は「Claude Design が **CSS だけ** をいじり、HTML 構造には触らない」こと。構造を触られるとスロット契約が壊れる。受け渡しを次で固定する:

- **Claude Design が受け取るもの**: レンダリング済み HTML（`data-slide-key` と `.slot-*` 入り）＋「使えるスライドキーとスロットクラスの一覧」という短い語彙表。**read-only**。
- **Claude Design が返すもの**: **override スタイルシート1枚だけ**。マークアップは生成させない。
- **Peitho**: base テーマの上に override.css を重ね、全セレクタのキー＆スロットを既知の契約に照合。

CSS 作業は2層に分かれる:

- **base テーマ** — 全スライド共通。たまに作り直す高価値な仕事。
- **per-slide override** — デッキ個別の詰め。キー起点。

どちらも「ただの CSS」なので、両方が Model B に自然に乗る。

---

## 9. マイルストーン 1 — 最小の縦切り

**目的**: 全アーキテクチャを end-to-end で実証する最小経路を1本通す。

- 安定キー1個を明示した Markdown 1枚
- スロット契約を持つ base テンプレート1枚に流し込む
- HTML を吐く（`data-slide-key` と `.slot-*` 付き）
- そのキーを狙った `override.css` を1行当てて、効くことを確認
- 検査パスが動作すること（契約違反でちゃんとエラーになる）

これが通れば三本柱すべてが実証できる。ここを最優先で。

### タスク分解

1. `parser.rs`: Markdown → 中間コンテンツモデル（見出しレベル / ブロック / コードブロックを保持）
2. `template.rs`: HTML テンプレートをパースし `<slot>` から契約（name / accepts / arity）を抽出
3. `mapping.rs`: コンテンツ片をスロットに割り当て（まず規約のみ。明示 fenced div は次段）
4. `check.rs`: 4段検査 ＋ typestate の相（`Parsed → Mapped → Checked → Rendered`）を型で表現
5. `render.rs`: 検査済みモデル → `data-slide-key` と `.slot-*` を持つ HTML を出力
6. `theme.rs`: `base.css` に `overrides.css` を重ね、セレクタのキー＆スロットを契約に照合
7. `main.rs`: `peitho build <md>` を通す。`--watch` は縦切りが通った後。
8. `examples/deck.md` と最小テンプレート・CSS を置いて手動で通す

---

## 10. 初期リポジトリ構成（案）

```
peitho/
  Cargo.toml
  src/
    main.rs           # CLI エントリ (build, watch)
    parser.rs         # markdown → 中間コンテンツモデル
    slot.rs           # スロット契約の型 (accepts, arity)
    template.rs       # HTML テンプレート → スロット契約抽出
    mapping.rs        # コンテンツ → スロット割り当て (規約 + 明示)
    check.rs          # 4段検査、typestate の相
    render.rs         # mapped+checked → HTML (data-slide-key, .slot-*)
    theme.rs          # base テーマ + override.css の重ね合わせと検証
  templates/
    title-body-code.html
  themes/
    base.css
    overrides.css
  examples/
    deck.md
  tests/
```

---

## 11. 立ち位置（既存ツールとの差別化）

- **deck**: コンテンツ/デザイン分離のコンセプトを借りる。ただし deck は Google Slides 依存（proprietary・OAuth・ステートフル）。Peitho は HTML ネイティブ・git 管理・純粋レンダラで差別化。
- **Slidev / Marp**: 同じ Markdown ベースの開発者向けプレゼンの系譜。Peitho の固有性は **型検査されるスロット契約＋キー付き override**（サイレントドロップしない、参照切れをビルド時に弾く）。Carina の型思想をスライドに持ち込む点。

一言でいうと: **deck のコンテンツ/デザイン分離 × HTML ネイティブで git 管理可能なテンプレート × Carina 由来の型検査されるスロット契約＆キー付き override**。

---

## 12. CLI サーフェスと責務の三分割

関心事「生成 / 発表 / 公開」を3コマンドに分ける。build と present は別言語・別コマンドだが、同じ契約（manifest ＋ 生成型）に対して co-evolve するので**単一リポジトリ（ワークスペース）に束ねる**。

```
peitho build      → dist/（スライド本体 HTML/CSS + slides/ + manifest.json + notes.json）
                    ※ 配布物 = スライド本体のみ。発表シェル・ノートは非混入がデフォルト。
                    ※ --watch で Markdown 保存のたびに再ビルド。
peitho present    → 中間表現から present.html + 発表シェル + ノートを揮発領域に生成して開く。
                    ※ .peitho/present-cache/ 等（gitignore）。永続の配布物には残さない。
peitho publish    → dist/ をそのまま公開先へ（除外・分岐なし・最薄ラッパー）。
                    ※ 中身は既存の IaC/CI（S3+CloudFront 等）に委譲。デプロイを再発明しない。
```

- **build と present は別コマンド・別言語**（build=Rust バイナリ、present=TS）。だが契約で結ばれ drift しない（§17）。
- **Claude Design は CLI の外側の低頻度ループ**。base テーマと override CSS を作る「たまに呼ぶデザイナー」。毎回の build/present には挟まらない。

---

## 13. 共通中間表現から emit で射影する

解析・マッピング・検査は **1回・1種類**。そこから複数の出力ターゲットへ射影する。分岐するのは入力ではなく**出力段（emit）だけ**なので、source of truth は割れない。

```
Markdown + テンプレート + テーマ + ノート
        │
   peitho build            ← 解析・マッピング・検査（§5）。ここは1回。
        ▼
  中間表現（manifest + 検査済みスライドモデル）   ← 唯一の source of truth
        │
   ├── emit distribute  →  slides/ + manifest.json + index.html（スライド本体のみ）
   └── emit present     →  上記を指す present.html + 発表シェル + notes.json（揮発）
```

将来 emit ターゲットが増えても（PDF 書き出し、静的サムネイル等）、同じ中間表現に emit を足すだけ。build/present/publish の三分割がそのまま拡張点になる。

---

## 14. 2エントリポイント構成（実体は単一）

配布と発表で**入口（エントリ HTML）は分かれるが、スライドの実体は単一**。両エントリは slides/ を「持つ」のではなく「指す」。

```
dist/
  index.html        ← 配布用エントリ。manifest を読み、slides/ を表示するだけ。
  slides/           ← スライド本体（HTML 断片 + CSS + 画像）。実体はここ。唯一。
  manifest.json     ← 契約（順序・キー・src・hasNotes）。

（present 実行時のみ、揮発領域に）
  present.html      ← 発表用エントリ。同じ slides/ と manifest を読み、発表シェルを起動。
  notes.json        ← ノート。present.html だけが読む。
```

- **共有**: slides/ と manifest（単一の source）。
- **index.html 固有**: 最小の表示・ナビのみ。発表シェルもノートも非混入。
- **present.html 固有**: 発表シェル（TS）を積み、notes.json も読む。**build の永続成果物ではなく present が揮発生成**する。
- **publish**: index.html・slides/・manifest.json のみ配る。present.html と notes.json は元々 dist に無い。

**スライド断片はスライドごとに別ファイル**（`slides/001-arch-1.html`）。fetch 単位・Shadow root 投入単位・差分再ビルド単位と一致する（§15）。

---

## 15. 接続方式：fetch + Shadow DOM

present.html が共有の slides/ を **fetch** して読み込み、各スライドを **Shadow DOM** に入れて表示する。

- **fetch**: スライド実体は単一のまま、present はそれを指して読むだけ（二重化しない）。
- **Shadow DOM**: スライドの CSS を Shadow root に閉じ、発表シェル UI の CSS と干渉させない（iframe の CSS 隔離利点を、iframe の操作しづらさ無しで得る）。
- **取っ手はホスト要素に出す**ので、Shadow で隔離してもシェルからキーで掴める。
- **発表者ビュー（2画面）**: 2ウィンドウ + BroadcastChannel（チャンネル `peitho-sync`）で位置同期。各ウィンドウは同じ fetch+Shadow の仕組みで1枚を表示。

（同梱方式＝ビルド時に1ファイルへ畳む案は、slides の単一実体を破り source が二重化するため却下。）

---

## 16. 本体⇔発表シェルの契約（取っ手とイベント）

**シェルはスライドの外側から、キーで対象を指し、イベントで状態を放送する。スライドはイベントを聞くだけで、シェルの存在を知らない**（一方向依存）。

### 取っ手（スライド本体が公開する表面）

```html
<section class="peitho-slide" data-slide-key="arch-1" data-slide-index="1">
  <!-- 以下 Shadow 内。シェルは踏み込まない -->
  <h1 class="slot-title">...</h1>
</section>
```

- `data-slide-key` — Shadow ホスト要素に付与。シェルが対象を掴む主キー。manifest の `key` と一致（build が保証）。
- `data-slide-index` — 同上。順序・進捗計算用。manifest の `index` と一致。
- `.peitho-slide` — ホストのクラス。`peitho-` 名前空間で著者クラスと衝突回避。

### カスタムイベント（DOM・ウィンドウ内。全て `peitho:` 接頭辞）

**shell → 全体（通知）**
- `peitho:slidechange` — payload `{ key, index, total, previousIndex }`。切替直後。進捗更新・2画面同期・発表者ビュー更新のトリガ。
- `peitho:presentationstart` — payload `{ total, startedAt }`。発表開始・タイマー始動。
- `peitho:presentationend` — payload `{ endedAt, elapsedMs }`。発表終了・タイマー停止。

**UI → shell（要求）**
- `peitho:navigate` — payload `{ to: "next"|"prev"|"first"|"last" | {key} | {index} }`。移動要求。
- `peitho:timercontrol` — payload `{ action: "pause"|"resume"|"reset" }`。

### 不変条件（癒着を防ぐ・最重要）

- **遷移の実行主体はシェルのみ**。UI 部品（リモコン・クリック・発表者ビューのボタン）は `navigate` を要求するだけ。シェルが実行し、結果 `slidechange` を放送。入力源が増えても状態は一元管理。
- **スライド本体はイベントを聞くのは可、要求（navigate 等の発火）は不可**。スライドがシェルの存在を前提にすると、配布物（シェル無し）で壊れる。
- **発表シェルはスライド本体 + manifest に一方向依存する独立エントリ**。本体のバンドルに混ぜ込まない。「含める/含めない」は emit の注入スイッチで済み、作り直しにならない。

### 同期の層分け

```
発表者ウィンドウで「次へ」
  → peitho:navigate {to:"next"}（DOM, ウィンドウ内）
  → シェルが遷移実行 → peitho:slidechange（DOM, ローカル UI 更新）
  → BroadcastChannel 'peitho-sync' に送信
  → 聴衆ウィンドウのシェルが受信 → 同位置へ → ローカルに slidechange 再放送
```

DOM イベント（ウィンドウ内）と BroadcastChannel（ウィンドウ間）を層として分け、シェルが橋渡しする。UI 部品はウィンドウ内イベントのみ知ればよい。

---

## 17. manifest / notes スキーマと契約の単一 source

### manifest.json（中身は持たず、参照とメタのみ）

```jsonc
{
  "version": 1,                 // スキーマ自身の版。シェルの読める形式判定用
  "peithoVersion": "0.3.1",     // 生成器の版。再現性の記録
  "title": "...",
  "slideCount": 40,
  "slides": [
    { "index": 0, "key": "title",  "src": "slides/000-title.html",  "hasNotes": true  },
    { "index": 1, "key": "arch-1", "src": "slides/001-arch-1.html", "hasNotes": false }
  ]
}
```

- `src` は**断片への参照であって中身ではない**（中身を埋めると二重化する。硬い制約）。
- `key` は目録。実物の取っ手は本体 DOM の `data-slide-key`。両者一致を build が保証。
- `hasNotes` は**有無のフラグのみ**。ノート本文は載せない（manifest は配布側 index.html からも読まれうるため）。

### notes.json（別ファイルに隔離）

```jsonc
{ "version": 1, "notes": { "title": "...", "arch-3": "...", "conclusion": "..." } }
```

- **key で紐づく**（index ではない）。スライドを挿入して順序がずれてもノートは正しいスライドに付いたまま。
- **物理的に別ファイル**。DOM に `display:none` で潜ませない（View Source で漏れるため）。present だけが fetch し、publish は配らない。

### 契約の単一 source of truth（drift 防止）

- ドメイン型（Slide, SlideKey, Notes 等）と manifest スキーマは **`peitho-core`（Rust）が唯一の source**。
- `ts-rs` / `schemars`（JSON Schema 経由）で **TS 型を Rust から生成**。発表シェル（TS）は契約を手書きせず生成物を参照。
- Rust 側の manifest を変えると TS の型がずれてコンパイルが赤くなる ＝ **drift を型で止める**。§3「契約を二重管理しない」のランタイム版。

### 初期リポジトリ構成（ワークスペース・§10 を更新）

```
peitho/                     # 単一 git repo（ワークスペース）
  crates/
    peitho-core/            # ドメイン・manifest スキーマ = 契約の source of truth
    peitho/                 # build CLI（Rust バイナリ）。present/publish サブコマンドもここ
  packages/
    peitho-present/         # 発表シェル（TS）。present.html に載る
  bindings/                 # peitho-core から生成した TS 型（peitho-present が参照）
  templates/                # レイアウト HTML（スロット契約を持つ）
  themes/                   # base.css / overrides.css（共有テーマ。デッキ側には置かない）
  examples/
    deck.md
  tests/
```

**スライドの Markdown 本体は peitho repo に置かない**（別リポジトリ、例 `mizzy/decks` を推奨。peitho をバージョン固定で参照）。生成 HTML はどこにもコミットしない（`.gitignore`・再生成する）。base テーマ/共通レイアウトは peitho repo、デッキ固有 override CSS はデッキ側。

---

## 18. 未決定事項（Claude Code は勝手に決めず、ここで止めるか著者に確認）

- **レイアウト選択の方式**: MVP は「明示指定 or ルールベース選択 → 選ばれたレイアウトに対して検査」で止める。**型駆動ディスパッチ**（コンテンツ形状を満たす契約を持つレイアウトを自動探索）は将来オプション。複数レイアウトが適合したときの曖昧性解決を決めていないため、MVP では踏み込まない。
- **明示スロット記法の最終形**: fenced div（`::: {slot=...}`）を第一候補とするが、Slidev 的コンポーネントスロットとの比較は未決。MVP では規約マッピングを先に通し、明示記法はその後。
- **コードブロックの扱い**: HTML ネイティブなのでシンタックスハイライタで直接描画できる（deck のような外部コマンドでの画像化は不要の見込み）。ハイライタ選定は実装時に。
- **発表キャッシュ（`.peitho/present-cache/`）の方針**: 毎回作り直す（クリーンだが起動が一拍遅い）か、キャッシュして差分更新する（速いが古い成果物が残りうる）か未決。watch との組み合わせで決める。
- **実装言語**: 確定（build=Rust / present=TS / 契約は Rust から生成）。§0・§17 参照。この項目はクローズ。
