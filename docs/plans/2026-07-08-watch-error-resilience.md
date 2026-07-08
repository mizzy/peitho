# Watch error resilience implementation plan

<!-- constrained-by ../specs/2026-07-08-watch-error-resilience-design.md -->

## Scope

Keep the `peitho preview` / `peitho build --watch` watch loop alive across deletions, revivals, and new sub-directories under monitored directories. The adopted design is fixed to §5(C) of [`../specs/2026-07-08-watch-error-resilience-design.md`](../specs/2026-07-08-watch-error-resilience-design.md).

To avoid `clippy::too_many_arguments`, group the actual watch set and watch-related state into `WatchState`.

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

The logical return value of `reconcile_watched_dirs` is a `bool` for "did the set change". stderr write failures stay fatal, so the Rust type is `miette::Result<bool>`.

## Task 1: measure the shape of debouncer-mini's disappearance events

**Goal**  
Per the design's verification points, confirm in a scratch test how `Ok(Vec<DebouncedEvent>)` and `Err(notify::Error { paths, .. })` arrive when a registered directory is deleted. Remove the test after success; do not leave it in the tree.

**Files**  
`crates/peitho/src/main.rs` (temporary edit only; revert at task end)

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
// Temporarily add under #[cfg(test)] mod tests.
// After probing, delete this entire test.
```

**Verification**

```bash
cargo test -p peitho watch_probe_directory_delete_event_shape -- --nocapture
git diff -- crates/peitho/src/main.rs
```

Red: if the assertion fails in a shape different from the design assumption, capture `--nocapture` output and revisit the design premise. Green: observe both `Ok` and `Err`. Refactor: delete the scratch test and confirm the second command shows no residual diff.

## Task 2: introduce WatchState and let WatchRuntime own it

**Goal**  
Hold the set of directories that were successfully registered at startup in `WatchState.watched_dirs`, and reshape `WatchRuntime` to hold only `state + debouncer + rx`.

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

Red: fails because `WatchState` is undefined. Green: passes with `WatchRuntime` owning `state`. Refactor: order initialization inside `prepare_watch_loop` as `targets -> watched_dirs -> state`, and rerun the same command.

## Task 3: add diff application to reconcile

**Goal**  
`unwatch` for `watched_dirs - desired_dirs`, `watch` for `desired_dirs - watched_dirs`, and converge `watched_dirs` to `desired_dirs` on success.

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

Red: fails because `reconcile_watched_dirs` is undefined. Green: passes by applying the diff. Refactor: consolidate duplicate `contains_watch_path` calls and rerun the same command.

## Task 4: turn reconcile's watch/unwatch failures into non-fatal notes

**Goal**  
Convert `watch_not_found` and `PathNotFound` registration change failures into stderr notes and continue, converging the owned set to `desired_dirs` regardless of success or failure.

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

Red: fails because Task 3's implementation returns watcher errors with `?`. Green: passes by turning them into stderr notes. Refactor: split failure-note wording between watch/unwatch for readability and rerun the same command.

## Task 5: replace deck-change updates with a WatchState-based path

**Goal**  
Make `refresh_watch_targets_after_deck_change` use `WatchState.watched_dirs` as the actual set and delete `update_watched_dirs`.

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

// Delete update_watched_dirs.
```

**Verification**

```bash
cargo test -p peitho deck_refresh_uses_owned_watched_dirs_when_filesystem_state_drifted
```

Red: fails to compile or fails on the expected unwatch under the old signature and diff calculation. Green: passes by threading `WatchState` through. Refactor: keep the ordering of `next_targets` and `desired_dirs` readable and rerun the same command.

## Task 6: convert the normal event handler to WatchState and follow new fonts subdirectories

**Goal**  
Migrate the normal event path in `handle_watch_paths_with_rebuild` and `handle_watch_event_result` to `&mut WatchState` and update every caller in the same task. After rebuild on a relevant normal event, reconcile with `state.targets.watch_dirs()` as desired.

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

// Update all callers with the same diff.
handle_watch_paths_with_rebuild(&mut state, &mut watcher, changed_paths, stdout, stderr, rebuild)?;
handle_watch_event_result(result, &mut runtime.state, &mut watcher, stdout, stderr, &mut rebuild)?;
```

**Verification**

```bash
cargo test -p peitho watch_path_handler_reconciles_after_new_fonts_subdir
```

Red: the old handler does not take `WatchState` and does not watch the new sub-directory. Green: passes with the `WatchState` signature and post-rebuild reconcile. Refactor: verify `handle_watch_paths_with_rebuild` and `handle_watch_event_result` have at most six arguments and rerun the same command.

## Task 7: unwatch fonts sub-directories that disappear via normal events

**Goal**  
Even when only the remove normal event arrives first, the post-rebuild reconcile removes the vanished sub-directory from `state.watched_dirs`.

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

Red: an implementation that leaves the stale subdir in `state.watched_dirs` fails. Green: passes with reconcile after the remove event. Refactor: put the add/remove tests next to each other and rerun the same command.

## Task 8: make watch errors non-fatal and reconcile

**Goal**  
Stop returning `DebounceEventResult::Err` as `Err`; write a stderr note, recompute desired, run reconcile, and keep the watch loop running. This task does not create `write_watch_error_note`; use raw `writeln!` and `flush`.

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

Delete the existing `watch_event_handler_returns_error_when_watcher_reports_error`
(around line 2709 in `crates/peitho/src/main.rs`) in this task and replace it with
`watch_event_handler_returns_ok_when_watcher_reports_error` above.
The old test expects `handle_watch_event_result(...).unwrap_err()` and `stderr.is_empty()`,
which will always fail after watch errors become non-fatal, so it must not remain.

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

Also update the `watch_paths_loop` call within the same task to `handle_watch_event_result(result, &mut runtime.state, ...)` so compilation passes at the end of the task.

**Verification**

```bash
cargo test -p peitho watch_event_handler_notes_error_reconciles_and_continues
cargo test -p peitho watch_event_handler_returns_ok_when_watcher_reports_error
```

Red: the old handler returns `Err`. Green: passes by consuming the error branch. Refactor: keep the Err branch's early return small and rerun both cargo test commands.

## Task 9: gate watch-error rebuilds on "set changed + relevant"

**Goal**  
Do not trigger a rebuild loop every 200ms when the same permission error keeps repeating on the same set.

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

Red: unconditional rebuild fails. Green: passes with `changed && relevant_error`. Refactor: decide whether to rename the rebuild-condition local to `should_rebuild` and rerun the same command.

## Task 10: do not rebuild on irrelevant watch errors

**Goal**  
When the set changes but `err.paths` do not match `WatchTargets::is_relevant_change`, run only reconcile and do not rebuild.

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

Red: rebuilding on set change alone fails. Green: passes by adding the relevant condition. Refactor: minimize the stale-path fixture construction and rerun the same command.

## Task 11: suppress consecutive duplicate watch-error notes

**Goal**  
When the same watch error arrives consecutively, do not emit the same note a second time or beyond. After an intervening normal event, allow it again.

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

Red: an implementation that emits the note every time counts 3. Green: passes by suppressing consecutive duplicates. Refactor: keep the note-string construction inside `write_watch_error_note` and rerun the same command.

## Task 12: wire up the watch loop and locally check too_many_arguments

**Goal**  
Confirm `watch_paths_loop` passes `runtime.state` to the handlers and that the watch handler family stays under clippy's argument-count threshold. Check the existing watch-test call sites with updated signatures collectively at this point.

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

Red: fails to compile if the loop references the old field or the old handler signature. Green: passes with the `WatchState` wiring. Refactor: consolidate `WatchState` initialization in existing watch tests' arrange sections and rerun both commands.

## Task 13: keep stdout/stderr write failures fatal

**Goal**  
Watcher runtime errors become non-fatal, but terminal I/O failures where diagnostics cannot be written stay `miette::Result::Err`.

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

Red: an implementation that ignores stderr failure fails. Green: passes with `into_diagnostic()?`. Refactor: place `FailingWriter` at the end of the test module and rerun the same command.

## Task 14: run final gates and manual E2E

**Goal**  
Verify unit tests, the full workspace, lint, format, contract drift, and real-process preview recovery.

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

In a separate shell:

```bash
tmpdir=$(cat /tmp/peitho-watch-e2e-dir)
mv "$tmpdir/fonts" "$tmpdir/fonts-gone"
curl -sf http://127.0.0.1:4321/ >/dev/null
mv "$tmpdir/fonts-gone" "$tmpdir/fonts"
touch "$tmpdir/fonts/noto/test.woff2"
curl -sf http://127.0.0.1:4321/ >/dev/null
```

Observations to confirm:

- The `cargo run -p peitho -- preview ...` process is still alive after the rename.
- stderr shows `note: watch error:` but not `restart the command`.
- While `fonts` is missing, stderr shows `build failed:` and preview keeps serving the last successful generation.
- Runtime rebuild failures do not swap in the error page. The error page remains an existing behavior only on first-build failure.
- After renaming back and `touch`, a rebuild fires and preview returns to a successful generation.
- The same rename / rename-back also does not terminate `cargo run -p peitho -- build "$tmpdir/deck.md" --watch`.
