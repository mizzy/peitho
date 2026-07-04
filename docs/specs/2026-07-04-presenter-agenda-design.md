# 発表者画面アジェンダ設計 (2026-07-04)

## ゴール

Claude Designモック(`Presenter.dc.html`)にあるAgendaセクションを実装する。deckを名前付きセクションに分割し、発表者画面のタイマーカード内(トラッカーとcontrolsの間)に、セクションごとの「名前・スライド範囲・実績/計画時間・差分」を表示する。セクションを宣言していないdeckでは何も表示しない(現状表示のまま)。

デザインの出自: https://claude.ai/design/p/73ba6a5b-288b-46f7-a2c3-cb06863e3c5b の `Presenter.dc.html` の `.agenda` 系。反映後は `render_presenter_index()` のCSSが正。

## 採用アプローチと理由

**Approach A: manifest経由・クライアント側計測**(2026-07-04 著者承認済み)

- セクションはページコメントで宣言し、core(Rust)がパース時に解決・検証して `manifest.json` に載せる。manifestは契約の単一ソースであり、セクションはdeck構造の一部なのでここに置く
- 実績時間はpresenterウィンドウがクライアント側で計測する。メモリ内のみ(リロードで消える)— 既存タイマーがリロードで消えるのと一貫した挙動

捨てた代替案:
- sections.jsonサイドカー方式 — deck構造をmanifestの外に分離する理由がない
- 実績のshell/サーバー同期永続化 — タイマー自体が永続化されない現状と非対称になる過剰設計

## 記法(著者決定 2026-07-04)

セクションの先頭スライドのページコメントで宣言する。frontmatter方式・見出し自動導出方式は不採用(著者決定)。frontmatterのフラットkey制約(2026-07-03著者判断)は不変。

```markdown
---
time: 15m
---

<!-- {"section": "Setup", "time": "1m"} -->
# タイトル

---

<!-- {"section": "Why HTML decks", "time": "3m", "layout": "cover"} -->
# なぜHTMLか
```

- `section` = セクション名(表示ラベル)。次の`section`マーカーの直前まで(最後はdeck末尾まで)がそのセクションのスライド範囲。範囲はビルド時に自動導出される — スライドの挿入・並べ替えでレンジ宣言がずれる事故が構造的に起きない
- `time` = そのセクションの計画時間。deck frontmatterの`time`と同じ文法(`15m`/`90s`/`1h30m`/裸整数=分)・同じ`PlannedTime`型・同じ検証(非ゼロ、上限)を再利用する
- 既存の`key`/`layout`と同じコメント内に同居できる。`PageComment`は`deny_unknown_fields`のまま

## 検証ルール(すべて行番号+help付きビルドエラー。サイレントパスなし)

1. `section`には`time`必須(省略を許すと合計導出の意味が壊れる)
2. `time`だけで`section`が無いページコメントはエラー(deck全体の`time`はfrontmatterに書く旨をhelpに)
3. `section`が空文字列はエラー
4. マーカーがひとつでも存在する場合、**先頭スライドにマーカー必須**。無ければエラー(暗黙の無名セクションは作らない — 曖昧さの暗黙解決の禁止)
5. **合計一致検証**(著者決定 2026-07-04): frontmatter `time`とセクション`time`合計が両方あれば一致必須。不一致は両方の値をメッセージに含めてエラー。frontmatter `time`省略時はセクション合計を総時間として導出する。合計は`checked_add`で計算し、オーバーフローと`PlannedTime`上限超過はエラー
6. セクション名の重複は許容(キーではなく表示ラベル。位置ベースで状態判定するため一意性は不要)

検証はパース終端(`parse_markdown`内、全スライドのマーカーが出揃った時点)で一度だけ行い、解決済みの`Vec<DeckSection>`を`Deck<Parsed>`構築時に確定させる。以降のフェーズは検証済みの値を運ぶだけ(`PlannedTime`と同じ「構築時に一度だけ検証」の方針)。**導出後は`DeckSettings`の`planned_time`が常に総時間を持つ**(省略時はここで合計から埋める)ので、下流(トラッカー・manifest `plannedDurationMs`)は無変更で動く。

## 型とデータフロー

```
PageComment { key, layout, section, time }        parser.rs(deny_unknown_fields維持)
  ↓ パース終端で解決・検証
DeckSection { name: String, planned: PlannedTime, start: usize, end: usize }  phase.rs
  ↓ DeckSettings { planned_time, sections: Vec<DeckSection> } に格納
     (Copyが外れる — 機械的変更。Parsed→Mapped→Checked→Renderedを既存のplanned_timeと同経路で通す)
  ↓ build_manifest
Manifest { ..., sections: Vec<ManifestSection> }   manifest.rs
ManifestSection { name, startIndex, endIndex, plannedDurationMs }
  ↓ ts-rs → bindings/Manifest.ts, bindings/ManifestSection.ts(コミット+CI drift検査)
presenter.ts / agenda.ts(新設)が shell.manifest.sections を消費
```

- `start`/`end`は`ManifestSlide.index`と同じ0-basedインデックス(表示時に1-based化)。ミリ秒はmanifest境界でのみ出現(既存方針)
- `sections`はセクション無しdeckでは空配列(フィールド自体は常に存在 — TS側でoptional分岐を作らない)
- manifest `version`は1のまま(追加的変更。golden test `serializes_manifest_schema_exactly`を更新)
- dist(publish)のmanifestにも`sections`は載るが、present(聴衆)側shellは読まない。発表シェル/notesをdistに混ぜない方針とは無関係(manifestは元々dist契約の一部)

## 実績時間の計測(著者決定 2026-07-04: 累積方式)

- 250ms tickごとに`elapsedMs()`の前回値との差分を「**いま表示中のスライドが属するセクション**」に加算する。戻って再説明した時間もそのセクションの実績に入る。pause中は`elapsedMs`が進まないので自然に除外される
- `done`/`current`/`upcoming`は現在スライドの位置基準で判定: current = 現在スライドが属するセクション、done = それより前、upcoming = それより後。前のセクションに戻ればそこが再びcurrentになる
- doneの`under`/`over`はライブ判定: 実績≤計画なら`under`、超えたら`over`
- タイマー未開始(stopped)時は全セクション実績0。実績表示はモックに従い、done/currentは`実績 / 計画`、upcomingは`— / 計画`、差分はdoneのみ`±M:SS`表示(current/upcomingは`·`)
- 時間表示フォーマットはトラッカー目盛りと同じ`m:ss`(`timeTracker.ts`の既存フォーマッタを共有)

## 実装の配置

**Rust (crates/peitho-core)**
- `parser.rs`: `PageComment`拡張、`section`/`time`検証、パース終端のセクション解決+検証(エラーは既存の`ErrorKind::Parse`+行番号+help)
- `phase.rs`: `DeckSection`新設、`DeckSettings`に`sections`追加(`Copy`外し)、アクセサ
- `manifest.rs`: `ManifestSection`新設、`Manifest.sections`追加、goldenテスト更新、ts-rsテスト追加
- `render.rs` `render_presenter_index()`: モックの`.agenda`系CSSを移植(セレクタは`data-peitho-*`ベースに書き換え)。renderテスト更新

**TS (packages/peitho-present)**
- `agenda.ts`新設(`timeTracker.ts`と同型の構造): `installAgenda({root, shell, sections})`。`peitho:slidechange`購読+250ms interval。teardown必須(vitestのリスナー汚染対策)。`sections`が空なら何もマウントしない
- `presenter.ts`: `.clock`カード内、tracker-slotとcontrolsの間にagenda-slotを追加し、`manifest.sections`が非空のときだけ`installAgenda`。**`.clock`はflex column+`.controls { margin-top: auto }`を維持**(gridにするとボタンが縦に太るバグが再発する — 実測済み)
- `bindings/`再生成コミット、`dist/shell.js`再ビルドコミット

**examples**
- `examples/lightning-talk/deck.md`にセクションマーカーを追加(E2E確認とドキュメントを兼ねる)

## DOM/CSSの制約

- DOMフックは`data-peitho-*`属性のみ(クラス依存セレクタ禁止 — 確定済み設計判断)。状態は`data-peitho-agenda-state="done|current|upcoming"`+`data-peitho-agenda-delta="under|over"`で表現し、CSSはそれに当てる
- モックの`.agenda`は`overflow: hidden`(はみ出しは切る)。セクション数が多い場合のスクロール対応は今回スコープ外(必要になったら別Issue)
- present(聴衆)側の`timeTracker` presentバリアントDOMは**バイト不変**(スナップショットテストで固定)。触らない

## エッジケース一覧

| ケース | 挙動 |
|---|---|
| セクションマーカー無しdeck | アジェンダ非表示。既存挙動完全不変 |
| `time`なしの`section` | ビルドエラー(help: timeを書く) |
| `section`なしの`time` | ビルドエラー(help: deck全体はfrontmatterの`time`) |
| 空文字列の`section` | ビルドエラー |
| 先頭スライドにマーカー無し(他にマーカーあり) | ビルドエラー(最初のマーカー行を指す) |
| frontmatter `time`とセクション合計の不一致 | ビルドエラー(両値をメッセージに) |
| frontmatter `time`なし+セクションあり | 総時間=合計。トラッカーも出る |
| セクション合計のオーバーフロー/上限超過 | ビルドエラー |
| セクション名重複 | 許容(表示ラベル) |
| 1スライドだけのセクション/セクション1個 | 許容 |
| 前セクションへ戻る | そこがcurrentに戻り、実績が累積再開 |
| presenterリロード | 実績消失(タイマーと同じ) |
| タイマーpause | 実績加算停止(`elapsedMs`準拠) |

## スコープ外

- アジェンダ行のクリックでのセクションジャンプ(要望が出たら別Issue)
- 実績の永続化・リロード復元
- セクション多数時のスクロールUI
- present(聴衆)側でのセクション表示
