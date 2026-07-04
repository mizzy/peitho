# Time tracker scale labels — fix last-label alignment (Issue #97)

## 現状

presenter time tracker のスケール(0:00 / 1:15 / 2:30 / 3:45 / 5:00)は
`display: grid; grid-template-columns: repeat(5, 1fr)` の 5 セル左端に配置される。
結果、最終ラベル「5:00」は最後のセルの開始位置 = バー全幅の 4/5 (80%) に出て、
バー右端(=計画時間の位置)と一致しない。

## 設計方針(author 承認済み)

各ラベルはバー上の 0% / 25% / 50% / 75% / 100% の**時点**を指す。

- **配置**: `position: absolute` + `left: N%` で各ラベルを配置
- **アライメント**:
  - 先頭 (0%): 左揃え (`translateX(0%)`)
  - 中間 (25%, 50%, 75%): 中央揃え (`translateX(-50%)`)
  - 末尾 (100%): 右揃え (`translateX(-100%)`)

これは既存の rabbit/turtle マーカーと同じ流儀(インライン style で `left` +
`transform: translateX(...)` を付与)なので、CSS には generic な
`.tracker-scale span { position: absolute; }` だけを置き、位置合わせは
timeTracker.ts が付ける個別のインライン style で行う。

### なぜインライン style か

`crates/peitho-core/src/render.rs` の presenter CSS テストに
`assert!(!html.contains("transform: translateX(-50%)"))` があり、CSS 内で
translateX を宣言できない。既存 rabbit/turtle と同様に TS からインラインで
付与する形が一貫していて、この制約とも整合する。

## 変更対象

### `packages/peitho-present/src/timeTracker.ts`

1. `timeScaleLabels` を、テキストと配置情報を返す形へ拡張(or 呼び出し側で
   index から派生させる)
2. presenter バリアントの `<div class="tracker-scale mono">…</div>` 生成箇所で、
   5 個の span それぞれに `style="left: X%; transform: translateX(Y%)"` を付与
   - index 0: `left: 0%; transform: translateX(0%)`
   - index 1: `left: 25%; transform: translateX(-50%)`
   - index 2: `left: 50%; transform: translateX(-50%)`
   - index 3: `left: 75%; transform: translateX(-50%)`
   - index 4: `left: 100%; transform: translateX(-100%)`

### `crates/peitho-core/src/render.rs`

CSS を以下のように書き換え:

```css
.tracker-scale { position: relative; height: 12px; margin-top: 6px; color: var(--fg-dim); font-size: 10px; letter-spacing: 0.08em; }
.tracker-scale span { position: absolute; top: 0; white-space: nowrap; }
```

削除:
- `display: grid; grid-template-columns: repeat(5, 1fr)`
- `border-left: 1px solid var(--line-soft); padding-left: 6px;` の cell 境界線
- `:first-child { border-left: none; padding-left: 0; }`

理由: 新しいレイアウトでは cell 概念自体が消えるので、cell 境界線も不要。
Issue にも「先頭は左揃え、中間は中央揃え、末尾は右揃え」とあり、cell 境界線
の意匠は保持要件に入っていない。

### `crates/peitho-core/src/render.rs` (テスト側)

新規アサーションを追加:

```rust
assert!(html.contains(".tracker-scale { position: relative;"));
assert!(html.contains(".tracker-scale span { position: absolute;"));
assert!(!html.contains(".tracker-scale { display: grid;"));
```

`assert!(!html.contains("transform: translateX(-50%)"))` はそのまま維持
(CSS 内には現れず、インライン style としてのみ生成される)。

## テスト

### `packages/peitho-present/test/timeTracker.test.ts`

既存 "renders presenter variant with legend fill track and five-point time scale"
テストの末尾に以下のアサーションを追加:

```ts
const scaleSpans = Array.from(tracker.querySelectorAll<HTMLElement>(".tracker-scale span"));
expect(scaleSpans.map((s) => s.style.left)).toEqual(["0%", "25%", "50%", "75%", "100%"]);
expect(scaleSpans.map((s) => s.style.transform)).toEqual([
  "translateX(0%)",
  "translateX(-50%)",
  "translateX(-50%)",
  "translateX(-50%)",
  "translateX(-100%)"
]);
```

これで「末尾は right-align」「先頭は left-align」「中間は center-align」が
DOM レベルで保証される。

既存の "keeps the present variant DOM unchanged" テストは変更なし
(present バリアントは今回の修正対象外、DOM は不変を維持)。

## Verify

- `cargo test --workspace` x3
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` (ts-rs 契約に変更なし)
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js` (再ビルド後)

## E2E (author が実機確認)

`peitho present examples/deck-with-time.md --port 8080` で presenter を開き、
スケールの「5:00」がバー右端に揃うことを目視。
