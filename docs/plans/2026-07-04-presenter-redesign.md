# Presenter view redesign (Claude Design mock reflection)

2026-07-04. Claude Designで作成したモック(project `73ba6a5b`/`Presenter.dc.html`、https://claude.ai/design/p/73ba6a5b-288b-46f7-a2c3-cb06863e3c5b)を発表者画面に反映する。反映後は `render_presenter_index()` のCSSが正。

## デザイン概要

- ダーク基調(oklch)+cyanアクセント、Geist/Geist Monoフォント(Google Fonts、オフライン時はsystem-uiへフォールバック)
- 左カラム: ステータス行(Now・Slide N of M・デッキタイトル)→ 16:9固定の現在スライド(container query)→ キーボードヒント行 → Notesカード
- 右カラム: Nextスライドカード(16:9)→ タイマーカード(大型タブラー数値+state pill+新見た目のトラッカー+コントロール)
- タイマー状態(stopped/running/paused)をstate pill・タイマー色・Playボタンのラベル/色に同期
- Start/Pause/Resumeを1つのPlayボタンに統合(状態からactionを導出、イベント契約`peitho:timercontrol`は不変)
- ボタン押下フィードバック: 沈み込み+色反転+クリック位置からのリップル(`prefers-reduced-motion`で無効化)

## スコープ外(保守的判断、報告に明記)

- モックにあるAgendaセクション(セクション別実績/計画時間): peithoにセクション概念・区間時間データが存在しないため除外。必要なら別Issueで検討
- ステータス行の「Section — ...」表示: 同上
- キーバインド変更: Space=nextの既存マップは不変。UI表記を実挙動に合わせる(PlayボタンにSpaceヒントを付けない)

## 変更ファイル

1. `packages/peitho-present/src/presenter.ts`
   - DOM構造を新デザインに全面変更(data-peitho-presenter / data-peitho-action フックは維持・拡張)
   - タイマー状態導出: `startedAt()===null`→stopped、`isPaused()`→paused、他→running
   - `data-peitho-action="playpause"`ボタン: stateに応じ start/pause/resume をdispatch
   - tick()でstate pill/playラベル/クロックカードの`data-peitho-state`を更新
   - タイマー表示をspan分割(planned/overrunの色分け)。textContentは既存フォーマット互換
   - リップル用pointerdownハンドラ(`--rx`/`--ry`+`.pressed`)
2. `packages/peitho-present/src/timeTracker.ts`
   - `variant: "presenter"`のときのみ legend(Slide progress/Time)+`.tracker`ラッパ+`.fill`(時間進捗幅)+scale(計画時間5分点)を生成
   - presentバリアントのDOMは不変。マーカー移動ロジック(left%+translateX clamp)は共通のまま
3. `crates/peitho-core/src/render.rs` `render_presenter_index()`
   - CSS全面差し替え+Google Fontsリンク。Agenda関連は含めない
   - `.clock`はflex column+`.controls { margin-top: auto }`(グリッドstretchによるボタン肥大の再発防止)
4. テスト更新
   - `packages/peitho-present/test/presenter.test.ts`: playpause統合ボタンの状態遷移、state pill、新フック
   - `packages/peitho-present/test/timeTracker.test.ts`: presenterバリアントのfill/scale追加分(既存presentテストは不変)
   - `crates/peitho-core/src/render.rs`のpresenterテスト2件を新CSSアサーションへ
5. `packages/peitho-present/dist/shell.js` 再ビルド+コミット

## ゲート

CLAUDE.md記載の全ゲート(cargo test×3 / clippy / fmt / bindings drift / npm build+test+typecheck / shell.js drift)+実ブラウザE2E(examplesをbuild→present→スクリーンショット)。
