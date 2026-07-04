# Presenter timer urgency colors (Issue #107)

## 意図

プレゼンター画面のタイマーは、残り時間に応じて色を変え、発表者が一目で残り時間の逼迫を認識できるようにする。

- 残り3分〜1分: warning（黄/アンバー）
- 残り1分〜0分: urgent（赤、overrunと同色。区別は state="running" ゲートで表現）
- 経過 ≥ 計画時間: overrun（赤、既存の `--warn`）
- `plannedDurationMs` 未設定: 中立色（現状維持）
- 計画時間が短い場合、閾値をスキップし最も強い該当状態から開始（例:2分デックはwarning開始、40秒デックはurgent開始）

## 設計

### 単一の派生関数と単一のDOM属性

タイマーの表示状態を1つの純関数から派生させる。`data-peitho-urgency` 属性を1つだけ切り替える。

```ts
export type TimerUrgency = "normal" | "warning" | "urgent" | "overrun";

export function urgencyFor(
  elapsedMs: number,
  plannedDurationMs: number | null
): TimerUrgency;
```

閾値:
- `plannedDurationMs == null` → `"normal"`
- `elapsedMs > plannedDurationMs` → `"overrun"`
- 残り時間 `remaining = plannedDurationMs - elapsedMs` に対して:
  - `remaining ≤ 60_000` → `"urgent"`
  - `remaining ≤ 180_000` → `"warning"`
  - それ以外 → `"normal"`

境界の閉じ方は `≤`（残り1分ちょうど時点でurgentに切り替わる）。

### なぜ単一属性か（3レンズ）

- **long-term**: 将来閾値を増やしても `urgencyFor()` 1関数の変更で済む。CSSも属性値マッチ1箇所で完結。
- **type-safety**: `type TimerUrgency` union型で網羅性保証。CSSも `[data-peitho-urgency="..."]` 4値をカバー。
- **root-cause**: 「経過時間と計画時間からの派生値」というただ1つの派生を、ただ1つの純関数から作る。upstream。

### 既存の `[data-peitho-overrun]` 属性の扱い

- `[data-peitho-presenter="timer"][data-peitho-overrun]` は既に存在し、色を `var(--warn)` に切り替えている。この属性は `.overrun` span（`+MM:SS` の表示）の可視条件でもあると読めるが、実は `.overrun` span は `.timer` の子要素として直接 `color: var(--warn)` を持つので、属性トグルは色目的専用。
- `urgencyFor` で `"overrun"` を返せば `data-peitho-urgency="overrun"` が同じ色を実現するため、**`data-peitho-overrun` の切り替えは撤去**する（単一属性に統一）。既存の `[data-peitho-presenter="timer"][data-peitho-overrun]` セレクタも撤去し、`.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer` に置き換える。
- `.peitho-time-tracker[data-peitho-overrun]` はtracker側の色切替なので**そのまま残す**（別コンポーネント）。

### CSS変更

`crates/peitho-core/src/render.rs` の `render_presenter_index()` 内 `<style>` ブロックに追加/変更:

```css
.clock[data-peitho-urgency="warning"] .timer,
.clock[data-peitho-urgency="warning"] .timer .planned { color: var(--pause); }
.clock[data-peitho-urgency="urgent"] .timer,
.clock[data-peitho-urgency="urgent"] .timer .planned { color: var(--warn); }
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer,
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer .planned { color: var(--warn); }
```

既存の `--pause`（アンバー）と `--warn`（赤）を直接参照する。値は「urgency-warning=pause」「urgency-urgent=overrun=warn」と一致するため、独立変数を作らず参照を統一する（overrunルールは元から `--warn` を直接使っていて、独立変数を作ると同PR内で不整合になる）。

既存の `.clock[data-peitho-state="paused"] .timer` と `.clock[data-peitho-state="stopped"] .timer` は残す。urgencyより timer state の色（paused=amber, stopped=dim）が優先されると自然（停止中に赤くしても意味がない）。

**優先順位の実装:** CSSは後勝ちなので、`:root`セレクタで書き終えた後に state セレクタを urgency の後に置く。あるいは `.clock[data-peitho-state="running"][data-peitho-urgency="warning"]` のように running時のみ urgency を適用する形が明示的で安全 → こちらを採用。

```css
.clock[data-peitho-state="running"][data-peitho-urgency="warning"] .timer,
.clock[data-peitho-state="running"][data-peitho-urgency="warning"] .timer .planned { color: var(--pause); }
.clock[data-peitho-state="running"][data-peitho-urgency="urgent"] .timer,
.clock[data-peitho-state="running"][data-peitho-urgency="urgent"] .timer .planned { color: var(--warn); }
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer,
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer .planned { color: var(--warn); }
```

注: overrunだけは stopped/paused でも赤にしたい（超過してから停止 → 依然として超過を知らせる）。→ overrunに限りstate値を限定しない compound セレクタを追加:

```css
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer,
.clock[data-peitho-state][data-peitho-urgency="overrun"] .timer .planned { color: var(--warn); }
```

`[data-peitho-state]` も含めて state ルールより specificity を高くし、stopped/paused でも overrun は赤、それ以外は state色（stopped=dim, paused=amber）が勝つ。

### TypeScript側の統合

`packages/peitho-present/src/presenter.ts`:

- `urgencyFor()` を別モジュール `timerUrgency.ts`（新設）にエクスポートし、presenterから import
- `tick()` で `data-peitho-urgency` を `clockRoot` に set:
  ```ts
  clockRoot.dataset.peithoUrgency = urgencyFor(elapsedMs, plannedDurationMs);
  ```
- 既存の `timerRoot.toggleAttribute("data-peitho-overrun", ...)` は撤去

### テスト（TDD）

`packages/peitho-present/test/timerUrgency.test.ts`（新規）:
- `plannedDurationMs == null` → `"normal"`
- 残り時間 > 3分 → `"normal"`
- 残り時間 = 3分ちょうど → `"warning"`
- 残り時間 = 1分1秒 → `"warning"`
- 残り時間 = 1分ちょうど → `"urgent"`
- 残り時間 = 1秒 → `"urgent"`
- 残り時間 = 0 → `"urgent"`（elapsed == planned はまだoverrunではない、既存 `isOverrun` の `>` 条件と合わせる）
- elapsed > planned → `"overrun"`
- planned = 2分 → elapsed=0で`"warning"`（3分閾値は範囲外なので skip、warning から開始）
- planned = 30秒 → elapsed=0で`"urgent"`（両閾値をskip、urgentから開始）

`packages/peitho-present/test/presenter.test.ts` に追加テスト:
- presenterでtickすると `clockRoot.dataset.peithoUrgency` が期待値に更新される
- planned=10分, elapsed=8分1秒 → urgency="warning"
- planned=10分, elapsed=9分1秒 → urgency="urgent"
- planned=10分, elapsed=10分1秒 → urgency="overrun"
- planned=null → urgency="normal"

### 影響範囲

- `packages/peitho-present/src/timerUrgency.ts`（新規）
- `packages/peitho-present/src/presenter.ts`（tick更新）
- `packages/peitho-present/test/timerUrgency.test.ts`（新規）
- `packages/peitho-present/test/presenter.test.ts`（追加テスト）
- `crates/peitho-core/src/render.rs`（CSS追加、色変数追加）
- `crates/peitho/tests/present.rs`（presenter HTMLに新CSSセレクタが含まれるかのアサーション追加、必要に応じ）
- `packages/peitho-present/dist/shell.js`（`npm run build` で再生成、コミット）

### 検証

- E2Eは `docs/plans/2026-07-04-presenter-redesign.md` の手順に準ずる
- `plannedDurationMs = 300_000` の10分デックで、開始→3分経過（残り2分）で黄、4分経過（残り1分）でオレンジ、6分経過で赤を目視確認
