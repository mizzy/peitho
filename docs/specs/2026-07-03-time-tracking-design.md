# 発表時間トラッキング（うさぎとかめ）設計書

日付: 2026-07-03
ステータス: 設計確定（時間の指定方法は著者指示「ゼロコンフィグにしつつ、Markdown frontmatterで設定を書く。そこにプレゼン時間を設定できるとよい」2026-07-03に従う）

## ゴール

slidev-rabbit-turtle（https://zenn.dev/kaakaa/articles/slidev-rabbit-turtle）と同等の時間管理機能を追加する。

- **うさぎ**: スライドの進行率（現在インデックス/最終インデックス）を示すマーカー
- **かめ**: 時間の進行率（経過時間/発表予定時間）を示すマーカー
- 両者を同一トラック上で右に移動させ、うさぎがかめより先行していれば予定内、かめが先行していれば時間超過ペースだと一目で分かる

**表示先の要件（ユーザー指定）**: 他のツールはプレゼン画面に表示するが、peithoでは**発表者画面に表示する**。1スクリーンで発表者画面がない場合のみ**プレゼン画面に表示する**。

## 決定事項

### D1: 発表予定時間はデッキ先頭のYAML frontmatterで指定する（著者指示）

```markdown
---
time: 15m
---

# 最初のスライド
```

- これは**デッキレベル設定の一般機構**の初出。ゼロコンフィグ方針（peitho.toml等の別ファイルを作らない）のもと、デッキ自身が設定を携行する。`time`が最初のキー
- 受理形式: `15m` / `90s` / `1h` / `1h30m`（文字列）、または裸の整数`15`（分と解釈。slidevの`?time=10`と同じ感覚）
- `time`未指定・frontmatter自体なし → 時間トラッキング表示なし（従来どおり）。既存デッキは無変更で動く
- CLIフラグは追加しない（ゼロコンフィグ。同一デッキの15分版/30分版のような上書きが必要になったら別Issue）

**エラー（サイレントドロップ絶対禁止）**:
- 未知キー → `deny_unknown_fields`で行番号+help付きビルドエラー（PageCommentと同じ流儀）
- 不正なtime値（`0`、負、単位なし文字列`abc`、空）→ 行番号+help付きビルドエラー
- YAMLとして壊れている → 行番号+help付きビルドエラー

### D2: frontmatterのパースはpulldown-cmarkのmetadata blockで字句解析する

スライド区切りも`---`（thematic break）なので、frontmatterの`---`との弁別が核心。自前の文字列前処理ではなく、`Options::ENABLE_YAML_STYLE_METADATA_BLOCKS`を使い、pulldown-cmark（当初は0.10を想定、下記の実測により**0.13**へ更新）に文書先頭のYAMLブロックを`Tag::MetadataBlock`としてトークナイズさせる。

**実測による修正（2026-07-03〜04、Task 2実装時）**: pulldown-cmark 0.10はこのオプションを有効にすると（1）**文書途中の密な`---`ペアもMetadataBlock化し**（`---`/`# Cfg`/`port: 8080`/`---`のような正当なスライドまで飲まれる）、（2）**字下げ`---`の密ペアで無限ループする**（`peitho build`が永久ハング。文書のどこにあっても発火）。どちらも実測で確認。よって**pulldown-cmarkを0.13へアップグレード**する（0.11以降でハングは修正済み、かつmetadata blockは文書先頭のみに限定された=途中の密ペアはRule/setextのまま。0.11/0.12/0.13で実測確認）。その上で**2文法の使い分け**にする:

- **先頭frontmatterの検出**はmetadata有効文法で行い、**最初のイベントがYAML metadata blockで、かつその前に空白文字しかない**場合だけ採用する（空白プレフィックス規則。空行の後のfrontmatterはユーザーの意図が明白で、空白は内容を持たないため何もドロップしない）。生YAMLは**原文のまま**（trimしない）保持し、後段のYAMLエラー位置を「開始`---`の行番号+YAML内行番号」で源文に正確に写像する。スライド範囲はブロック終端の後から始める
- **スライド分割**は従来文法（metadata無効）で行う。2枚目以降の`---`はこれまでどおりスライド区切り/setextであり、既存デッキの意味を一切変えない
- 文書途中に`---`+`key: value`+`---`を書いた場合は従来どおりのCommonMark解釈（rule+setext見出し）になる。「途中frontmatter」を内容ヒューリスティックで推測してエラーにすることは**しない**（正当なデッキを誤検知でリグレッションさせるため。曖昧さは推測で解決しない原則どおり）。コメント等の非空白ブロックが先行する場合も同様にCommonMark解釈のまま（結果は可視のsetext見出しであり、サイレントドロップではない）
- **意図的な挙動変更**: `---`で始まるファイル（空白プレフィックス含む）はfrontmatter領域になる。先頭の密な`---`ペアでスライドを区切っていた既存デッキ（例: `---`/`# Title`/`---`）は、中身がYAMLマッピングでないため行番号+help付きビルドエラーになる（黙って1枚目が消えるのではなく明示的に落ちる。捕捉と検証は必ず一体で実装し、「捕捉だけして未検証」の中間状態をmainに置かない）
- `parse_slide`側は（metadata有効文法のため）slice先頭でMetadataBlockに遭遇し得る。その場合は明示エラー（`_ => {}`に飲ませない）。これは防御的な網であり、通常の分割経路では到達しない
- frontmatter検出中に想定外のイベントが来た場合も明示エラー（開いたブロックを黙って無視しない）

**サイレントミスを塞ぐ追加規則（実測起点、2026-07-04）**:
- **行形状ホワイトリスト**: 捕捉したfrontmatter本文は、末尾のスタイル的空行を除き、**全ての行が`key:`で始まるフラットな設定行**でなければ行番号+helpエラー。閉じ`---`忘れはmetadata blockが次の`---`までを飲み込むが（実測）、飲まれたmarkdownは行形状エラー（見出し・段落・空行・リスト）かunknown keyエラー（`note: ...`型）の必ずどちらかに落ち、黙って通る経路が構造的に存在しない。空行のみを禁止するブラックリストでは`# 見出し`（YAMLコメント扱い）をすり抜けるため不足だった
- **`---`開始ガード**: 最初の非空白行が`---`なのにfrontmatterが認識されなかった場合（opener直後の空行`---`/空行/`time: 15m`/`---`、空のペア`---`/`---`など）は行番号+helpエラー。「`---`で始まるファイルはfrontmatter領域」の位置規則であり内容の推測ではない
- **BOM除去**: 先頭のU+FEFFはパース前に1つ除去（BOMがあるとmetadata blockが認識されず設定が黙って無視されるため。実測）
- YAML本体は新規`DeckFrontmatter`（serde、`deny_unknown_fields`）にデシリアライズ。YAML crateは保守が継続しているserde互換のもの（serde_norway）をworkspace依存に追加
- `time`値は専用の型（例: `PlannedTime`）にカスタムDeserializeで解釈し、文字列/整数の両形式と不正値エラーを型の構築点で一元化する（消費側に検証を分散させない）
- パース結果は`Deck<Parsed>`にデッキ設定として載り、以降の相（Mapped/Checked/Rendered）へ携行される

### D3: フロントへの配線はmanifest.json（予定時間）+present.json（表示先）

**予定時間はManifestに載せる**。frontmatter由来のデッキメタデータなので、デッキを記述するmanifestが自然な運搬役。シェルは既にmanifest.jsonをfetchしている。

```rust
// crates/peitho-core/src/manifest.rs（既存構造体にフィールド追加）
pub struct Manifest {
    // 既存: version, peitho_version, title, slide_count, slides
    pub planned_duration_ms: Option<u64>,  // frontmatter time未指定ならNone
}
```

**表示先の判定材料はpresent.jsonに載せる**。「発表者ウィンドウを開くか」は起動時のランタイム知識であってデッキメタデータではないため、manifestには混ぜない。

```rust
// crates/peitho-core/src/present_config.rs（新規、manifest.rsと同型のパターン）
pub struct PresentConfig {
    pub version: u32,
    pub presenter_open: bool,
}
```

- どちらもts-rsで`bindings/*.ts`を生成・コミット（契約の単一source原則、CI drift検査）
- present.jsonは`emit_present_cache`で常に書き出す。**present-cache専用**で配布物dist/には含めない（非混入不変条件）。manifest.jsonは従来どおりdist/にも入るが、`plannedDurationMs`は単なるデッキメタデータで発表シェルではないので非混入条件に抵触しない

却下した代替案:
- **CLIフラグ`--time`**（初版設計）: 著者指示によりfrontmatter方式へ変更
- **予定時間もpresent.jsonに載せる**: 時間はデッキ由来の値なのでmanifestが正。present.jsonはランタイム構成のみに限定する
- **エントリHTMLへのJSON埋め込み**: 文字列組み立てになり型契約から外れる

### D4: 表示先の判定はCLIが起動時に確定する（`presenter_open`）

`presenter_open = !no_open && !no_presenter && ディスプレイレイアウト検出がSome`

- 2ディスプレイ（発表者ウィンドウあり）→ `true` → トラッカーは**発表者画面のみ**
- 1ディスプレイ＋既定Fullscreen（発表者ウィンドウなし）→ `false` → トラッカーは**プレゼン画面**
- `--presenter-windowed`（1画面デバッグ、両ウィンドウ）→ `true` → 発表者画面のみ
- `--no-presenter` → `false` → プレゼン画面
- 実装上、`present()`内のレイアウト検出を`emit_present_cache`より前に1回だけ行い、config書き出しとブラウザ起動の両方で同じ検出結果を使う

フロント側の表示規則:
- presenter.html: `manifest.plannedDurationMs != null`ならトラッカー表示
- present.html: `manifest.plannedDurationMs != null && !config.presenterOpen`ならトラッカー表示

**エッジケース（保守的判断）**: `--no-open`時は検出が走らないため`presenter_open=false`となり、ユーザーが手動で発表者画面も開くと両画面に表示され得る。また起動後にコントロールバーの「Presenter」ボタンで発表者画面を後から開いた場合もプレゼン画面側の表示は消えない。どちらも「配置は起動時に確定」という単純なモデルの帰結として許容する（動的なpresenter接続検出は/syncプロトコルへのロール概念追加が必要で過剰。必要になったら別Issue）。

### D5: トラッカーはシェル層のUI部品（§16イベント契約に準拠）

新規`packages/peitho-present/src/timeTracker.ts`に`installTimeTracker(options)`を実装。

- **読むだけ**: `peitho:slidechange`イベントでうさぎ位置を更新、`setInterval`（250ms、presenterタイマーと同じ）で`shell.elapsedMs()`を読んでかめ位置を更新
- **要求イベントのみ発行**: タイマー自動開始（D6）は`peitho:timercontrol {action:"start"}`のdispatchで行う。遷移・タイマーの実行主体はシェルのまま
- スライド本体（レイアウトHTML/テーマCSS）には一切触れない。オーバーレイはシェルのDOM
- 戻り値はcleanup関数（vitestのリスナー汚染対策の既存慣行どおり）

位置計算:
- うさぎ: `index / (total - 1)`（最終スライドで右端）。`total <= 1`のときは0%に固定（ゼロ除算ガード）
- かめ: `min(elapsedMs / plannedDurationMs, 1)`。超過時は右端に張り付き、トラッカーに超過状態属性（`data-peitho-overrun`）を付与して警告色にする

### D6: タイマーの自動開始

`time`設定時、**最初の前進ナビゲーション**（`peitho:slidechange`で`previousIndex !== null && index > previousIndex`）でトラッカーが`peitho:timercontrol start`をdispatchする。

- `startPresentation()`は開始済みなら何もしない（既存実装が冪等）ので、発表者画面の手動Startと競合しない
- 発表者画面の手動Start/Pause/Resume/Resetは従来どおり全て有効
- 理由: プレゼン画面のみの1スクリーン運用にはStartボタンが存在せず、自動開始がないとかめが永遠に0%のまま。スライドを進めた瞬間=発表開始とみなすのが最も自然

### D7: 見た目

- **トラック**: 画面下端の細いバー（プレゼン画面では高さ約6px・半透明でスライドの邪魔をしない。発表者画面ではサイドバー内にやや大きめに表示）
- **マーカー**: 🐰と🐢の絵文字（アセット不要、rabbit-turtleへのオマージュ）。重なったときも判別できるよううさぎを上段・かめを下段に僅かにずらす
- **発表者画面の数値表示**: 既存タイマー`MM:SS`を`time`設定時は`MM:SS / MM:SS`（経過/予定）に拡張し、超過時は`+MM:SS`の超過分を警告色で併記
- CSSは既存慣行どおりエントリHTML（render.rs）の`<style>`ブロックに追加。テーマCSS（themes/）には触れない（デザイン分離）
- 配布物dist/のビューアにはトラッカーを出さない（発表時の機能）

## 変更ファイル一覧

| ファイル | 変更 |
|---|---|
| `Cargo.toml`（workspace） | YAML crate（serde_norway）追加 |
| `crates/peitho-core/src/parser.rs` | `ENABLE_YAML_STYLE_METADATA_BLOCKS`有効化、`DeckFrontmatter`+`PlannedTime`、`split_slide_ranges`のmetadata block捕捉、`parse_slide`の先頭以外frontmatter明示エラー |
| `crates/peitho-core/src/phase.rs` | `Deck<Parsed>`以降にデッキ設定を携行 |
| `crates/peitho-core/src/manifest.rs` | `Manifest.planned_duration_ms`追加 |
| `crates/peitho-core/src/present_config.rs` | 新規: `PresentConfig`＋JSON化＋ts-rs export＋テスト |
| `crates/peitho-core/src/lib.rs` | モジュール公開 |
| `bindings/Manifest.ts` / `bindings/PresentConfig.ts` | ts-rs生成（コミット） |
| `crates/peitho/src/main.rs` | `present()`のレイアウト検出前倒し＋`emit_present_cache`でpresent.json書き出し |
| `crates/peitho-core/src/render.rs` | present.html/presenter.htmlのエントリスクリプトでmanifest/present.json取得→トラッカー配線、CSS追加 |
| `packages/peitho-present/src/timeTracker.ts` | 新規: `installTimeTracker` |
| `packages/peitho-present/src/presenter.ts` | タイマー表示拡張（経過/予定）、トラッカー設置 |
| `packages/peitho-present/src/index.ts` | export追加 |
| `packages/peitho-present/dist/shell.js` | 再ビルドしてコミット（drift検査） |
| `CLAUDE.md` | 著者判断の記録: ゼロコンフィグ+frontmatter設定方針（2026-07-03）、§18待ちリストからpeitho.toml前提を更新 |
| テスト | vitest（timeTracker単体・presenter統合）、Rust（frontmatterパース・PlannedTime・manifest・present_config・presenter_open判定） |

## テスト方針

- **frontmatterパース**: `time: 15m`/`time: 90s`/`time: 1h30m`/`time: 15`（整数分）/frontmatterなし/空frontmatter/未知キー（エラー+行番号+help）/不正time値（`0`、負、`abc`、空。エラー+help）/YAML壊れ（エラー）/**2枚目スライド以降の`---`が従来どおり区切りとして機能**/先頭以外のmetadata blockはエラー
- **Manifest/PresentConfig**: serdeラウンドトリップ、camelCaseフィールド名、ts-rs drift（既存`ts_tests`と同型）
- **timeTracker（vitest）**: うさぎ位置（先頭/中間/最終/1枚デッキ）、かめ位置（0%/50%/超過張り付き＋overrun属性）、自動開始dispatch（前進で発火・後退で発火しない・二重発火しない）、cleanup後にリスナーが残らない
- **presenter統合**: `time`あり→`MM:SS / MM:SS`表示、なし→従来表示
- **E2E（実ブラウザ必須）**: 1画面（トラッカーがプレゼン画面下端）/`--presenter-windowed`（発表者画面のみに表示、プレゼン画面に出ない）/`time`なし（どこにも出ない）を`--port`固定＋`curl POST /sync`＋`screencapture`で確認

## 壊してはいけないもの（セルフチェック）

- 三本柱1: 時間設定はデッキメタデータ（frontmatter）であってデザインではない。レイアウト/テーマに混ぜない
- 三本柱3: frontmatterの未知キー・不正値・位置違反をサイレントに飲まない。`_ => {}`禁止
- §16: トラッカーは要求イベント発行と状態読み取りのみ。実行主体はシェル
- typestate: デッキ設定は`Parsed`で確定し以降の相に携行（後段でのlookup失敗経路を作らない）
- dist/非混入: present.jsonはpresent-cacheのみ
- 契約単一source: Manifest/PresentConfigはRustが正、TSはts-rs生成
- 既存の発表者タイマー（Start/Pause/Resume/Reset）の挙動を変えない
- frontmatterなしの既存デッキ・examplesが無変更でビルドできる
