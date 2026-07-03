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
