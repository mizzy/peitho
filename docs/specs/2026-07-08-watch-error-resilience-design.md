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

### 変更点

1. `WatchRuntime` に `watched_dirs: Vec<PathBuf>`(実際に登録した集合)を
   追加。初期登録時に確定させる。
2. 新関数 `reconcile_watched_dirs(watcher, watched: &mut Vec<PathBuf>,
   desired: &[PathBuf], stderr)`:
   - `watched - desired` を unwatch、`desired - watched` を watch。
   - unwatch/watch の失敗(`watch_not_found` 等)は stderr ノートにして
     継続(致命傷にしない)。成功・失敗にかかわらず `watched` を
     desired に収束させる(PollWatcher は存在しないパスの watch を
     イベント経由でしか失敗報告しないため、コールサイトの Result に
     依存しない)。
   - 戻り値は「集合が変化したか」(bool)。
3. `handle_watch_event_result` のエラー枝を非致命化:
   - `watch error: {err}` を stderr にノート(help 文言は「消えたパスは
     監視から外し、復活または deck 変更時に再監視する」旨に変更。
     「restart the command」は削除)。
   - `targets.watch_dirs()`(現FS状態から再導出した desired)に対して
     reconcile。消えたルートは desired から自然に消えるので
     unwatch され、ポーリングスパムが止まる。消えたルートの親が
     desired に入るので、**復活はその親の通常イベントとして観測できる**。
   - reconcile で集合が変化し、かつ `err.paths` のいずれかが
     `is_relevant_change` に該当する場合のみ、リビルドを1回トリガー
     (frontmatter が明示参照するアセット欠損ならリビルドは失敗し、
     preview は既存挙動どおり stderr に `build failed:` を出して
     直前の成功世代を配信し続ける。実行中のエラーページ差し替えは
     初回ビルド失敗時のみの既存仕様で、本修正では変えない)。
     集合が変化しない持続的エラー(権限エラー等)ではリビルドしない
     (200ms ごとのリビルドループを防ぐゲート)。
   - 同一文言のエラーノートが連続する場合は2回目以降を抑制する
     (直前ノートの文字列を保持して比較)。権限エラー等の持続的
     スパムをログ洪水にしないため。
4. 通常イベント経路(`handle_watch_paths_with_rebuild`)でも、relevant な
   変更でリビルドした後に reconcile を実行する。これで
   「消えたディレクトリの復活」「fonts ツリー内の新サブディレクトリ」に
   監視集合が追従する(§3-3 の修正)。
5. `refresh_watch_targets_after_deck_change` は差分計算に
   `targets.watch_dirs()` の再計算値(=ドリフトした推測)ではなく
   所有された `watched_dirs` を使う(§3-2 の潜在バグ修正)。
   既存の `update_watched_dirs` は reconcile に置き換えて削除。
6. `spawn_preview_watch` の `process::exit(1)` 経路は残すが、そこに
   到達するのは stdout/stderr 書き込み失敗などwatch継続が無意味な
   場合のみになる(watcher由来のエラーは 3 で消費され、Err として
   返らない)。`build --watch` の `watch_paths_loop` も同じ関数を
   通るため同時に直る。

### 到達しうるエラー経路の整理(サイレントドロップ禁止の確認)

| 事象 | 挙動 |
| --- | --- |
| 監視中ディレクトリが消えた | ノート1回 + unwatch + 親watch継続。relevantならリビルド(明示frontmatterならビルド失敗ノート、preview は直前の成功世代を配信し続ける) |
| 消えたディレクトリが復活 | 親watchの通常イベント → リビルド + reconcile でツリー再監視 |
| fonts ツリー内に新サブディレクトリ | 通常イベント → リビルド + reconcile で追加監視 |
| 権限エラー等の持続的watchエラー | ノート(連続同一文言は抑制)+ reconcile no-op + リビルドなし。ループ継続 |
| deck 変更でアセットパス変更 | 従来どおり再解決 + reconcile(所有集合ベースなので watch_not_found で死なない) |
| 起動時に watcher 自体が作れない | 従来どおり起動失敗(致命傷のまま。サーバー起動前なので正しい) |
| stdout/stderr 書き込み失敗 | 従来どおり致命傷(端末喪失。継続する意味がない) |

### テスト方針

既存の `WatchController` フェイクを使った単体テストに加える:

- watchエラーイベント(消えたルートを paths に含む)→ ノート出力、
  unwatch 呼び出し、関数は Ok を返す(ループ継続)。
- reconcile: watched/desired の差分どおりに watch/unwatch が呼ばれ、
  失敗しても Ok で収束する。
- エラー後に集合変化なし(権限エラー相当)→ リビルドが呼ばれない。
- エラー後に集合変化あり+relevant → リビルドが1回呼ばれる。
- 同一文言エラーノートの連続 → 2回目は出力されない。
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
