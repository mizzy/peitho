# 明示的スロット割り当て構文 設計 (2026-07-05)

## ゴール

慣習マッピング(title/code/image/body の4種類)では曖昧になるレイアウト — two-column、left/right、複数code slotなど — に対して、著者がスライド本文内でスロットを明示指定できる構文を導入する。silent drop / silent fallback を作らず、pillar ①「Content(Markdown)と Design(HTML+CSS)の分離」を破らない形で拡張する。

対象Issue: [#21](https://github.com/mizzy/peitho/issues/21)。§18「Undecided Items」の1つ。前提となるマルチレイアウト対応(hybrid dispatch)は実装済み。

## 採用アプローチと理由

**Approach A: Pandoc fenced div `::: {slot=name}`**(2026-07-05 著者承認済み)

- Markdown記法の延長として設計された構文(Pandoc/MyST/Quarto系)。Markdownパーサの延長で構造化ノードとして扱えるため、パーサから mapping まで型で情報を運べる
- Markdownの中にHTML片(`<template>`など)を持ち込まない — pillar ①「Content と Design の分離」を破らない
- `:::` は行頭マーカーなので開始/終了/属性の行番号がpin可能。既存のエラー方針(行番号+help)と一致
- 属性のパースはfrontmatterの `key: value` パースと同種の1行構造。既存のパーサ規約と揃う

捨てた代替案:

- **`<template #name>` (Slidev風)**: HTMLパススルー依存でMarkdownパーサから見ると不透明ブロック。属性抽出を自前で書く必要があり、内側のMarkdownが素通しになる。何より`<template>`要素そのものがcontentに露出するのが pillar ① と噛み合わない
- **属性シンタックスなしのショートハンド `::: name`**: Pandocでは `.name` は class 属性のシンタックスシュガーであり、slot名との衝突が発生しうる。`{slot=name}` の明示形のみ許可

## 記法(著者決定 2026-07-05)

````markdown
# タイトル

::: {slot=left}
左カラムの本文。
- リスト項目もOK
:::

::: {slot=right}

```rust
fn main() {}
```

:::
````

- 開始マーカーは行頭の `:::`(ちょうど3コロン)+ 必須の `{slot=name}` 属性。終了マーカーは属性なしの `:::` 単独行
- 4コロン以上(Pandocではネスト用の長いフェンス)はv1ではエラー。将来ネストを解禁する際の予約とし、silent に3コロン扱いしない
- 内側のMarkdownはpulldown-cmarkでそのままパースされる(fenced code、リスト、段落、画像すべて許可)
- 属性は `slot=name` のみ許可。他のキー、複数キー、値なしはすべてビルドエラー
- 属性値の `name` は既存の `SlotName` バリデーション(識別子ルール)を再利用
- div内のHTMLコメントは外側と同一規則(JSON→ページ設定コメント、平文→スピーカーノート)。divで囲んでも per-slide のコメント意味論は変わらない

## パース戦略

### 処理順序(既存実装との整合)

スライド分割が先、fenced divスキャンが後。分割は全ソースに対する pulldown-cmark の `Event::Rule` スキャン(`split_slide_ranges`)で行われ、pulldown-cmark は `:::` を知らないため、**開いたdivの中に `---` があればそこでスライドが割れる**。結果としてそのdivは「終了マーカー未到達のままスライド末尾」となり検証ルール#5のエラーになる(silentに跨がせない)。divはスライド内で完結する構造であり、スライドを跨ぐdivはv1では表現できない — これは制約ではなく仕様。

### スライド内の二段パース

frontmatter/slide-split の「二段パース」パターンを踏襲する。

1. **プリトークナイズ**: スライドごとに `:::` 行を行ベースでスキャンし、開始/終了ペアを行番号付きで抽出。**スキャナはfenced code block(` ``` `/`~~~`)の内側を追跡し、コードフェンス内の `:::` はマーカー扱いしない**(Pandoc記法を解説するスライド等で必須)
2. **内側のパース**: 抽出した内側Markdownをpulldown-cmarkに渡し、通常の `Fragment` 列を得る
3. **外側のパース**: divブロックを取り除いた残りをpulldown-cmarkに渡し、通常の `Fragment` 列を得る
4. **ノード合成**: 行番号順に外側fragment列へ `Fragment::SlotGroup { name, children, line }` を挿入し、1つのfragment列にする

pulldown-cmarkの `ENABLE_*` に該当フラグはなく、自前トークナイズが唯一の選択肢。frontmatter切り出しと同じ方針で無理はない。

## ドメイン型の変更

```rust
pub enum FragmentKind {
    Heading { .. },
    Paragraph,
    List,
    Text,
    Code,
    Image { .. },
    SlotGroup { name: ExplicitSlot, children: Vec<SourceFragment> },
}
```

`ExplicitSlot` は `SlotName` を包む newtype(公開コンストラクタなし、パーサ内でのみ生成)。慣習マッピング側と型で識別できるようにする — CLAUDE.md「long-term view + type safety」ルールに従い、明示指定と慣習指定が同じ型を共有して silent 上書きが起きる余地を残さないため。

`bindings/` への影響はない見込み(TS側に露出しているのはManifest/Notes/PresentConfig系のみで、`Fragment`はRust内部型)。drift check(`git diff --exit-code bindings/`)で無変更を確認する。

## レイアウトdispatchとの相互作用

hybrid dispatch(明示`{"layout":…}` > 単一レイアウト無条件 > 一意な構造マッチ)の「構造マッチ」に `SlotGroup` を組み込む:

- スライドが `slot=left` を含むなら、`left` という名前のslotを持たないレイアウトは**構造マッチの候補から外れる**(明示slot名は強い構造シグナル)
- SlotGroupの中身は慣習マッピングの構造判定(title/code/image/bodyの充足)には**数えない** — 中身は指定slotに直行するため、慣習slotの充足計算に混ぜると二重カウントになる
- これにより「two-columnレイアウトと1カラムレイアウトが併存するdeckで、`slot=left`/`slot=right` を書いたスライドは自動的にtwo-columnへ一意マッチする」という自然な挙動が得られる。曖昧・ゼロマッチが依然エラーであることは不変

## title推定との相互作用

慣習のtitle推定(`shallowest_heading_line`)は**SlotGroup外のfragmentのみ**を対象とする。SlotGroup内の見出しは指定slotへ直行し、title候補にならない。`::: {slot=title}` と明示すればtitleへの明示投入も可能(accepts/arityは既存検査に従う)。

## マッピングの変更

`mapping.rs::map_slide` に分岐を追加:

- `FragmentKind::SlotGroup { name, children }` → 指定 `name` の slot に直接投入。`children` は慣習マッピングを**通さず**そのまま `MappedSlot` に押し込む
- レイアウトに該当slotが存在しない → ビルドエラー(行番号は開始マーカー行、helpにレイアウトのslot一覧)。慣習マッピングの未知slotは `unassigned` 経由でcheck時にエラーになるが、明示指定はマッピング時点で著者の意図(slot名のtypo等)が確定しているため、より早くより具体的なエラーを出す
- `SlotGroup` の中にネストした `SlotGroup` → v1ではビルドエラー(「ネストは未対応」旨のhelp)
- それ以外の fragment は従来の慣習マッピングをそのまま適用

**慣習が拾う slot と明示が指定する slot の衝突**は許可 — 同じslotに慣習と明示の両方が入る場合は arity 検査で自然に検出される(silent drop なし)。

## 検証ルール(すべて行番号+help付きビルドエラー。silentパスなし)

1. `:::` の開始マーカーに `{slot=…}` 属性が無い → エラー
2. 属性キーが `slot` 以外 → エラー(「`slot=name` のみ許可」)
3. 属性の複数指定(例 `{slot=a slot=b}`) → エラー
4. slot名が `SlotName` の識別子ルールに違反 → 既存の `SlotName` エラーを流用
5. 開始マーカーに対する終了 `:::` が無いままスライド末尾に達した → エラー(div内の `---` によるスライド分割もこの経路に落ちる。help に「divはスライド内で完結させる」旨を含める)
6. 終了 `:::` に属性が付いている → エラー
7. 空の `SlotGroup`(開始と終了の間に fragment が0個) → エラー(明示的に空を書くのは著者のミスであり、arity `0..*` のslotでも「書かない」ことで空を表現できる)
8. `SlotGroup` の中に `SlotGroup` がネストしている → エラー(v1未対応)
9. 4コロン以上のフェンス → エラー(将来のネスト用予約)
10. 指定した slot 名がレイアウトの slot に存在しない → マッピング時エラー(help: レイアウトの slot 名一覧)
11. `accepts=code` のslotに inline paragraph を明示投入したような accepts 違反 → 既存の check.rs 経路でそのままエラー(検査の重複実装をしない。既存経路で捕捉されることをテストで pin)

## サンプル(examples/ 追加分)

`examples/two-column/` を新設予定:

- `layouts/two-column.html`: `title` + `left`(accepts=blocks) + `right`(accepts=blocks) の3スロット
- `deck.md`: 慣習では left/right の判別が付かないことを示すため、`::: {slot=left}` / `::: {slot=right}` を使う実例
- `css/`: 左右2カラムのCSS

これにより「新記法を使うと何が可能になるか」がドキュメントとしても機能する。

## 実装フェーズ

1. Parser: `:::` プリトークナイズ(コードフェンス追跡込み)+ `FragmentKind::SlotGroup` の生成 + 構文エラー群(#1〜#9)
2. Mapping/Dispatch: `SlotGroup` 分岐 + 構造マッチへの組み込み + 未知slot名エラー(#10)
3. Check: 既存経路のまま変更不要(accepts違反が既存路線でエラー化されることをテストで pin)
4. Examples: `examples/two-column/` 追加
5. Docs: この spec を保持し、CLAUDE.md の「Undecided」節から確定事項へ移す

## v1で意図的に見送るもの

- **ネスト**: 将来 grid / tabs / steps を fenced div の入れ子で表現できる素地はあるが、v1はフラット1階層のみ。ネスト(#8)と長フェンス(#9)をエラーにし、将来の解禁時に silent path が生まれないよう seam を塞ぐ
- **`::: name` ショートハンド**: 属性省略形は導入しない。Pandoc慣習の `.name` は class扱いで衝突するため、明示形 `{slot=name}` のみ
- **スライドを跨ぐdiv**: 表現不可(処理順序の節を参照)。仕様として明記

## 著者決定(2026-07-05)

1. **構文は案A(fenced div `::: {slot=name}`)で確定**。Slidev風 `<template #name>` は不採用
2. **`examples/two-column/` を新設**する(既存exampleへの追加ではなく)
3. **慣習でも拾える名前(title/body/code)の明示指定は許可**。意図の明示は害がなく、arity超過は既存検査で捕捉される。「この段落は確実にbodyへ」という保険的用法も可能になる
4. 属性区切り文字(カンマ等)の扱いは**v1では発生しない**ため未決のまま先送り — v1の属性は `slot=name` の1個のみで、複数キーはエラー(検証ルール#3)。複数属性を導入する将来拡張の時点で決める

## 関連

- pillar ①: Content/Design分離 — HTML片を content に持ち込まない案A採用の主要根拠
- pillar ③: 型付きslot契約 — `ExplicitSlot` newtype で慣習と明示を型で分離
- CLAUDE.md「long-term view + type safety」: 型で強制することで将来の新caller(パーサ拡張)が silent path を作れないようにする
- 前提: マルチレイアウト対応(hybrid dispatch, 2026-07-03採用)は済み
