# サンプルギャラリー（内容・デザイン違いの3パターン追加）

## 目的

`examples/`にデッキ1枚しかなく、「コンテンツとデザインの分離」の説得力が出ない。内容・テンプレート構造・テーマが全て異なる自己完結サンプルを3つ追加し、テンプレートがスキーマであること（スロット契約のバリエーション）とキー付きoverrideの実用例を見せる。

## 構成

既存の`examples/deck.md`はデフォルトフラグ（`templates/`・`themes/`）で動く最小サンプルとして残す（テストのfixtureでもある）。新規サンプルは各ディレクトリで自己完結:

```
examples/
  deck.md              # 最小（デフォルトフラグで動く）
  lightning-talk/      # 日本語LT。テキストのみ、codeスロット無しテンプレート
  code-walkthrough/    # 英語コード解説。code arity=1の2カラム、キー付きoverride実用
  keynote/             # 日本語キーノート。中央寄せエディトリアル
    deck.md
    template.html
    base.css
    overrides.css
```

各サンプルの狙い:

| サンプル | 契約の見せ場 | デザイン |
|---|---|---|
| lightning-talk | codeスロットが無い=コードを書いたらビルドエラー | ダーク+大型タイポのポスター風 |
| code-walkthrough | `code accepts="code" arity="1"`=毎スライドコード必須 | ターミナル風。overrides.cssでpayoffスライドのコードを強調 |
| keynote | title+bodyのみの最小契約 | クリーム地+セリフ体+中央寄せ |

## 制約（実装済みの仕様に従う）

- テンプレートは`<section>`ちょうど1個。`peitho-slide`クラスとdata-slide-keyはレンダラが注入
- overrides.cssのセレクタが使えるクラスはスロットクラス（`.slot-*`）のみ。キーはデッキに実在するもののみ
- 全テーマ1280x720固定キャンバス、overflow hidden、システムフォントのみ（オフライン動作）

## 検証

各サンプルを`peitho build`し、実ブラウザで全スライドを目視確認（はみ出し・潰れをスクリーンショットで見る）。READMEにサンプル一覧とビルドコマンドを追記。
