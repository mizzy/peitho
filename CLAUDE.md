# peitho

Markdownをsource of truthとするHTMLネイティブなプレゼンツール。設計の正は `docs/PEITHO_KICKOFF.md`（キックオフ仕様書）。設計判断に迷ったら仕様書§18「未決定事項」を見て、載っていない新規判断は著者に確認する。

## 三本柱（壊してはいけない不変条件）

1. **コンテンツとデザインの分離**: コンテンツはMarkdown、デザインはテンプレートHTML+CSS。混ぜない
2. **git管理可能なHTML/CSSテンプレート**: テンプレート自身がスキーマ（`<slot name accepts arity>`）。契約を別ファイルに分けない
3. **型検査されるスロット契約とキー付きoverride**: スロット過不足・型不整合・参照切れ・未割当コンテンツは全て行番号+help付きビルドエラー。**サイレントドロップ絶対禁止**（パーサに`_ => {}`で未知構造を飲ませない）

その他の不変条件:
- typestate `Parsed→Mapped→Checked→Rendered`。相の構築子はcrate内私有。未検査デッキはレンダラに渡せない（compile_failドクテストで固定）
- 契約の単一source: ドメイン型はpeitho-core(Rust)が正。TS型は`bindings/*.ts`にts-rsで生成しコミット。CIでdrift検査
- §16イベント契約: 遷移の実行主体はシェルのみ。UI部品は`peitho:navigate`/`peitho:timercontrol`等の要求イベント発行のみ。スライド本体はシェルの存在を知らない
- 配布物(dist/)に発表シェル・ノートを混ぜない（publishが非混入検査で門番）

## 構成

```
crates/peitho-core/   契約・パイプライン(parser/template/mapping/check/render/theme/manifest/notes)
crates/peitho/        CLI(build/present/publish)、server.rs(配信+/syncロングポール)、browser.rs、displays.rs
packages/peitho-present/  TS発表シェル(canvas/shell/controls/keyboard/sync/presenter)
bindings/             ts-rs生成TS型（コミット対象）
templates/ themes/ examples/  共有レイアウト・baseテーマ・サンプル
docs/plans/           各マイルストーンの実装計画（履歴）
```

## ゲート（全部通ってからコミット）

```
cargo test --workspace          # 3回連続（過去にテストレース事故あり）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/  # 契約drift
cd packages/peitho-present && npm run build && npm test && npm run typecheck
```

UX変更は必ず実ブラウザ/実ディスプレイでE2E確認する（jsdomはレイアウト・フラッシュ・ウィンドウ挙動を検出できない。過去に真っ黒画面・SSE不達・無限再ビルドをE2Eでのみ検出）。presentの確認は `--port`固定+`curl POST /sync`+`screencapture -x -D <n>` が便利。

## ハマりどころ（実測で確定済みの事実。再調査不要）

- **tiny_httpでSSEは不成立**: data_length Noneだと閾値以下のbodyをEOFまでバッファ、chunkエンコーダも小チャンクをflushしない。だから/syncはロングポーリング（`GET /sync?seq=N`、クエリ無しGETは現在seq即答=参加ハンドシェイク、`POST /sync`で`{index}|{close:true}`）
- **Chrome既起動インスタンスへのフラグhandoffは`--app`しか効かない**: `--start-fullscreen`/`--window-position`/`--window-size`は無視される。確実に効かせるには別`--user-data-dir`で新規プロセス起動（だからslides/presenterは`~/.peitho/chrome-profile-{slides,presenter}`の2インスタンス）
- **macOSのChromeは全ウィンドウを閉じてもプロセスが残る**: 前回presentのインスタンスがpeithoプロファイルを掴んだままだと次回起動がhandoffになり配置フラグが全滅する。だからpresent起動時に残存プロセスを終了してから開く
- **Chromeの残存プロセスはSIGTERM killではなく正規終了させる**: SIGTERMはChrome的にクラッシュ（`exit_type: Crashed`）で、次回起動でクラッシュ復元が走り古いセッションの窓・boundsが復活する。`NSRunningApplication.terminate`（JXAではブリッジの都合で**括弧なし**アクセスで発火・実測）で終了させるとNormal。なお`exit_type`は起動中は常にCrashed表示（起動時に先書き→正常終了でNormalに戻す仕様）なので稼働中の読み取りは無意味。対象pidは`ps`からChromeメインプロセス（`--type=`なし）だけに絞る（`pgrep -f`はパターンを含むシェル自身まで拾う）
- **Chromeの`--app`ウィンドウ位置復元はapp名にドットがあると壊れる**: 位置は`browser.app_window_placement`にURL由来のapp名（host+path）で保存されるが、名前中のドット（`127.0.0.1`や`.html`）が書き込み時にprefパスとして展開され、読み出しと不一致になり復元されない（Chromiumの実挙動、実測）。だからpresenterは`http://localhost:<port>/presenter`（拡張子なしルート）で開く。app名にポートは含まれないのでポートが毎回変わっても復元は効く
- **配置フラグ無しの`--app`初回起動はウィンドウがどのディスプレイに出るか不定**: slides側ディスプレイに出るとフルスクリーンSpaceの裏に完全に隠れる（CGWindowListのOnScreenOnlyにも出ない）。だからwindowedモードは、保存placementが無い/不可視位置のときだけ`--window-position`+`--window-size`でプライマリ中央にシードし、可視な保存placementがあるときだけフラグ無しでChrome復元に任せる（判定はpeithoがプロファイルのPreferencesを読む）。Chromeは部分的に画面外の保存boundsを起動時にクランプする
- **別プロファイル間はBroadcastChannelが届かない**: だから同期はサーバ経由（§15からの意図的拡張。層分け=DOMイベント⇔トランスポート橋渡しは不変）
- **CLI起動のappウィンドウは`window.close()`で閉じられる**（履歴1エントリのため）。Escは`peitho:closerequest`→`{close:true}`全窓配信→各自close→サーバも猶予後にunblockして終了
- **requestFullscreen/window.openはtransient user activation必須**: permission promptのawaitを挟むと失効する。ブラウザ内でのウィンドウ配置はこれで2敗した末にCLI主導へ転換した経緯（M8/M9/M10）
- NSScreenはbottom-left原点。Chromeの`--window-position`はtop-left。変換は`displays.rs`（純関数+実測値テスト）
- vitestテストではシェル/リスナーを必ずdestroy/cleanup（共有windowのリスナー汚染で多重発火する）
- `.peitho/present-cache/`は毎回作り直し（§18キャッシュ方針の採用値）。`dist/slides/`もビルド毎にクリア（stale断片の公開漏れ防止）

## 未決定・著者判断待ち（勝手に決めない）

- スピーカーノートのMarkdown記法（notes.jsonスキーマ・TS型・presenter表示配線は実装済み、常に空）
- fenced div明示スロット記法 `::: {slot=...}`（§18）
- 型駆動レイアウトディスパッチ（§18）
- シンタックスハイライタ選定
- デッキ別リポジトリ運用の設定ファイル（peitho.toml等）とpeitho.gosu.keデプロイ

残タスクはGitHub Issuesに登録済み。着手時は`docs/plans/`に計画を書いてから実装する。
