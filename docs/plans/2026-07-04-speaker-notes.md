# 発表者ノート実装計画 (2026-07-04)

## 決定事項

- **記法**: HTMLコメント `<!-- ... -->` （Marp / k1LoW/deck方式）
  - 既存の「JSONオブジェクトを含むHTMLコメント = ページ設定 (`key`/`layout`)」との判別は、**JSONとしてパースできるかどうか**で行う（既存 `parse_page_comment` の分岐を再利用）
  - JSON設定でも空HTMLコメントでもない、非空のHTMLコメントを **speaker note** として拾う
- **配置制約**: スライド本文のどこに書いてもよい。1スライド内に複数コメントがあれば `\n\n` で連結（k1LoW/deckと同じ）
- **中身の解釈**: 保存時はプレーンテキスト（前後の空白のみ `trim`）。presenter側での表示もv1はプレーンテキスト（`textContent`）を維持
  - Markdown書式解釈は後段で拡張可能な軸として残す（`notes.json`はversion付きなので前方互換）
- **配布HTMLへの埋込**: なし。既存設計どおり `dist/index.html` にはノートを含めず、`peitho present` のみが `notes.json` を読む

## 三本柱・不変条件との整合

- **柱1（内容と設計の分離）**: ノートはMarkdown側にHTMLコメントとして書く → 内容側に閉じる。レイアウトHTMLには一切影響しない ✅
- **柱3（型検査・沈黙禁止）**:
  - 現在 `parser.rs:1000` の `ignores_plain_html_comments` は「非JSONコメントを黙って捨てる」テスト。これを **「非JSONコメントは speaker note として収集される」** テストに置き換える
  - 空HTMLコメント `<!-- -->` は現状も無視されている。明示的に「空コメントはノート扱いしない（無視）」を単体テストで固定
  - JSONに見えるが `key`/`layout` 以外のフィールドを含むコメントは既に `parse_page_comment` がエラーにしている → 変更不要
- **typestate**: `ParsedSlide` に `notes: Option<String>` を追加し `Mapped→Checked→Rendered` を通して運搬。`Notes` コレクションの組み立ては `Checked` から `Rendered` への遷移で `SlideKey` が確定してから

## パーサ側の変更（`crates/peitho-core/src/parser.rs`）

現状 `Event::Html | Event::InlineHtml` の分岐（`parser.rs:465-497`）:

1. `parse_page_comment` が `Some(settings)` を返せば設定として処理
2. それ以外の非空HTMLで、`is_html_comment` でなければ `unsupported_construct` エラー
3. HTMLコメントかつ設定でない場合は **何もしない**（← ここが黙って捨てているポイント）

変更:

- 3のケースで、コメント本文（`<!--` と `-->` の間）を抽出し `trim` して、空でなければ `ParsedSlide.note_fragments: Vec<String>` に push
- `parse_slide` の末尾で `note_fragments.join("\n\n")` を `notes: Option<String>` にまとめる（空なら `None`）
- 位置制約は付けない（本文の前・中・後どこでもOK）。設定コメントの「先頭のみ」制約は現状維持
- テスト:
  - `collects_speaker_note_from_html_comment` （1個のコメントが `notes` に入る）
  - `joins_multiple_html_comments_with_blank_line` （複数コメントは `\n\n` 連結）
  - `note_with_page_settings_comment_coexist` （設定コメント＋別のノートコメントが両立）
  - `empty_html_comment_is_ignored` （`<!-- -->` は無視）
  - `note_can_appear_after_content` （本文後のコメントも拾う）
  - 既存 `ignores_plain_html_comments` は上記に置き換え

## 型・ドメイン層の変更

- `ParsedSlide`（parser.rs内の型）に `notes: Option<String>` を追加
- `Mapped<Slide>` / `Checked<Slide>` / `Rendered<Slide>` の内部でも `notes` を伝搬（既存フィールド追加パターンに従う）
- `SlideKey` は Mapped 以降で確定するので、`Notes` コレクション（`BTreeMap<SlideKey, String>`）の構築は Rendered 段で行う

## Notes構築とmanifest

- `crates/peitho-core/src/notes.rs`: 既存 `Notes::new(BTreeMap)` をそのまま使う。組み立てヘルパを追加:
  ```rust
  impl Notes {
      pub fn from_slides(slides: &[Rendered<Slide>]) -> Self { ... }
  }
  ```
- `manifest.rs`: `SlideEntry::new` の `has_notes` に `slide.notes.is_some()` を渡す（現在は `false` 固定と思われる箇所を修正）
- CLI（`crates/peitho/src/main.rs:701`）: 現在 `Notes::empty()` を書き出している → `Notes::from_slides(...)` に差し替え

## Presenter側

- 現状 `presenter.ts:150` で `notesRoot.textContent = options.notes.notes[detail.key] ?? "No notes for this slide."`
- v1はこのまま **プレーンテキスト表示**。Markdown解釈への拡張は将来課題（`notes.json` version bumpで対応可能）
- 型 (`bindings/Notes.ts`) は変更なし

## E2E確認

- `examples/` に speaker note入りのサンプルを1つ追加（既存デックに追記でも可）
- `peitho build` → `dist/notes.json` の中身確認
- `peitho present` → presenter画面で Cmd+左右で切り替えつつノート表示を目視確認
- 複数ディスプレイ実機での挙動は CLAUDE.md の [BetterDisplay仮想ディスプレイ手順](file:///Users/mizzy/.claude/projects/-Users-mizzy-src-github-com-mizzy-peitho/memory/betterdisplay-virtual-display-e2e.md) を使う

## ゲート

- `cargo test --workspace` を3回
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` （今回 `Notes` の型は変えないので差分ゼロのはず）
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js` （presenter配線は現状のまま textContent なので shell側変更なし → 差分ゼロのはず）

## Undecidedを残すもの

- **Markdown書式解釈**: v1はプレーンテキスト固定。将来 `NotesFormat: "plain" | "markdown"` のような軸を frontmatter に追加する余地を残す
- **fenced div `::: notes` 記法**: §18 の fenced div slot notation と同時に検討。今回は着手しない

## PR

- ブランチ: `feat/speaker-notes`（既に worktree作成済み: `../peitho-speaker-notes`）
- draft PRで作成
- タイトル案: `feat: extract speaker notes from HTML comments`
