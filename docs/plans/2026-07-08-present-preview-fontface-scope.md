# Issue 194: present/previewのwebフォントをdocumentスコープへ出す

## Problem

`peitho present`/`peitho preview`は`peitho.css`をfetchし、各スライドのShadow DOM内へ`<style>`として注入している。スライド用CSSの隔離としては正しいが、ブラウザはShadow DOM内の`@font-face`をdocumentフォントとして登録しないため、`fonts:`でコピーされたfontsourceのwebフォントがpresent/previewだけOSフォールバックになる。

サイトビューアとPDFは`<head>`の`<link rel="stylesheet" href="peitho.css">`でdocumentスコープにCSSを読むため、同じ`peitho.css`でもwebフォントが効く。差分はフォント宣言のスコープだけ。

## Scope

- 変更対象は`packages/peitho-present/src/`のTypeScriptだけ。Rust側は変更しない。
- `packages/peitho-present/dist/shell.js`と`packages/peitho-present/dist/preview.js`は埋め込み成果物なので、実装後に`npm run build`で再生成してコミットする。
- Shadow DOM側の`peitho.css`注入は現状維持する。documentへ出すのはフォント関連ルールだけで、`peitho.css`全体は出さない。

## Plan

1. Red:新規`packages/peitho-present/test/fontscope.test.ts`を追加し、`extractFontScopeCss`相当の抽出関数に失敗する単体テストを書く。
   - `extracts leading imports after charset comments and whitespace`
   - `does not promote imports after ordinary rules`
   - `extracts top level font face blocks from anywhere`
   - `skips comments and strings while scanning font face blocks`
   - `omits non font rules`
2. Green:新規`packages/peitho-present/src/fontscope.ts`を追加する。
   - 先頭prefixだけを走査し、空白、コメント、`@charset`文を読み飛ばしてから、有効な連続`@import`文だけを抽出する。
   - 通常ルールが出た後の`@import`はCSS仕様上も無効なので抽出しない。途中の無効な`@import`をdocumentスコープで有効化しない。
   - ファイル全体を小さな字句走査で読み、コメントと文字列リテラルをスキップしながらトップレベルの`@font-face{...}`ブロックだけを抽出する。
   - `@media`などのネスト内の`@font-face`は追わない。今回の制約としてトップレベルだけを扱う。
   - 抽出結果が空ならdocumentへstyleを追加しない。
3. Red:`packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts`にpresentシェルのdocumentフォントスコープテストを追加する。
   - `injects document scoped font css once for present shells`
   - `removes document scoped font css when the last present shell is destroyed`
   - 既存の`injects peitho css into each shadow root before fragment html`は残し、Shadow DOM注入が変わらないことも守る。
4. Green:`packages/peitho-present/src/shell.ts`で`peitho.css`取得後に`installDocumentFontScope(document, css)`相当を呼び、返されたcleanupを`destroy()`で実行する。
   - documentごとに`style[data-peitho-font-scope]`を1つだけ作る。
   - 複数スライド、複数`mountPresentShell`、presenterのcurrent/preview同時shellでも1つに保つ。
   - 複数同時マウントで先にdestroyされたshellが残りのshellを壊さないよう、`fontscope.ts`側でDocument単位の参照数を持ち、最後のcleanupで注入したstyleを除去する。
   - 既に外部から`style[data-peitho-font-scope]`がある場合は再注入せず、その既存styleはdestroyで削除しない。
5. Red:`packages/peitho-present/test/preview.test.ts`にpreviewシェルのdocumentフォントスコープテストを追加する。
   - `injects document scoped font css once for preview shells`
   - `removes document scoped font css when the last preview shell is destroyed`
   - 複数スライドと複数`mountPreviewShell`でもstyleが1つだけであることを見る。
6. Green:`packages/peitho-present/src/preview.ts`にも同じ`installDocumentFontScope(document, css)`を入れ、既存の`destroy()`でcleanupする。previewには既にmount後のクリーンアップ経路が`destroy()`としてあるため、そこに合わせる。
7. 仕上げとして、present/previewのfetch失敗時に不要な空styleが残らないことを確認する。`peitho.css`取得に失敗した場合は抽出前なので注入されない。CSS取得後の後続失敗でも、返されたshellの`destroy()`で掃除される。

## URL解決

document直下の`<style data-peitho-font-scope>`内に置いた`@import url("fonts/noto-sans-jp/index.css")`や`@font-face src:url("fonts/...")`はdocument base URLで解決される。presentのHTMLとpreviewのindex、`peitho.css`はいずれも同じ出力ルートに置かれるため、`peitho.css`内での相対URL解決と結果は同じになる。

## Non-goals

- `peitho.css`全体をdocumentへ`<link>`または`<style>`で入れない。シェルのホスト要素自身が`.peitho-slide`などを持つため、スライド用スタイルがシェルページへ二重適用される。
- Shadow DOM側の`@import`は消さない。ロードされてもfontsourceの`index.css`は`@font-face`だけなので、documentスコープ側の適用が本命で、Shadow DOM側は無害。
- 実ブラウザE2EはCodexのsandboxでは実行しない。webフォントの実適用確認はOpus側で`peitho present`/`peitho preview`を開いて実施する。

## Gates

- `cd packages/peitho-present && npm ci && npm run build && npm test && npm run typecheck`
- `git diff --check`
- `git diff --exit-code -- crates bindings`でRust側とbindingが変わっていないことを確認する。
- `packages/peitho-present/dist/shell.js`と`packages/peitho-present/dist/preview.js`が`npm run build`後の再生成物としてコミット対象に入っていることを確認する。
