# Watch error resilience — preview/buildのwatchループを監視対象のトラブルで死なせない

- 日付: 2026-07-08
- 状態: 設計確定(実装前)
- 関連: `crates/peitho/src/main.rs` の watch 系(`watch_paths_loop` / `handle_watch_event_result` / `refresh_watch_targets_after_deck_change` / `spawn_preview_watch`)

## 1. 問題

`peitho preview` 実行中に、frontmatter `fonts:` が参照するディレクトリ
(`../../fonts/noto-sans`)がリネーム・削除されると、次の出力とともに
プロセスごと落ち、サーバーの再起動が必要になる。

```
rebuilt 18 slide(s) into .peitho/preview-cache
preview watch error: watch error: IO error for operation on ./../../fonts/noto-sans: No such file or directory (os error 2) about ["./../../fonts/noto-sans"]
help: restart the command after checking file watcher permissions
exit 1
```

ビルドエラーは「stderrに出す+エラーページを出す+サーバーは生き続ける」
設計なのに、watcherのランタイムエラーだけが即死する非一貫性がある。

## 2. 計測で確認した事実(notify 8.2.0 / notify-debouncer-mini 0.7.0)

- `PollWatcher::watch()` は存在しないパスに対して **Err を返さない**。
  `WatchData::new` が `fs::metadata` 失敗時にエラー**イベント**を発行して
  登録をスキップする(`poll.rs` の `watch_inner` / `WatchData::new`)。
  つまり登録失敗はコールサイトの `Result` ではなく、イベントチャネルの
  `DebounceEventResult::Err` として非同期に届く。
- 登録済みルートが消えた場合、ポーリングごとの `rescan` で WalkDir が
  ルート欠損エラーを出し、**ポーリング間隔(200ms)ごとにエラーイベントが
  発行され続ける**。PollWatcher は自分では watches からルートを削除しない。
  → 単純な「ログして継続」ではエラーノートが無限にスパムする。
- ルート配下の既知エントリは `rescan` の disappeared 検出で **remove の
  通常イベント(Ok)としても届く**。つまりディレクトリ消失の多くのケースで、
  リビルドのトリガーは通常イベント経路からも得られる。
- `DebounceEventResult = Result<Vec<DebouncedEvent>, notify::Error>`、
  `notify::Error` は `paths: Vec<PathBuf>` を持つ(エラー対象パスが取れる)。

## 3. 壊れている不変条件(根本原因)

1. **watchループの生存性**: `handle_watch_event_result`(main.rs:542)が
   `DebounceEventResult::Err` を `?` で致命傷として伝播し、preview 側は
   `spawn_preview_watch`(main.rs:1532)が `std::process::exit(1)` で
   プロセスごと殺す。`build --watch` も同じ `watch_paths_loop` を通るので
   同様に終了する。
2. **監視集合の所有権**: 「いま実際にwatch登録されているディレクトリ集合」を
   誰も保持しておらず、`WatchTargets::watch_dirs()` が**その時点の
   ファイルシステム状態から毎回再計算**される。集合がドリフトすると
   (例: 監視中のディレクトリが消えると `is_dir()` が false になり親を返す)、
   `refresh_watch_targets_after_deck_change` → `update_watched_dirs` の
   差分計算が実際の登録状態と食い違い、`unwatch` の `watch_not_found` が
   致命傷として伝播する**潜在バグ**が deck 変更経路にもある。
3. **監視集合の収束契機**: 監視集合の更新が deck ファイル変更時にしか
   行われない。fonts ツリー内に新しいサブディレクトリができた場合や、
   消えたディレクトリが復活した場合に監視集合が古いまま追従しない
   (非再帰watchなので配下の変更を取りこぼす)。これも同じ根本原因
   (集合が所有されておらず、収束処理が1経路にしかない)の別症状。

## 4. アプローチ比較

### A. エラーをログして継続するだけ(却下)

- Pros: 最小差分。
- Cons: PollWatcher が消えたルートに 200ms ごとにエラーを出し続けるため
  ノートがスパムする。監視集合のドリフト(§3-2, §3-3)は直らない。
  症状への絆創膏。

### B. watchエラー時に debouncer ごと作り直して全再登録(次点)

- Pros: 状態追跡不要でセマンティクスが単純。`watch_dirs()` が
  ファイルシステム状態を反映するので、消えたルートは自然に親watchへ
  切り替わる。
- Cons: `WatchRuntime` の rx/debouncer をループ内で差し替える再構築が
  必要になり、ループ構造の変更が大きい。§3-2 の deck 変更経路の潜在バグは
  「差分更新をやめて全再構築に統一」しない限り残る。全再構築に統一すると
  イベントチャネルの張り替えが頻発する。

### C. 実watch集合を所有状態にして、単一の reconcile に収束させる(採用)

- Pros: 根本原因(§3の3点すべて)を1つの上流シームで直す。
  「実際に登録済みの集合」と「望ましい集合(targets+現FS状態から導出)」の
  差分適用が唯一の変更経路になり、watchエラー時・deck変更時・通常変更時の
  3契機がすべて同じ関数に集まる。将来の呼び出し元がフィルタを
  覚える必要がない。
- Cons: `WatchRuntime` に状態が1つ増える。差分適用のテストが必要。
- Complexity: 中。

## 5. 採用設計(C)

### 復元する不変条件

- **watchループは、起動に成功したら watcher/ファイルシステム由来の
  ランタイムエラーで終了しない。** エラーは診断ノートであり、
  ビルドエラーと同格の扱い。
- **実watch集合は `WatchRuntime` が所有する状態であり、すべての登録変更は
  単一の reconcile 関数を通る。** ファイルシステムからの再計算で
  「いま何をwatchしているか」を推測しない。

### 変更点(2026-07-08 レビュー改訂済み。改訂理由は §5.1)

1. watch 関連状態(`input` / `targets` / 実watch集合 `watched_dirs` /
   ノート抑制状態)は `WatchState` に束ね、`WatchRuntime` は
   `state + debouncer + rx` を持つ(引数肥大と
   `clippy::too_many_arguments` の回避、所有の明示)。
2. 新関数 `reconcile_watched_dirs(watcher, watched: &mut Vec<PathBuf>,
   desired: &[PathBuf], stderr, emitted_notes: &mut HashSet<String>)`
   → `ReconcileResult { changed, had_failures }`:
   - `watched - desired` を unwatch、`desired - watched` を watch。
     差分計算は各パスを一度だけ正規化(canonicalize 成功時はその値、
     失敗時は元のパス)したキーの HashSet で行う(O(n×m) の
     `same_watch_path` 総当たりと二重 canonicalize を避ける)。
   - unwatch/watch の失敗は stderr ノートにして継続(致命傷にしない)。
     失敗ノートは呼び出し元が渡す出力済みノート集合で抑制され、
     `had_failures` が変更点4のクリア条件の判定材料になる。
   - **`watched` には登録に成功したディレクトリだけを記録する。**
     `WatchController::watch_dir` は対象が存在しない場合に同期的に
     Err を返す(PollWatcher の `watch()` は存在しないパスを黙って
     スキップして Ok を返すため、存在チェックをコントローラ側で行い
     「登録済みのつもりで未登録」という嘘の状態を作らない)。
     失敗した desired は watched に入らないので、次の reconcile 契機で
     自動的に再試行される。
3. desired 集合の導出(`WatchTargets::watch_dirs()`)は、ルートパスが
   存在しない場合に**存在する最も近い祖先ディレクトリまで遡って**
   watch 対象にする(従来の「直上の親」固定では、親ごと消えたときに
   存在しないディレクトリが desired に入り、2 の存在チェックと併せて
   誰も監視しない=復活を観測できない穴になる)。復活は祖先 watch の
   通常イベントとして観測され、reconcile が段階的にツリーを
   再監視する。
4. `handle_watch_event_result` のエラー枝を非致命化:
   - reconcile を**先に**実行し、その後に `watch error: {err}` ノートを
     stderr に出す。help は「どのイベント順序でも常に真」になる
     メカニズム説明の単一文言(missing watch targets are dropped and
     re-watched automatically when they reappear or the deck
     frontmatter changes; if this error persists, check file watcher
     permissions)。配送単位の reconcile 結果(stopped した集合)から
     help を分岐させると、remove の Ok イベントが先に届いて Ok 枝が
     先に unwatch した場合や同一インシデントの Err が複数届いた場合に
     実態と矛盾するノートになる(セルフレビュー R1 で検出)。
     消えたパス名は err 本文自体に含まれるため列挙は不要。
   - リビルドは「reconcile で集合が変化した」場合のみ1回トリガー。
     `err.paths` の relevance 判定は使わない(§5.1 の改訂 [2]:
     ルートの unwatch がそのルート自身のエラー配送より先行すると
     relevance が永遠に真にならないレースがあり、またリビルドループの
     防止は集合変化ゲートだけで担保できる)。明示 frontmatter の
     アセット欠損ならリビルドは失敗し、preview は既存挙動どおり
     stderr に `build failed:` を出して直前の成功世代を配信し続ける
     (実行中のエラーページ差し替えは初回ビルド失敗時のみの既存仕様)。
   - ノート抑制は `WatchState` が保持する「出力済みノート集合」で行う
     (直前1件だけの比較では、複数ルートの持続エラーが交互に届くと
     抑制が効かず洪水が再発する — §5.1 の改訂 [1])。エラーノートの
     抑制キーは err 本文、reconcile の watch/unwatch 失敗ノートも
     同じ集合を通す。集合のクリアは「reconcile で watch/unwatch の
     失敗が1件もなかった Ok バッチ」のときのみ(「印字しなかったら
     クリア」だと、持続失敗時に印字→抑制→クリア→印字…の交互で
     半レートの洪水が再発する。無条件クリアも同様)。
5. 通常イベント経路(`handle_watch_paths_with_rebuild`)は、relevance に
   かかわらず**毎イベントバッチ後に reconcile** する(集合が一致して
   いれば watcher 呼び出しゼロの no-op)。これで「消えたディレクトリの
   復活」「fonts ツリー内の新サブディレクトリ」「祖先 watch 経由の
   段階的復帰」に監視集合が追従する(§3-3 の修正)。リビルドは
   「relevant な変更があった」**または「reconcile で集合が変化した」**
   場合に1回発火する(Err 枝の changed ゲートと対称)。後者がないと、
   親ごと消えたアセットの復活イベント(祖先ディレクトリの作成)は
   relevant に該当しないため、ツリーは再監視されるのに出力が
   古いまま次の relevant 変更まで放置される(E2E で確認した実挙動)。
   集合が変化するのは監視トポロジが変わったときだけなので、
   リビルドループにはならない。
6. `refresh_watch_targets_after_deck_change` は targets の再解決と
   ノート出力だけを行い、reconcile は 5 のバッチ末尾の1回に集約する
   (deck 変更で reconcile が二重実行されるのを避ける)。
   既存の `update_watched_dirs` は削除。
7. `spawn_preview_watch` の `process::exit(1)` 経路は残すが、そこに
   到達するのは stdout/stderr 書き込み失敗などwatch継続が無意味な
   場合のみになる(watcher由来のエラーは 4 で消費され、Err として
   返らない)。`build --watch` の `watch_paths_loop` も同じ関数を
   通るため同時に直る。

### 5.2 隠しディレクトリの扱い(2026-07-08 セルフレビュー R2/R3)

fonts ツリー内のドット始まり**ディレクトリ**は watch 対象からも
relevance 判定からも除外する(既存のドット始まり**ファイル**の
除外決定 — `watch_ignores_dotfiles_in_fonts_dir` で固定済み — を
ディレクトリと全パス階層に一貫させる)。理由: 毎バッチ reconcile の
導入により、fonts 内に `.git` 等が作成されると desired の増加が
リビルドを発火し、さらにその内部の非ドット名ファイル(`refs/...` 等)の
変化が葉名フィルタをすり抜けてリビルド源になる。ビルド時のコピーは
従来どおり verbatim(隠し含む)なので、「隠しはコピーされるが
リビルド契機にならない」という既存の非対称を維持・拡張する形になる。
隠しディレクトリ内のフォント編集で自動リビルドされない点は
この決定の意図されたトレードオフ(可視の変更でリビルドされた際には
最新の隠し内容がコピーされる)。

### 5.1 レビュー改訂の記録(2026-07-08)

初版実装に対する検証済みレビューで以下が確定し、上記 2〜6 に
反映した:

- **[0] 登録失敗の黙殺**: 初版は「成功・失敗にかかわらず watched を
  desired に収束」としていたが、PollWatcher の `watch()` は存在しない
  パスを黙ってスキップするため、親ごと消えたルートが「登録済み」と
  記録され、以後 desired と一致し続けて再試行されず、復活しても
  誰も監視していない(サイレント劣化)。→ 存在チェックを
  コントローラで同期化し、watched には成功分のみ記録、desired は
  存在する最近祖先まで遡る。
- **[1] 抑制の直前1件比較**: 異なる持続エラーが交互に届くと抑制が
  無効化しログ洪水が再発する。→ 出力済みノート集合で抑制。
- **[2] relevance ゲートのレース**: reconcile が先にルートを unwatch
  すると、そのルートのエラーが配送されないままになり得て、
  リビルドが永久に抑止される。→ ゲートを「集合変化のみ」に単純化。
- **[3] 嘘ノート**: reconcile 前に固定文言「removed missing paths」を
  出すと、権限エラー等の no-op 時に実態と矛盾する。→ reconcile 後に
  結果に応じた文言を出す。
- ほか、O(n×m)×canonicalize の差分計算、deck 変更時の二重 reconcile、
  不要 clone を解消し、本実装で追加した「実行時に何も検証しない
  関数ポインタテスト」(`watch_loop_function_accepts_runtime_with_watch_state`)
  を削除(main 由来の既存 `watch_build_function_is_available_for_cli_dispatch`
  は本 PR のスコープ外として残置)。

### 到達しうるエラー経路の整理(サイレントドロップ禁止の確認)

| 事象 | 挙動 |
| --- | --- |
| 監視中ディレクトリが消えた | ノート1回 + unwatch + 存在する最近祖先を watch。集合が変化するのでリビルド1回(明示frontmatterならビルド失敗ノート、preview は直前の成功世代を配信し続ける) |
| 親ごと消えた(復活時にまず親だけできる等) | 祖先 watch の通常イベント → 毎バッチ reconcile が段階的にツリーを再監視し、集合変化でリビルド(relevant イベントを待たずに出力が復帰する) |
| 消えたディレクトリが復活 | 親/祖先watchの通常イベント → リビルド + reconcile でツリー再監視 |
| fonts ツリー内に新サブディレクトリ | 通常イベント → リビルド + reconcile で追加監視 |
| 権限エラー等の持続的watchエラー | ノート(出力済み集合で抑制、Okイベントでクリア)+ reconcile no-op + リビルドなし。ループ継続 |
| watch 登録が失敗した(対象消失等) | ノート + watched に記録しない → 次の reconcile 契機で自動再試行 |
| deck 変更でアセットパス変更 | 再解決 + バッチ末尾の reconcile 1回(所有集合ベースなので watch_not_found で死なない) |
| 起動時に watcher 自体が作れない・初期登録に失敗 | 従来どおり起動失敗(致命傷のまま。サーバー起動前なので正しい。初期登録失敗は `watch_dirs()` が存在確認済みの dir のみ返すため TOCTOU レースでしか踏めない) |
| stdout/stderr 書き込み失敗 | 従来どおり致命傷(端末喪失。継続する意味がない) |

### テスト方針

既存の `WatchController` フェイクを使った単体テストに加える:

- watchエラーイベント(消えたルートを paths に含む)→ ノート出力、
  unwatch 呼び出し、関数は Ok を返す(ループ継続)。
- reconcile: watched/desired の差分どおりに watch/unwatch が呼ばれ、
  unwatch 失敗はノートで継続する。
- watch 登録失敗(対象消失)→ watched に記録されず、次の reconcile で
  再試行される。
- 存在しないルートの desired 導出 → 存在する最近祖先が watch される。
- エラー後に集合変化なし(権限エラー相当)→ リビルドが呼ばれず、
  ノートには「check file watcher permissions」系の help が付く。
- エラー後に集合変化あり → リビルドが1回呼ばれる。
- 異なる文言の持続エラーが交互に届いても、各文言のノートは1回だけ
  出力される(Okイベント後は再度出力できる)。
- deck 変更時の refresh がドリフトした実集合を使って死なないこと
  (旧実装では watch_not_found が致命傷になるケース)。
- E2E(手動・実ブラウザ): preview 起動 → fonts ディレクトリを
  リネーム → サーバー生存・watchエラーノート出力・`build failed:` ノート
  (明示frontmatter時。配信は直前の成功世代のまま)→ リネームを戻す →
  次のビルドが成功し preview が復帰。

### 検証ポイント(実装時に計測で確認)

- ディレクトリ消失時に remove の通常イベントと Err がどの順序・粒度で
  届くか(debouncer-mini がエラーを別メッセージとして flush する挙動)。
  設計はどちらの順序でも成立するが、テストの期待値を書く際に確認する。

## 6. 採用しなかった選択肢の記録

- 「preview だけ直して build --watch は据え置き」: 同じ根本原因の
  別症状なので不可(CLAUDE.md のルートコーズ原則)。
- 「エラー時に process::exit をやめて watch スレッドだけ静かに終える」:
  preview が黙って更新されなくなるサイレント劣化であり、
  サイレントドロップ禁止の柱に反する。
