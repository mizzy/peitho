# presenter windowed mode（デバッグ用）

## 目的

`peitho present`にpresenterウィンドウをフルスクリーンにしないデバッグモードを追加する。BetterDisplayの仮想ディスプレイと組み合わせた動作確認で、presenterが常時フルスクリーンだと検証しづらいため。

## 現状の問題（root cause）

`displays.rs`の`plan_presentation_layout`はpresenterに`WindowPlacement { fullscreen: false, width: 1200, height: 800, .. }`を計画するが、`browser.rs`の`chrome_presenter_args`はこれを無視して常に`--start-fullscreen`を付ける。つまり`WindowPlacement.fullscreen`とサイズは死にフィールドで、「計画」と「実行」が乖離している。

単にフラグ分岐を`chrome_presenter_args`に足すのは症状パッチ。正しくは**WindowPlacementを唯一の真実にする**:

- `plan_presentation_layout`が意図（presenterはデフォルトでフルスクリーン、windowedモードなら1200x800中央）を`WindowPlacement`に完全に書き込む
- `browser.rs`は`placement.fullscreen`に従い、`--start-fullscreen`か`--window-size={w},{h}`のどちらかを機械的に出す。判断はしない

これで将来の呼び出し元も`WindowPlacement`を作れば正しいコマンドが出る（フラグを覚えておく必要がない）。

## 変更

1. `displays.rs`
   - `plan_presentation_layout(displays, presenter_windowed: bool)`に引数追加
   - デフォルト（false）: presenter placementは`fullscreen: true`（現行の実挙動を型に反映）
   - windowed（true）: `fullscreen: false`、1200x800（プライマリにクランプ）中央配置
   - `layout_from_jxa_output` / `detect_presentation_layout`にも同引数を貫通
2. `browser.rs`
   - `chrome_presenter_args`: `placement.fullscreen`がtrueなら`--start-fullscreen`、falseなら`--window-size={width},{height}`
   - slides側は常に`fullscreen: true`の計画なので挙動不変
3. `main.rs`
   - `present`に`--presenter-windowed`フラグ追加、`PresentOptions`経由で`detect_presentation_layout`へ

## テスト（TDD順）

- displays: windowedモードでpresenter placementが`fullscreen: false` + クランプ済みサイズになる / デフォルトは`fullscreen: true`
- browser: presenter placementが`fullscreen: false`のとき`--window-size=1200,800`が出て`--start-fullscreen`が出ない / デフォルトは従来どおり`--start-fullscreen`
- main: `peitho present deck.md --presenter-windowed`がparseできる

## E2E

2ディスプレイ環境でのみpresenterが開くため、実ディスプレイ構成に依存する。ディスプレイ数をJXAで確認し、2枚あれば`--port`固定+`--presenter-windowed`で起動→screencaptureで窓モードを確認→`curl POST /sync`の`{close:true}`で全窓クローズ。1枚しかなければ単体テストとコマンド生成の実出力確認まで（報告に明記）。

## E2Eで発覚した追加root cause（実装済み）

初回E2Eで`--window-size`が無視され、presenterがほぼ全画面で開いた。原因は前回presentセッションのChromeプロセス: macOSのChromeは全ウィンドウを閉じてもプロセスが残り、peithoプロファイルを掴んだままになる。そこへ`open -na`すると既起動インスタンスへのhandoffになり、`--app`以外の配置フラグが全滅する（既存の`--window-position`/`--start-fullscreen`も2回目以降のpresentでは同様に効いていなかった）。

対処: `open_browser_with_request`で起動前に`pkill -f -- "--user-data-dir=<profile>"`で残存プロセスをkillし、pgrepで消滅を確認してからspawnする（`terminate_stale_profile_instances`）。pkillはパターンが`--`始まりだと`--`セパレータが必須（無いとexit 2で何もkillされない、実測）。

## 追記: windowedは位置指定をやめChrome復元に任せる（著者指示）

当初は`--window-position`+`--window-size=1200,800`を明示していたが、デバッグ中に手で動かした位置・サイズを次回も使いたいという著者要望で、windowedモードでは配置フラグを一切渡さない形に変更。Chromeはプロファイルの`Preferences`（`browser.window_placement`）に最後のウィンドウ位置を保存し、フラグが無ければそれを復元する（実測確認済み）。

これに伴い`WindowPlacement`をstruct（x/y/width/height/fullscreen）からenum `Fullscreen { x, y } | Restored`に再構成。structのままだとwindowed時にwidth/heightが再び死にフィールドになるため。サイズのクランプ計算はフルスクリーン時の中央座標算出のローカル計算に残る。初回起動（プロファイルに保存値が無い場合）はChromeのデフォルト位置で開く（許容済みトレードオフ）。

## 追記2: 復元を成立させるために潰した2つのroot cause

E2Eで「動かした位置に復元されない」が再現し、調査で以下2点が確定した（詳細はCLAUDE.mdハマりどころ）。

1. **SIGTERM killはクラッシュ扱い**: 前PRの`pkill`による残存プロセスkillは`exit_type: Crashed`を残し、次回起動のクラッシュ復元が古いセッションの窓・boundsを復活させ、保存placementを上書きしていた。対処として残存プロセスは`NSRunningApplication.terminate`（osascript JXA、括弧なしアクセスで発火）で正規終了させ、タイムアウト時のみpkillへエスカレーション。対象pidは`ps`でChromeメイン（`--type=`なし、パターン含有）に絞る。
2. **app名のドットでplacement prefが壊れる**: `--app`窓の位置は`browser.app_window_placement`にURL由来のapp名で保存されるが、`127.0.0.1_/presenter.html`のドットが書き込み時にネスト展開され読み出しと不一致→復元が一度も効いていなかった。presenterのURLを`http://localhost:<port>/presenter`（server.rsに拡張子なしルート追加）に変え、app名`localhost_/presenter`をドットフリーに。app名にポートは含まれないため、presentのポートが毎回変わっても復元は維持される（スクラッチ環境のChrome実機で移動→正規終了→再起動→移動先復元を確認）。

さらにE2Eで、配置フラグ無しの初回起動はChromeがウィンドウをslides側ディスプレイに置くことがあり、フルスクリーンSpaceの裏に完全に隠れると判明。最終形:

- `PresenterMode::Windowed { saved: Option<SavedWindowBounds> }`: peithoがpresenterプロファイルのPreferencesから`localhost_/presenter`の保存boundsを読む
- 保存boundsの中心がslides以外のディスプレイ上にある → `WindowPlacement::Restored`（フラグ無し、Chrome復元）
- 保存が無い/不可視位置 → `WindowPlacement::Windowed`（`--window-position`+`--window-size`でプライマリ中央1200x800にシード。この位置がChromeに保存され次回以降の復元の種になる）

実機E2E: 初回シード表示→移動→close→再起動で移動先(300,150)に正確に復元、不可視保存bounds（外部1534,47）からのreseedフォールバック、部分画面外boundsのChromeクランプ、をそれぞれ確認。

## 追記3: presentation終了時にChromeインスタンスを正規終了する

macOSのChromeはウィンドウ全closeでもプロセスが残るため、present終了後もDockに窓なしChromeが2つ居座り続けていた（「閉じても閉じても消えない」というUX問題）。present終了時（サーバ終了後、`--no-open`でない場合のみ）に`quit_profile_instances`で正規終了させる。起動時のstale quitは異常終了したセッションの保険として残す。実機E2Eでclose後にプロセス0を確認。

## 追記4: 1画面でも動くように（著者バグ報告起点）

仮想ディスプレイOFFの1画面構成では従来presenterが開かず、slidesがフルスクリーンで開くだけだった（「windowedなのに全画面になる」の正体）。デバッグモードの趣旨に合わせ、`--presenter-windowed`かつ1画面のときはslidesを960x600の窓（左上シード）、presenterを従来どおり復元/シードの窓で開くようにした。何もフルスクリーンにならないため、保存placementの可視判定は「slidesディスプレイ除外なし」に緩和。通常モード（フラグ無し）の1画面=slidesのみフルスクリーンは不変。
