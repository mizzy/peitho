# peitho

Markdownをsource of truthとする、HTMLネイティブなプレゼンテーションツール。

Peitho（ペイトー）は、力ではなく言葉で人の心を動かす力を司るギリシャ神話の女神。プレゼンテーションの本質に対応する。

## 特徴

- **コンテンツとデザインの分離** — コンテンツはMarkdown、デザインはレイアウトHTMLとCSS。両者を混ぜない
- **git管理可能なレイアウト** — デザイン成果物はただのHTML/CSS。diffが取れ、レビューできる
- **型検査されるスロット契約** — レイアウト自身がスキーマ。スロットの過不足・型不整合・参照切れ・余ったコンテンツは、黙って捨てられる代わりに行番号とヒント付きのビルドエラーになる

```
error: slide 2 ('code-slide'), line 7: slot 'code' got 2 item(s), but layout 'title-body-code' allows 0..1
  = help: use a layout with more code capacity or remove one code block
```

- **安定キー起点のper-slide調整** — `<!-- {"key":"arch-1"} -->` で固定したキーをCSSから狙う。タイトルを直してもCSSは剥がれず、存在しないキーを狙えばビルドが止まる

```css
[data-slide-key="arch-1"] .slot-code {
  grid-column: 2 / 3;
  width: 60%;
}
```

- **Keynote風の発表体験** — `peitho present` で、外部ディスプレイにスライドをフルスクリーン、手元に発表者ビュー（現在/次スライド・ノート・タイマー）を自動配置。Escで全終了

## 使い方

```sh
# 配布物の生成（dist/ に slides/断片 + manifest.json + index.html + peitho.css）
peitho build deck.md

# 保存のたびに再ビルド
peitho build deck.md --watch

# 発表（揮発キャッシュ生成 + ローカルサーバ + ブラウザ起動。2画面なら自動配置）
peitho present deck.md

# デバッグ用: 発表者画面をフルスクリーンにせず通常ウィンドウで開く（位置・サイズは前回の状態をChromeが復元）
peitho present deck.md --presenter-windowed

# 公開（検査してから既存のデプロイコマンドに委譲。デプロイは再発明しない）
peitho publish -- aws s3 sync dist/ s3://your-bucket/
```

レイアウト・テーマ・発表シェルはバイナリに内蔵されたデフォルトが使われるため、デッキファイルだけあればどのディレクトリでも動く。差し替えるときだけ`--layout`/`--base-css`/`--overrides-css`/`--shell`を渡す。

発表シェル（TS）を開発するときは`cd packages/peitho-present && npm ci && npm run build`で`dist/shell.js`を再生成する（コミット対象。CIがdriftを検査する）。

## デッキの書き方

規約マッピングにより、素のMarkdownがそのままスライドになる。スライド区切りは `---`、最も浅い見出しがタイトル、コードブロックはcodeスロット、残りはbodyへ。

```markdown
<!-- {"key":"intro"} -->
# タイトル

本文の段落。

- リストも
- 使える

---

# 次のスライド

```rust
enum Phase { Parsed, Mapped, Checked, Rendered }
```
```

## 複数レイアウト

`--layouts`にディレクトリを渡すと中の`*.html`が全てレイアウトになる（名前はファイルstem、順序はファイル名順で決定論的）。スライドごとのレイアウトは次の順で決まる（[k1LoW/deck](https://github.com/k1LoW/deck)のページ設定を参考にしたハイブリッド方式）:

1. **明示指定** — ページ設定コメント`<!-- {"layout":"cover"} -->`があればそのレイアウト（未知の名前は候補一覧つきビルドエラー）
2. **1枚なら無条件** — レイアウトが1枚だけなら常にそれ（契約違反は従来どおりの行番号付きエラー）
3. **型駆動ディスパッチ** — 複数枚なら、スライドの内容の形（タイトルだけ・本文あり・コードあり等）にスロット契約が一致するレイアウトへ自動で振り分ける。ちょうど1枚に一致することが条件で、**複数一致（曖昧）も0枚一致も黙って解決せずビルドエラー**にし、明示指定を促す

## サンプル

`examples/`に、内容・レイアウト構造・テーマが全て異なるサンプルを置いている。各ディレクトリは`deck.md`+`layouts/`+`css/`で自己完結する。

| サンプル | 内容 | デザイン | 契約の見どころ |
|---|---|---|---|
| `examples/deck.md` | 最小デモ | デフォルトテーマ | デフォルトフラグでそのまま動く |
| `examples/lightning-talk/` | 日本語LT | ダーク+大型タイポのポスター風 | codeスロットが無い=コードを書くとビルドエラー |
| `examples/code-walkthrough/` | Rustのtypestate解説 | ターミナル風2カラム | `code`が`arity="1"`=毎スライドコード必須。キー付きoverrideの実用例 |
| `examples/keynote/` | 日本語キーノート | クリーム地+セリフ体+中央寄せ | 2レイアウト構成。タイトルだけのスライドは`cover`へ、本文ありは`statement`へ型駆動ディスパッチ |

```sh
peitho present examples/keynote/deck.md \
  --layouts examples/keynote/layouts \
  --css examples/keynote/css
```

動作確認にはMakefileのターゲットが便利（`make help`で一覧。`make keynote`、`make lightning-talk`等。シェルバンドルのビルド込みで`cargo run`する）。

## アーキテクチャ

```
Markdown ─→ peitho build（解析・マッピング・4段検査。決定論的・純粋関数）
              ├─ emit distribute → dist/（配布物のみ。シェル・ノート非混入）
              └─ emit present    → .peitho/present-cache/（発表シェル・揮発）
```

- ビルドコアはRust（typestate: `Parsed→Mapped→Checked→Rendered`。未検査スライドはレンダラに渡せない）
- 発表シェルはTypeScript。契約（manifest等のドメイン型）はRustが唯一のsourceで、`bindings/` にTS型を生成してdriftをCIで検査
- 詳細な設計は `docs/PEITHO_KICKOFF.md` を参照

## License

MIT
