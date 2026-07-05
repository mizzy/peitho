# 明示的スロット割り当て構文 実装計画 (2026-07-05)

Spec: `docs/specs/2026-07-05-explicit-slot-syntax.md`
Issue: [#21](https://github.com/mizzy/peitho/issues/21)

specで確定した設計を実装可能な粒度に落とし込む。全体をTDDで進める(各ステップにテストを先に書き、次に実装)。

## 三本柱・不変条件との整合

- **柱①(内容と設計の分離)**: 新記法はMarkdown側の拡張(`:::` + 属性)。HTML片を著者に書かせない ✅
- **柱②(Gitで管理できるHTML/CSSレイアウト)**: レイアウト側は無変更。slot契約はレイアウトHTMLの単一ソースのまま ✅
- **柱③(型チェック済みslot契約、silent drop禁止)**:
  - `ExplicitSlot(SlotName)` newtypeで慣習指定と明示指定を型で分離
  - 検証ルール11項目(spec参照)がすべて行番号+help付きビルドエラー
  - fenced div 内側の未クローズ、未知slot名、ネスト、長フェンス — silent path なし
- **typestate**: 変更なし。`FragmentKind` に variant を追加するだけで phase 遷移は既存経路

## 実装上の設計判断(specでは触れていない実装詳細)

### 判断1: SlotGroupは「マッピング時に展開」する

spec中「`SlotGroup { name, children }` をMappedSlotに押し込む」の実装形として、**mapping段階で SlotGroup を子 fragment に展開し、指定slotに個別 fragment として積む**方式を採る。

理由:
- 既存の check.rs は個別 fragment 単位で `check_accepts`/`check_arity` を回している。SlotGroup をラップしたまま check に渡すには `check_accepts` を fragment ツリーを掘る形に書き換える必要があり、変更が広がる
- SlotGroup を展開すれば check.rs は完全に無変更。accepts違反(検証ルール#11)は既存路線で自動的にエラー化される
- SlotGroup が phase を渡り歩く必要がなく、Mapped 以降には従来通り「slot → 個別fragment列」のフラット構造だけが流れる

代償: `MappedSlot` の fragment には「明示指定で入った」記録が残らない。ただし arity/accepts エラーは行番号(SlotGroup 内側 fragment の行)で十分特定可能なので、実用上問題ない。

### 判断2: ExplicitSlot は `FragmentKind` 内にのみ現れる中間型

`ExplicitSlot` は Parsed 段階で SlotGroup fragment に格納される中間型で、Mapped 以降には出てこない。lib.rs で公開はせず crate 内 pub(crate) に留める。

これで CLAUDE.md「long-term view + type safety」を満たす:
- Parser 内でのみ `ExplicitSlot::new(slot_name)` を生成できる
- Mapping で SlotGroup を展開する際に `ExplicitSlot` を消費して `SlotName` に落とす
- 将来 SlotGroup を扱う新caller(たとえばレンダラ拡張)が現れても、`ExplicitSlot` を作れないので silent path が生まれない

### 判断3: SlotGroup 内は生Markdownの再パースではなく、pulldown-cmark を一度で回してイベント列上で判定

spec 記述では「内側/外側の生Markdownを分離して2回pulldown-cmarkに掛ける」としていたが、実装効率上は **単一パスで pulldown-cmark を走らせつつ、`:::` 行を events の直前に検出して SlotGroup コンテキストを切り替える** ほうが素直。

理由:
- pulldown-cmark は `:::` を単なる HTMLブロック(あるいはparagraph)として拾うが、offset-iter で行位置を追跡できるので、`:::` を含む行のイベントは per-line で識別可能
- 生Markdownの再パースは、コードフェンスの ` ``` ` を跨ぐ範囲切り出しでオフバイワンを起こしやすい
- 既存の `parse_slide` は event ループなので、SlotGroup コンテキスト用のスタックを1つ足す形で済む

具体的には `parse_slide` の頭で全ソースを行スキャンして `:::` 行のマップ (`line -> SlotDivMarker`) を作り、event ループ中で「今 line が SlotGroup 開始行のイベントならスタックに push、終了行なら pop」する。fragment を積む際にスタック top を見て、生えた fragment の「所属先」を SlotGroup コンテキストに紐付ける。

コードフェンス内の `:::` 除外は、行スキャン時に ` ``` ` / `~~~` フェンスを追跡してフェンス内は無視すればよい。

### 判断4: 「所属先」の表現

具体的な fragment 構造は以下:

```rust
// domain.rs
pub(crate) struct ExplicitSlot(SlotName);

impl ExplicitSlot {
    pub(crate) fn into_slot_name(self) -> SlotName { self.0 }
    pub(crate) fn as_slot_name(&self) -> &SlotName { &self.0 }
}

pub enum FragmentKind<S = RawImagePath> {
    Heading { level: u8 },
    Paragraph,
    Text,
    Code,
    Image { alt: String, src: S },
    List,
    // 新規: 内側は 1個以上の子 fragment を持つ。ネストは検証ルール#8で禁止
    SlotGroup { name: ExplicitSlot, children: Vec<SourceFragment<S>> },
}
```

SlotGroup fragment 自体は line (`:::` 開始行) を持ち、children は SlotGroup 内で発生した通常 fragment の列。

## パーサ側の変更(`crates/peitho-core/src/parser.rs`)

### ステップ 1: `:::` 行スキャナ

`parse_slide` の先頭で、slide slice (= `source[range.start..range.end]`) を1行ずつスキャンして以下を作る:

```rust
struct SlotDivMarker {
    line: usize,           // ソース全体での行番号
    kind: SlotDivKind,
}
enum SlotDivKind {
    Open(ExplicitSlot),
    Close,
}
```

スキャン規則:
- 行頭(先頭空白は許さない)から `:::` で始まる行を検出
- コードフェンス (` ``` ` / `~~~` の開閉ペア) 内は無視
- ` :::` (前置空白あり)、`::::` (4コロン以上)、`::` (2コロンのみ) は SlotDivMarker にしない
  - 4コロン以上は spec 検証ルール#9 で明示エラー化するため、別途「4+コロン行」も検出して即エラー
- 開始行: `:::` の後に空白、`{slot=name}`、空白許容、行末
  - 属性なし → 検証ルール#1エラー
  - 属性キーが `slot` でない → 検証ルール#2エラー
  - 属性形式不正(複数キー、`=`欠落) → 検証ルール#3エラー
  - slot名不正 → `SlotName::new` の既存エラーを流用(検証ルール#4)
- 終了行: `:::` 単独(末尾空白のみ許容)
  - 属性付き `:::` → 検証ルール#6エラー

### ステップ 2: event ループ改修

`parse_slide` の event ループで:
- 現在の event の行番号 = `line_for_offset(source, global_start)` から SlotDivMarker map を引く
- Open マーカー行にヒットしたら **現在のスタック深さが 0** であることを確認(0でなければ検証ルール#8: ネスト禁止エラー)。空の SlotGroup 蓄積用 `Vec<SourceFragment>` を新規作成してスタック push
- Close マーカー行にヒットしたら:
  - スタック深さが 0 → 「対応する開始がない `:::`」= 実質検証ルール#5の裏返し(または純粋な構文エラー)
  - 深さ 1 → pop して SlotGroup fragment を作り、外側 fragment 列に append
  - 深さ 2 は判定順序上ありえない(判定はOpen時に済んでいる)
- SlotDivMarker 行では pulldown-cmark の event(HTMLブロック扱いのpush等)を無視して吸収する

`Event::Rule` は既存のように「スライド内 thematic break はエラー」の路線を維持。div内で `---` が来たら分割が発生するため、スライド末尾で「未クローズdiv」= 検証ルール#5エラーになる。実際にはこちらは分割器 (`split_slide_ranges`) が既に走った後の話だが、`split_slide_ranges` は `:::` を知らないので同じ結論(未クローズ → エラー)に落ちる。

### ステップ 3: SlotGroup fragment 化

Close 検出時に:
- children が空 → 検証ルール#7エラー(空の SlotGroup 禁止)
- 空でなければ `SourceFragment::slot_group(open_line, ExplicitSlot, children)` を作って外側列へ append

### ステップ 4: 属性パーサ

`{slot=name}` の1行パーサをテスト付きで実装。プロトコル:
- 先頭 `{`、末尾 `}` 必須
- 中身は `slot=name`。前後の空白は許容
- `name` は `SlotName::new` に渡す(既存の識別子ルール)
- それ以外の形式(`=` 欠落、複数キー、値なし)は具体的なメッセージ+helpのビルドエラー

### テスト(parser.rs 内 `#[cfg(test)]`)

- `slot_group_open_close_produces_fragment`: 単純な1つの div → SlotGroup fragment 1個
- `slot_group_children_are_parsed`: div 内の見出し・段落・リスト・コードが children に入る
- `nested_slot_group_is_error`: `::: {slot=a}` の中に `::: {slot=b}` → 検証ルール#8
- `long_fence_four_colons_is_error`: `::::` → 検証ルール#9
- `unclosed_slot_group_is_error`: 開始マーカーだけでスライド末尾 → 検証ルール#5
- `slot_group_missing_attr_is_error`: `:::` 単独開始(閉じ以外) → 検証ルール#1
- `slot_group_unknown_attr_is_error`: `::: {layout=x}` → 検証ルール#2
- `slot_group_multi_attr_is_error`: `::: {slot=a slot=b}` → 検証ルール#3
- `slot_group_invalid_slot_name_is_error`: `::: {slot=Foo}` → SlotName 経由のエラー(検証ルール#4)
- `close_marker_with_attr_is_error`: `::: {slot=x}` を終了に使う → 検証ルール#6
- `empty_slot_group_is_error`: 開始と終了の間に fragment 0個 → 検証ルール#7
- `slot_group_in_code_fence_is_ignored`: ` ``` ` 内の `::: {slot=x}` はマーカー扱いされない
- `slot_group_across_thematic_break_is_error`: div 内に `---` → 未クローズdivエラー

## ドメイン側の変更(`crates/peitho-core/src/domain.rs`)

### `ExplicitSlot` newtype 追加

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExplicitSlot(SlotName);

impl ExplicitSlot {
    pub(crate) fn new(name: SlotName) -> Self { Self(name) }
    pub(crate) fn into_slot_name(self) -> SlotName { self.0 }
    pub(crate) fn as_slot_name(&self) -> &SlotName { &self.0 }
}
```

`pub(crate)` に留めて外部crateから作れないようにする。

### `FragmentKind` に `SlotGroup` variant 追加

```rust
pub enum FragmentKind<S = RawImagePath> {
    ...,
    SlotGroup { name: ExplicitSlot, children: Vec<SourceFragment<S>> },
}
```

`default_accepts` / `removal_noun` / `Display` を実装(SlotGroup は展開されて Mapped 以降には現れないので、conservativeに `Accepts::Blocks` / `"slot group"` などにしておく。実装上は展開のほうが先に走るのでこの値が使われる経路はないが、`unreachable!` は避ける)。

`try_map_image_src` は SlotGroup の場合 children を再帰的にマップ。

### `SourceFragment::slot_group` コンストラクタ追加

```rust
impl SourceFragment<RawImagePath> {
    pub(crate) fn slot_group(
        line: usize,
        name: ExplicitSlot,
        children: Vec<SourceFragment<RawImagePath>>,
    ) -> Self { ... }
}
```

### bindings への影響

`Fragment` 系はTS bindingsに露出していない(bindings/はManifest/Notes/PresentConfig系のみ)。`git diff --exit-code bindings/` はゼロのはず。

## マッピング側の変更(`crates/peitho-core/src/mapping.rs`)

### `map_slide` に SlotGroup 展開ロジック

現在のループ:
```rust
for fragment in slide.fragments.iter().cloned() {
    let target = match fragment.kind() { ... };
    // 目的slotに fragment を積む
}
```

改修後:
```rust
for fragment in slide.fragments.iter().cloned() {
    if let FragmentKind::SlotGroup { name, children } = fragment.kind() {
        let target = name.as_slot_name().clone();
        // レイアウトに該当slotが無ければ検証ルール#10エラー(mapping 段階で早期に)
        let Some(contract) = layout.slot_by_name(&target).cloned() else {
            return Err(unknown_explicit_slot_error(target, fragment.line(), layout));
        };
        for child in children.iter().cloned() {
            slots.entry(target.clone())
                .or_insert_with(|| MappedSlot::new(contract.clone()))
                .push(child);
        }
        continue;
    }
    // 既存の慣習マッピング
    let target = match fragment.kind() { ... };
    ...
}
```

`unknown_explicit_slot_error` は「レイアウトのslot一覧」を help に含める(既存の unknown layout エラーと同スタイル)。

### `shallowest_heading_line` を SlotGroup 除外に

現状は fragment ツリー最上位のみ走査しているため、SlotGroup 内の見出しは自動的に対象外になる(判断1の展開ロジック上、SlotGroup fragment 自体は Heading ではないので既存のフィルタで自然に落ちる)。

明示的なテストで pin する: `title_inferred_from_outside_slot_group` (SlotGroup 内の見出しは title 推定に使われない)。

### テスト(mapping.rs 内 `#[cfg(test)]`)

- `explicit_slot_routes_fragment_to_named_slot`: two-column レイアウトで `::: {slot=left}` の中身が left slot に入る
- `unknown_explicit_slot_is_error`: レイアウトに無いslot名 → 検証ルール#10
- `explicit_slot_body_is_allowed`: 慣習でも拾える名前(body)の明示指定 OK(著者決定#3)
- `explicit_and_conventional_share_slot_check_arity`: 同じ slot に慣習と明示両方入って arity 超過 → 既存 check エラー
- `title_inferred_from_outside_slot_group`: SlotGroup 内の見出しは title に昇格しない
- `accepts_violation_via_explicit_slot`: `accepts=code` の slot に paragraph を明示投入 → 既存 check_accepts エラーに落ちる(検証ルール#11)

## Dispatch 側の変更(`crates/peitho-core/src/mapping.rs::dispatch_slide`)

specの「レイアウトdispatchとの相互作用」の実装:

- 現状の構造マッチは「各レイアウトで `map_slide` + `check_slide` を試して matches に集める」路線
- 判断1でSlotGroupが展開されるため、`map_slide` 内で「レイアウトに無いslot名」→ 即エラーになる。つまり明示slot名を持たないレイアウトは probing 段階で自然に落ちる ✅
- SlotGroup 中身は展開されて指定slotへ積まれるので、慣習slot(title/body/code)の充足計算に混ざる余地がない(そもそもSlotGroup 自身は既存の慣習分岐に到達しない)✅

したがって dispatch_slide のロジックは無変更で spec の相互作用が成立する。テストで pin:

- `dispatch_prefers_layout_with_explicit_slot_name`: two-column と title-only 2レイアウトの deck で `::: {slot=left}` を含むスライドが two-column に一意マッチ
- `dispatch_rejects_when_no_layout_has_explicit_slot`: 明示slot名を持つレイアウトがゼロ → 既存の「no layout matches」エラー(rejections に「unknown explicit slot」が並ぶ)

## 例(`examples/two-column/`)

新規ディレクトリ:

```
examples/two-column/
├── deck.md
├── layouts/
│   └── two-column.html
└── css/
    └── base.css
```

- `layouts/two-column.html`: 単一レイアウトファイル。`title` + `left` (accepts=blocks) + `right` (accepts=blocks)
- `deck.md`: frontmatter に `time: 5m` などを載せ、複数スライドで `::: {slot=left}` / `::: {slot=right}` を使う
- `css/base.css`: `.slot-left` / `.slot-right` を display:grid の 1fr 1fr で並べる最小スタイル

## ゲート

- `cargo test --workspace` を **3回連続** で通す(過去にflaky事故あり、CLAUDE.md指定)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` (今回はゼロのはず)
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js` (TS側は変更なしのはず)
- `examples/two-column/` で `cargo run --bin peitho -- build examples/two-column` を実行し、生成された `dist/index.html` を目視確認(左右2カラムがCSSで並ぶ)

## CLAUDE.md 更新

「Undecided — awaiting author's judgment」節から `Explicit fenced div slot notation ::: {slot=...} (§18)` の行を削除し、確定事項の invariants に「明示スロット割り当ては fenced div `::: {slot=name}` 記法で行う。パーサで `ExplicitSlot` newtype を生成、mapping で展開して指定slotへ直行(採用2026-07-05、spec: `docs/specs/2026-07-05-explicit-slot-syntax.md`)」相当を追加。

## 進行順序(TDD)

1. domain.rs: `ExplicitSlot` newtype + `FragmentKind::SlotGroup` + テスト(型のみ)
2. parser.rs: `:::` スキャナ + 属性パーサ + SlotGroup fragment 生成 + テスト(検証ルール#1〜#9)
3. mapping.rs: SlotGroup 展開 + 未知slot名エラー(#10) + テスト
4. check.rs: accepts違反(#11)が既存路線で捕捉されることの pin テスト(実装は変更なし)
5. dispatch: 相互作用の pin テスト(実装は変更なし)
6. examples/two-column/ 新設
7. CLAUDE.md 更新
8. 全ゲート実行
