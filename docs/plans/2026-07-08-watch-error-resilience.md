# Watch error resilience implementation plan

<!-- constrained-by ../specs/2026-07-08-watch-error-resilience-design.md -->

## Scope

`peitho preview` / `peitho build --watch` の watch ループを、監視中ディレクトリの削除・復活・配下ディレクトリ追加で終了させない。採用設計は [`../specs/2026-07-08-watch-error-resilience-design.md`](../specs/2026-07-08-watch-error-resilience-design.md) の §5(C) に固定する。

`clippy::too_many_arguments` を避けるため、実 watch 集合と watch 関連状態は `WatchState` に束ねる。

```rust
struct WatchState {
    input: PathBuf,
    targets: WatchTargets,
    watched_dirs: Vec<PathBuf>,
    last_watch_error_note: Option<String>,
}

struct WatchRuntime {
    state: WatchState,
    debouncer: Debouncer<PollWatcher>,
    rx: mpsc::Receiver<DebounceEventResult>,
}

fn handle_watch_paths_with_rebuild<F>(
    state: &mut WatchState,
    watcher: &mut dyn WatchController,
    changed_paths: &[PathBuf],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    rebuild: F,
) -> miette::Result<()>;

fn handle_watch_event_result<F>(
    result: DebounceEventResult,
    state: &mut WatchState,
    watcher: &mut dyn WatchController,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    rebuild: F,
) -> miette::Result<()>;
```

`reconcile_watched_dirs` の論理戻り値は「集合が変化したか」の `bool`。stderr 書き込み失敗は致命傷のままにするため、Rust の型は `miette::Result<bool>` にする。

## Task 1: debouncer-mini の消失イベント形状を計測する

**Goal**  
設計文書の検証ポイントどおり、登録済みディレクトリ削除時に `Ok(Vec<DebouncedEvent>)` と `Err(notify::Error { paths, .. })` がどう届くかを scratch テストで確認する。成功後に削除して成果物に残さない。

**Files**  
`crates/peitho/src/main.rs` (一時変更のみ。タスク終了時に元へ戻す)

**Test**

```rust
#[test]
fn watch_probe_directory_delete_event_shape() {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let fonts = root.join("fonts");
    let nested = fonts.join("noto");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("400.woff2"), b"font").unwrap();

    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let notify_config = notify::Config::default().with_poll_interval(Duration::from_millis(50));
    let debounce_config = DebounceConfig::default()
        .with_timeout(Duration::from_millis(50))
        .with_notify_config(notify_config);
    let mut debouncer = new_debouncer_opt::<_, PollWatcher>(debounce_config, tx).unwrap();
    debouncer
        .watcher()
        .watch(&fonts, RecursiveMode::NonRecursive)
        .unwrap();
    debouncer
        .watcher()
        .watch(&nested, RecursiveMode::NonRecursive)
        .unwrap();

    thread::sleep(Duration::from_millis(150));
    fs::rename(&fonts, root.join("fonts-gone")).unwrap();

    let mut saw_ok_for_fonts = false;
    let mut saw_err_for_fonts = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && !(saw_ok_for_fonts && saw_err_for_fonts) {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(events)) => {
                eprintln!("probe ok events: {events:?}");
                saw_ok_for_fonts |= events.iter().any(|event| event.path.starts_with(&fonts));
            }
            Ok(Err(err)) => {
                eprintln!("probe error: {err:?}");
                saw_err_for_fonts |= err.paths.iter().any(|path| path.starts_with(&fonts));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("watch channel closed: {err}"),
        }
    }

    assert!(saw_ok_for_fonts, "expected at least one remove-like Ok event");
    assert!(saw_err_for_fonts, "expected at least one notify::Error with fonts path");
}
```

**Implementation**

```rust
// #[cfg(test)] mod tests に一時追加する。
// probe 後、このテスト全体を削除する。
```

**Verification**

```bash
cargo test -p peitho watch_probe_directory_delete_event_shape -- --nocapture
git diff -- crates/peitho/src/main.rs
```

Red: assertion が設計文書と違う形で失敗した場合、`--nocapture` の出力を記録して設計前提を見直す。Green: `Ok` と `Err` の両方を観測する。Refactor: scratch テストを削除し、2つ目のコマンドで差分が残っていないことを確認する。

## Task 2: WatchState を導入して WatchRuntime に所有させる

**Goal**  
初期登録に成功したディレクトリ集合を `WatchState.watched_dirs` に保持し、`WatchRuntime` は `state + debouncer + rx` だけを持つ形にする。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn watch_state_owns_registered_dirs_after_registration() {
    let fixture = WatchFixture::new("# Intro\n");
    let mut watcher = RecordingWatchController::default();

    let watched_dirs = register_watch_target_dirs(&fixture.targets, &mut watcher).unwrap();
    let state = WatchState::new(
        fixture.options.input.clone(),
        fixture.targets.clone(),
        watched_dirs.clone(),
    );

    assert_eq!(state.input, fixture.options.input);
    assert_eq!(state.watched_dirs, watched_dirs);
    assert_eq!(watcher.watched, state.watched_dirs);
    assert!(state.last_watch_error_note.is_none());
}
```

**Implementation**

```rust
struct WatchState {
    input: PathBuf,
    targets: WatchTargets,
    watched_dirs: Vec<PathBuf>,
    last_watch_error_note: Option<String>,
}

impl WatchState {
    fn new(input: PathBuf, targets: WatchTargets, watched_dirs: Vec<PathBuf>) -> Self {
        Self {
            input,
            targets,
            watched_dirs,
            last_watch_error_note: None,
        }
    }
}

struct WatchRuntime {
    state: WatchState,
    debouncer: Debouncer<PollWatcher>,
    rx: mpsc::Receiver<DebounceEventResult>,
}

fn register_watch_target_dirs(
    targets: &WatchTargets,
    watcher: &mut dyn WatchController,
) -> miette::Result<Vec<PathBuf>> {
    let dirs = targets.watch_dirs();
    watch_all_dirs(watcher, &dirs)?;
    Ok(dirs)
}

let watched_dirs = {
    let mut watcher = NotifyWatchController::new(debouncer.watcher());
    register_watch_target_dirs(&targets, &mut watcher)?
};
let state = WatchState::new(input, targets, watched_dirs);
Ok(WatchRuntime { state, debouncer, rx })
```

**Verification**

```bash
cargo test -p peitho watch_state_owns_registered_dirs_after_registration
```

Red: `WatchState` が未定義で失敗する。Green: `WatchRuntime` が `state` を所有して通す。Refactor: `prepare_watch_loop` 内の初期化順を `targets -> watched_dirs -> state` に整え、同じコマンドを再実行する。

## Task 3: reconcile の差分適用を追加する

**Goal**  
`watched_dirs - desired_dirs` を `unwatch`、`desired_dirs - watched_dirs` を `watch` し、成功時に `watched_dirs` を `desired_dirs` へ収束させる。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn reconcile_watched_dirs_applies_diff_and_updates_owned_set() {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let old = root.join("old");
    let keep = root.join("keep");
    let new = root.join("new");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&keep).unwrap();
    fs::create_dir_all(&new).unwrap();
    let mut watched_dirs = vec![old.clone(), keep.clone()];
    let desired_dirs = vec![keep.clone(), new.clone()];
    let mut watcher = RecordingWatchController::default();
    let mut stderr = Vec::new();

    let changed =
        reconcile_watched_dirs(&mut watcher, &mut watched_dirs, &desired_dirs, &mut stderr)
            .unwrap();

    assert!(changed);
    assert_eq!(watcher.unwatched, vec![old]);
    assert_eq!(watcher.watched, vec![new]);
    assert_eq!(watched_dirs, desired_dirs);
    assert!(stderr.is_empty());
}
```

**Implementation**

```rust
fn contains_watch_path(paths: &[PathBuf], path: &Path) -> bool {
    paths.iter().any(|existing| same_watch_path(existing, path))
}

fn watch_sets_differ(left: &[PathBuf], right: &[PathBuf]) -> bool {
    left.len() != right.len() || left.iter().any(|path| !contains_watch_path(right, path))
}

fn reconcile_watched_dirs(
    watcher: &mut dyn WatchController,
    watched_dirs: &mut Vec<PathBuf>,
    desired_dirs: &[PathBuf],
    stderr: &mut dyn Write,
) -> miette::Result<bool> {
    let previous_dirs = watched_dirs.clone();
    let changed = watch_sets_differ(&previous_dirs, desired_dirs);

    for old in &previous_dirs {
        if !contains_watch_path(desired_dirs, old) {
            watcher.unwatch_dir(old)?;
        }
    }
    for new in desired_dirs {
        if !contains_watch_path(&previous_dirs, new) {
            watcher.watch_dir(new)?;
        }
    }

    *watched_dirs = desired_dirs.to_vec();
    Ok(changed)
}
```

**Verification**

```bash
cargo test -p peitho reconcile_watched_dirs_applies_diff_and_updates_owned_set
```

Red: `reconcile_watched_dirs` が未定義で失敗する。Green: 差分適用で通す。Refactor: `contains_watch_path` の呼び出し重複を整え、同じコマンドを再実行する。

## Task 4: reconcile の watch/unwatch 失敗を非致命ノートにする

**Goal**  
`watch_not_found` と `PathNotFound` の登録変更失敗を stderr ノートにして継続し、成功・失敗にかかわらず所有集合を `desired_dirs` に収束させる。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn reconcile_watched_dirs_notes_failures_and_converges() {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let stale = root.join("stale");
    let desired = root.join("desired");
    let mut watched_dirs = vec![stale.clone()];
    let desired_dirs = vec![desired.clone()];
    let mut watcher = RecordingWatchController {
        fail_unwatch: vec![stale.clone()],
        fail_watch: vec![desired.clone()],
        ..RecordingWatchController::default()
    };
    let mut stderr = Vec::new();

    let changed =
        reconcile_watched_dirs(&mut watcher, &mut watched_dirs, &desired_dirs, &mut stderr)
            .unwrap();

    assert!(changed);
    assert_eq!(watcher.unwatched, vec![stale]);
    assert_eq!(watcher.watched, vec![desired]);
    assert_eq!(watched_dirs, desired_dirs);
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("note: failed to stop watching"), "actual stderr: {stderr}");
    assert!(stderr.contains("note: failed to watch"), "actual stderr: {stderr}");
    assert!(!stderr.contains("restart --watch"), "actual stderr: {stderr}");
}
```

**Implementation**

```rust
#[derive(Default)]
struct RecordingWatchController {
    watched: Vec<PathBuf>,
    unwatched: Vec<PathBuf>,
    fail_watch: Vec<PathBuf>,
    fail_unwatch: Vec<PathBuf>,
}

impl WatchController for RecordingWatchController {
    fn watch_dir(&mut self, dir: &Path) -> miette::Result<()> {
        self.watched.push(dir.to_path_buf());
        if contains_watch_path(&self.fail_watch, dir) {
            return Err(miette::miette!("injected watch failure for {}", dir.display()));
        }
        Ok(())
    }

    fn unwatch_dir(&mut self, dir: &Path) -> miette::Result<()> {
        self.unwatched.push(dir.to_path_buf());
        if contains_watch_path(&self.fail_unwatch, dir) {
            return Err(miette::miette!("injected unwatch failure for {}", dir.display()));
        }
        Ok(())
    }
}

if let Err(err) = watcher.unwatch_dir(old) {
    writeln!(stderr, "note: failed to stop watching {}: {err}", old.display())
        .into_diagnostic()?;
}
if let Err(err) = watcher.watch_dir(new) {
    writeln!(stderr, "note: failed to watch {}: {err}", new.display()).into_diagnostic()?;
}
stderr.flush().into_diagnostic()?;
*watched_dirs = desired_dirs.to_vec();
Ok(changed)
```

**Verification**

```bash
cargo test -p peitho reconcile_watched_dirs_notes_failures_and_converges
```

Red: Task 3 の実装は watcher エラーを `?` で返すため失敗する。Green: stderr ノート化で通す。Refactor: failure note の文言生成を watch/unwatch で読みやすく分け、同じコマンドを再実行する。

## Task 5: deck 変更時の更新を WatchState ベースに置き換える

**Goal**  
`refresh_watch_targets_after_deck_change` が `WatchState.watched_dirs` を実集合として使い、`update_watched_dirs` を削除する。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn deck_refresh_uses_owned_watched_dirs_when_filesystem_state_drifted() {
    let fixture = WatchFixture::new("# Intro\n");
    let old_layouts = fixture._dir.path().join("layouts");
    let alternate_layouts = fixture._dir.path().join("other-layouts");
    fs::create_dir_all(&alternate_layouts).unwrap();
    fs::write(alternate_layouts.join("title-body-code.html"), TEST_LAYOUT_HTML).unwrap();
    let mut state = WatchState::new(
        fixture.options.input.clone(),
        fixture.targets.clone(),
        fixture.targets.watch_dirs(),
    );
    fs::remove_dir_all(&old_layouts).unwrap();
    fs::write(
        &state.input,
        "---\nlayouts: ./other-layouts\n---\n# Intro\n",
    )
    .unwrap();
    let mut watcher = RecordingWatchController::default();
    let mut stderr = Vec::new();

    refresh_watch_targets_after_deck_change(&mut state, &mut watcher, &mut stderr).unwrap();

    assert!(watcher.unwatched.iter().any(|path| path == &old_layouts));
    assert!(watcher.watched.iter().any(|path| path == &alternate_layouts));
    assert_eq!(state.watched_dirs, state.targets.watch_dirs());
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("note: watching new asset paths from frontmatter:"));
}
```

**Implementation**

```rust
fn refresh_watch_targets_after_deck_change(
    state: &mut WatchState,
    watcher: &mut dyn WatchController,
    stderr: &mut dyn Write,
) -> miette::Result<()> {
    let current_assets = match resolve_deck_assets(&state.input) {
        Ok(assets) => assets,
        Err(_) => return Ok(()),
    };
    if state.targets.assets == current_assets {
        return Ok(());
    }

    let next_targets = WatchTargets::new(state.input.clone(), current_assets);
    let desired_dirs = next_targets.watch_dirs();
    reconcile_watched_dirs(watcher, &mut state.watched_dirs, &desired_dirs, stderr)?;
    state.targets = next_targets;
    writeln!(
        stderr,
        "note: watching new asset paths from frontmatter: {}",
        describe_resolved_assets(&state.targets.assets)
    )
    .into_diagnostic()?;
    stderr.flush().into_diagnostic()?;
    Ok(())
}

// update_watched_dirs は削除する。
```

**Verification**

```bash
cargo test -p peitho deck_refresh_uses_owned_watched_dirs_when_filesystem_state_drifted
```

Red: 旧シグネチャと旧差分計算ではコンパイルまたは期待 unwatch で失敗する。Green: `WatchState` を渡して通す。Refactor: `next_targets` と `desired_dirs` の生成順を読みやすく保ち、同じコマンドを再実行する。

## Task 6: 通常イベント handler を WatchState 化して新しい fonts サブディレクトリを追従する

**Goal**  
`handle_watch_paths_with_rebuild` と `handle_watch_event_result` の通常イベント経路を `&mut WatchState` へ移行し、全呼び出し元を同じタスク内で更新する。relevant な通常イベントでリビルドした後、`state.targets.watch_dirs()` を desired として reconcile する。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
fn watch_state_with_fonts() -> (tempfile::TempDir, WatchState, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let deck = root.join("deck.md");
    let fonts = root.join("fonts");
    fs::create_dir_all(&fonts).unwrap();
    fs::write(&deck, "---\nfonts: ./fonts\n---\n# Intro\n").unwrap();
    let targets = resolve_watch_targets(&deck).unwrap();
    let watched_dirs = targets.watch_dirs();
    (dir, WatchState::new(deck, targets, watched_dirs), fonts)
}

#[test]
fn watch_path_handler_reconciles_after_new_fonts_subdir() {
    let (_dir, mut state, fonts) = watch_state_with_fonts();
    let nested = fonts.join("noto");
    fs::create_dir_all(&nested).unwrap();
    let mut watcher = RecordingWatchController::default();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut rebuilds = 0;

    handle_watch_paths_with_rebuild(
        &mut state,
        &mut watcher,
        std::slice::from_ref(&nested),
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| {
            rebuilds += 1;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(rebuilds, 1);
    assert!(watcher.watched.iter().any(|path| path == &nested));
    assert_eq!(state.watched_dirs, state.targets.watch_dirs());
    assert!(stderr.is_empty());
}
```

**Implementation**

```rust
fn handle_watch_paths_with_rebuild<F>(
    state: &mut WatchState,
    watcher: &mut dyn WatchController,
    changed_paths: &[PathBuf],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    mut rebuild: F,
) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    let relevant = changed_paths
        .iter()
        .any(|changed| state.targets.is_relevant_change(changed));

    if relevant {
        if changed_paths
            .iter()
            .any(|changed| same_watch_path(&state.input, changed))
        {
            refresh_watch_targets_after_deck_change(state, watcher, stderr)?;
        }
        rebuild(stdout, stderr)?;
        let desired_dirs = state.targets.watch_dirs();
        reconcile_watched_dirs(watcher, &mut state.watched_dirs, &desired_dirs, stderr)?;
    }

    Ok(())
}

fn handle_watch_event_result<F>(
    result: DebounceEventResult,
    state: &mut WatchState,
    watcher: &mut dyn WatchController,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    rebuild: F,
) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    let events = result.map_err(|err| {
        miette::miette!("watch error: {err}")
    })?;
    state.last_watch_error_note = None;
    let paths = events.into_iter().map(|event| event.path).collect::<Vec<_>>();
    handle_watch_paths_with_rebuild(state, watcher, &paths, stdout, stderr, rebuild)
}

// 同じ差分で全呼び出し元を更新する。
handle_watch_paths_with_rebuild(&mut state, &mut watcher, changed_paths, stdout, stderr, rebuild)?;
handle_watch_event_result(result, &mut runtime.state, &mut watcher, stdout, stderr, &mut rebuild)?;
```

**Verification**

```bash
cargo test -p peitho watch_path_handler_reconciles_after_new_fonts_subdir
```

Red: 旧 handler は `WatchState` を受け取らず、新サブディレクトリを watch しない。Green: `WatchState` signature とリビルド後 reconcile で通す。Refactor: `handle_watch_paths_with_rebuild` と `handle_watch_event_result` の引数数が6以下であることを確認し、同じコマンドを再実行する。

## Task 7: 通常イベントで消えた fonts サブディレクトリを unwatch する

**Goal**  
remove の通常イベントだけが先に届く順序でも、リビルド後 reconcile によって消えたサブディレクトリを `state.watched_dirs` から外す。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn watch_path_handler_reconciles_after_removed_fonts_subdir() {
    let (_dir, mut state, fonts) = watch_state_with_fonts();
    let nested = fonts.join("noto");
    fs::create_dir_all(&nested).unwrap();
    state.watched_dirs = state.targets.watch_dirs();
    fs::remove_dir_all(&nested).unwrap();
    let mut watcher = RecordingWatchController::default();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut rebuilds = 0;

    handle_watch_paths_with_rebuild(
        &mut state,
        &mut watcher,
        std::slice::from_ref(&nested),
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| {
            rebuilds += 1;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(rebuilds, 1);
    assert!(watcher.unwatched.iter().any(|path| path == &nested));
    assert!(!state.watched_dirs.iter().any(|path| path == &nested));
}
```

**Implementation**

```rust
let desired_dirs = state.targets.watch_dirs();
reconcile_watched_dirs(watcher, &mut state.watched_dirs, &desired_dirs, stderr)?;
```

**Verification**

```bash
cargo test -p peitho watch_path_handler_reconciles_after_removed_fonts_subdir
```

Red: stale subdir が `state.watched_dirs` に残る実装なら失敗する。Green: remove イベント後の reconcile で通す。Refactor: add/remove のテストを隣接させ、同じコマンドを再実行する。

## Task 8: watch エラーを非致命化して reconcile する

**Goal**  
`DebounceEventResult::Err` を `Err` として返さず、stderr ノート、desired 再計算、reconcile を実行して watch ループを継続させる。このタスクでは `write_watch_error_note` を作らず、素の `writeln!` と `flush` で書く。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn watch_event_handler_notes_error_reconciles_and_continues() {
    let (_dir, mut state, fonts) = watch_state_with_fonts();
    fs::remove_dir_all(&fonts).unwrap();
    let mut watcher = RecordingWatchController::default();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut rebuilds = 0;
    let result: DebounceEventResult =
        Err(notify::Error::path_not_found().add_path(fonts.clone()));

    handle_watch_event_result(
        result,
        &mut state,
        &mut watcher,
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| {
            rebuilds += 1;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(rebuilds, 1);
    assert!(watcher.unwatched.iter().any(|path| path == &fonts));
    assert!(!state.watched_dirs.iter().any(|path| path == &fonts));
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("note: watch error:"), "actual stderr: {stderr}");
    assert!(!stderr.contains("restart the command"), "actual stderr: {stderr}");
}

#[test]
fn watch_event_handler_returns_ok_when_watcher_reports_error() {
    let (_dir, mut state, _fonts) = watch_state_with_fonts();
    let mut watcher = RecordingWatchController::default();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    handle_watch_event_result(
        Err(notify::Error::generic("backend stopped")),
        &mut state,
        &mut watcher,
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| Ok(()),
    )
    .unwrap();

    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("note: watch error: backend stopped"));
}
```

既存の `watch_event_handler_returns_error_when_watcher_reports_error`
(`crates/peitho/src/main.rs` の 2709 行付近)はこのタスク内で削除し、
上の `watch_event_handler_returns_ok_when_watcher_reports_error` に置き換える。
旧テストは `handle_watch_event_result(...).unwrap_err()` と `stderr.is_empty()` を
期待しており、watch エラー非致命化後は必ず失敗するため残さない。

**Implementation**

```rust
Err(err) => {
    let relevant_error = err
        .paths
        .iter()
        .any(|path| state.targets.is_relevant_change(path));
    let note = format!(
        "note: watch error: {err}\nhelp: removed missing paths from the watch set; they will be watched again after they reappear or the deck frontmatter changes"
    );
    writeln!(stderr, "{note}").into_diagnostic()?;
    stderr.flush().into_diagnostic()?;
    state.last_watch_error_note = Some(note);
    let desired_dirs = state.targets.watch_dirs();
    let changed = reconcile_watched_dirs(watcher, &mut state.watched_dirs, &desired_dirs, stderr)?;
    if changed && relevant_error {
        rebuild(stdout, stderr)?;
    }
    return Ok(());
}
```

同じタスク内で `watch_paths_loop` の呼び出しも `handle_watch_event_result(result, &mut runtime.state, ...)` に更新して、タスク終了時点でコンパイルを通す。

**Verification**

```bash
cargo test -p peitho watch_event_handler_notes_error_reconciles_and_continues
cargo test -p peitho watch_event_handler_returns_ok_when_watcher_reports_error
```

Red: 旧 handler は `Err` を返す。Green: エラー枝を消費して通す。Refactor: Err branch の早期 return を小さく保ち、2つの cargo test コマンドを再実行する。

## Task 9: watch エラーのリビルドを「集合変化あり + relevant」に限定する

**Goal**  
権限エラーが同じ集合のまま繰り返す場合に、200ms ごとのリビルドループを起こさない。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn watch_event_handler_does_not_rebuild_when_error_does_not_change_watch_set() {
    let (_dir, mut state, _fonts) = watch_state_with_fonts();
    let mut watcher = RecordingWatchController::default();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut rebuilds = 0;
    let result: DebounceEventResult = Err(
        notify::Error::generic("permission denied while scanning")
            .add_path(state.input.clone()),
    );

    handle_watch_event_result(
        result,
        &mut state,
        &mut watcher,
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| {
            rebuilds += 1;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(rebuilds, 0);
    assert_eq!(state.watched_dirs, state.targets.watch_dirs());
    assert!(watcher.watched.is_empty());
    assert!(watcher.unwatched.is_empty());
}
```

**Implementation**

```rust
let relevant_error = err
    .paths
    .iter()
    .any(|path| state.targets.is_relevant_change(path));
let desired_dirs = state.targets.watch_dirs();
let changed = reconcile_watched_dirs(watcher, &mut state.watched_dirs, &desired_dirs, stderr)?;
if changed && relevant_error {
    rebuild(stdout, stderr)?;
}
```

**Verification**

```bash
cargo test -p peitho watch_event_handler_does_not_rebuild_when_error_does_not_change_watch_set
```

Red: 無条件 rebuild なら失敗する。Green: `changed && relevant_error` で通す。Refactor: rebuild 条件のローカル変数名を `should_rebuild` にするか判断し、同じコマンドを再実行する。

## Task 10: irrelevant な watch エラーではリビルドしない

**Goal**  
集合が変化しても `err.paths` が `WatchTargets::is_relevant_change` に該当しない場合は、reconcile だけ実行してリビルドしない。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn watch_event_handler_does_not_rebuild_when_error_path_is_irrelevant() {
    let (_dir, mut state, fonts) = watch_state_with_fonts();
    let stale = fonts.parent().unwrap().join("stale-watch-root");
    state.watched_dirs.push(stale.clone());
    let mut watcher = RecordingWatchController::default();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut rebuilds = 0;
    let result: DebounceEventResult =
        Err(notify::Error::path_not_found().add_path(stale.clone()));

    handle_watch_event_result(
        result,
        &mut state,
        &mut watcher,
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| {
            rebuilds += 1;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(rebuilds, 0);
    assert!(watcher.unwatched.iter().any(|path| path == &stale));
    assert!(!state.watched_dirs.iter().any(|path| path == &stale));
}
```

**Implementation**

```rust
let relevant_error = err
    .paths
    .iter()
    .any(|path| state.targets.is_relevant_change(path));
```

**Verification**

```bash
cargo test -p peitho watch_event_handler_does_not_rebuild_when_error_path_is_irrelevant
```

Red: 集合変化だけで rebuild すると失敗する。Green: relevant 条件を加えて通す。Refactor: stale path の fixture 構築を最小化し、同じコマンドを再実行する。

## Task 11: 同一文言 watch エラーノートの連続出力を抑制する

**Goal**  
同じ watch エラーが連続して届く場合、2回目以降の同一ノートを出さない。通常イベントを挟んだ後は再度出せるようにする。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn watch_event_handler_suppresses_consecutive_duplicate_error_notes() {
    let (_dir, mut state, _fonts) = watch_state_with_fonts();
    let mut watcher = RecordingWatchController::default();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    for _ in 0..2 {
        handle_watch_event_result(
            Err(notify::Error::generic("backend noisy")),
            &mut state,
            &mut watcher,
            &mut stdout,
            &mut stderr,
            |_stdout, _stderr| Ok(()),
        )
        .unwrap();
    }

    handle_watch_event_result(
        Ok(vec![notify_debouncer_mini::DebouncedEvent::new(
            state.input.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )]),
        &mut state,
        &mut watcher,
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| Ok(()),
    )
    .unwrap();

    handle_watch_event_result(
        Err(notify::Error::generic("backend noisy")),
        &mut state,
        &mut watcher,
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| Ok(()),
    )
    .unwrap();

    let stderr = String::from_utf8(stderr).unwrap();
    assert_eq!(stderr.matches("note: watch error: backend noisy").count(), 2);
}
```

**Implementation**

```rust
fn write_watch_error_note(
    err: &notify::Error,
    stderr: &mut dyn Write,
    last_watch_error_note: &mut Option<String>,
) -> miette::Result<()> {
    let note = format!(
        "note: watch error: {err}\nhelp: removed missing paths from the watch set; they will be watched again after they reappear or the deck frontmatter changes"
    );
    if last_watch_error_note.as_deref() == Some(note.as_str()) {
        return Ok(());
    }
    writeln!(stderr, "{note}").into_diagnostic()?;
    stderr.flush().into_diagnostic()?;
    *last_watch_error_note = Some(note);
    Ok(())
}

// Err branch
write_watch_error_note(&err, stderr, &mut state.last_watch_error_note)?;

// Ok branch
state.last_watch_error_note = None;
```

**Verification**

```bash
cargo test -p peitho watch_event_handler_suppresses_consecutive_duplicate_error_notes
```

Red: 毎回 note を出す実装では count が 3 になる。Green: 連続重複だけ抑制して通す。Refactor: note 文字列の生成を `write_watch_error_note` 内に閉じ、同じコマンドを再実行する。

## Task 12: watch ループ結線と too_many_arguments を局所確認する

**Goal**  
`watch_paths_loop` が `runtime.state` を handler に渡し、watch handler 群が clippy の引数数閾値を超えないことを確認する。シグネチャ変更済みの既存 watch テスト呼び出しはこの時点で一括確認する。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
#[test]
fn watch_loop_function_accepts_runtime_with_watch_state() {
    let _loop_fn =
        watch_paths_loop::<fn(&mut dyn Write, &mut dyn Write) -> miette::Result<()>>;
    let _paths_handler =
        handle_watch_paths_with_rebuild::<fn(&mut dyn Write, &mut dyn Write) -> miette::Result<()>>;
    let _event_handler =
        handle_watch_event_result::<fn(&mut dyn Write, &mut dyn Write) -> miette::Result<()>>;
}
```

**Implementation**

```rust
fn watch_paths_loop<F>(mut runtime: WatchRuntime, mut rebuild: F) -> miette::Result<()>
where
    F: FnMut(&mut dyn Write, &mut dyn Write) -> miette::Result<()>,
{
    while let Ok(result) = runtime.rx.recv() {
        let mut watcher = NotifyWatchController::new(runtime.debouncer.watcher());
        handle_watch_event_result(
            result,
            &mut runtime.state,
            &mut watcher,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
            &mut rebuild,
        )?;
    }
    Ok(())
}
```

**Verification**

```bash
cargo test -p peitho watch_loop_function_accepts_runtime_with_watch_state
cargo clippy -p peitho --all-targets -- -D warnings
```

Red: loop が旧フィールドや旧 handler signature を参照していればコンパイルで失敗する。Green: `WatchState` 結線で通す。Refactor: 既存 watch テスト群の arrange 部に `WatchState` 初期化を寄せ、2つのコマンドを再実行する。

## Task 13: stdout/stderr 書き込み失敗は致命傷のままにする

**Goal**  
watcher ランタイムエラーは非致命化するが、診断を書けない端末 I/O 失敗は `miette::Result::Err` として返す。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```rust
struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))
    }
}

#[test]
fn watch_event_handler_keeps_stderr_write_failure_fatal() {
    let (_dir, mut state, _fonts) = watch_state_with_fonts();
    let mut watcher = RecordingWatchController::default();
    let mut stdout = Vec::new();
    let mut stderr = FailingWriter;

    let err = handle_watch_event_result(
        Err(notify::Error::generic("backend stopped")),
        &mut state,
        &mut watcher,
        &mut stdout,
        &mut stderr,
        |_stdout, _stderr| Ok(()),
    )
    .unwrap_err();

    assert!(err.to_string().contains("closed"), "actual error: {err}");
}
```

**Implementation**

```rust
writeln!(stderr, "{note}").into_diagnostic()?;
stderr.flush().into_diagnostic()?;
```

**Verification**

```bash
cargo test -p peitho watch_event_handler_keeps_stderr_write_failure_fatal
```

Red: stderr 失敗を無視する実装なら失敗する。Green: `into_diagnostic()?` で通す。Refactor: `FailingWriter` を test module の末尾に置き、同じコマンドを再実行する。

## Task 14: 最終ゲートと手動 E2E を実行する

**Goal**  
単体テスト、ワークスペース全体、lint、format、契約ドリフト、実プロセスの preview 復帰を確認する。

**Files**  
`crates/peitho/src/main.rs`

**Test**

```bash
cargo test -p peitho watch_state_owns_registered_dirs_after_registration
cargo test -p peitho reconcile_watched_dirs_applies_diff_and_updates_owned_set
cargo test -p peitho reconcile_watched_dirs_notes_failures_and_converges
cargo test -p peitho deck_refresh_uses_owned_watched_dirs_when_filesystem_state_drifted
cargo test -p peitho watch_path_handler_reconciles_after_new_fonts_subdir
cargo test -p peitho watch_path_handler_reconciles_after_removed_fonts_subdir
cargo test -p peitho watch_event_handler_notes_error_reconciles_and_continues
cargo test -p peitho watch_event_handler_returns_ok_when_watcher_reports_error
cargo test -p peitho watch_event_handler_does_not_rebuild_when_error_does_not_change_watch_set
cargo test -p peitho watch_event_handler_does_not_rebuild_when_error_path_is_irrelevant
cargo test -p peitho watch_event_handler_suppresses_consecutive_duplicate_error_notes
cargo test -p peitho watch_loop_function_accepts_runtime_with_watch_state
cargo test -p peitho watch_event_handler_keeps_stderr_write_failure_fatal
```

**Implementation**

```bash
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
```

**Verification**

Manual E2E:

```bash
tmpdir=$(mktemp -d)
printf '%s\n' "$tmpdir" > /tmp/peitho-watch-e2e-dir
cat > "$tmpdir/deck.md" <<'EOF'
---
fonts: ./fonts
---
# Intro
EOF
mkdir -p "$tmpdir/fonts/noto"
printf 'font-bytes' > "$tmpdir/fonts/noto/test.woff2"
cargo run -p peitho -- preview "$tmpdir/deck.md" --port 4321 --no-open
```

別シェルで実行する。

```bash
tmpdir=$(cat /tmp/peitho-watch-e2e-dir)
mv "$tmpdir/fonts" "$tmpdir/fonts-gone"
curl -sf http://127.0.0.1:4321/ >/dev/null
mv "$tmpdir/fonts-gone" "$tmpdir/fonts"
touch "$tmpdir/fonts/noto/test.woff2"
curl -sf http://127.0.0.1:4321/ >/dev/null
```

確認する観測結果:

- rename 後も `cargo run -p peitho -- preview ...` のプロセスが生存している。
- stderr に `note: watch error:` が出るが `restart the command` は出ない。
- `fonts` 消失中は stderr に `build failed:` が出て、preview は直前の成功世代を配信し続ける。
- 実行中のリビルド失敗ではエラーページへ差し替わらない。エラーページは初回ビルド失敗時のみの既存仕様として残る。
- rename 戻しと `touch` 後に rebuild が走り、preview が成功世代へ復帰する。
- `cargo run -p peitho -- build "$tmpdir/deck.md" --watch` でも同じ rename / rename 戻しでプロセスが終了しない。
