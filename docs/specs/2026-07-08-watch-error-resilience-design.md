# Watch error resilience — keep the preview/build watch loop alive across trouble on watched paths

- Date: 2026-07-08
- Status: Design finalized (pre-implementation)
- Related: watch-related code in `crates/peitho/src/main.rs` (`watch_paths_loop` / `handle_watch_event_result` / `refresh_watch_targets_after_deck_change` / `spawn_preview_watch`)

## 1. Problem

While `peitho preview` is running, if the directory referenced by `fonts:` in frontmatter (`../../fonts/noto-sans`) is renamed or deleted, the process dies with the following output and the server has to be restarted.

```
rebuilt 18 slide(s) into .peitho/preview-cache
preview watch error: watch error: IO error for operation on ./../../fonts/noto-sans: No such file or directory (os error 2) about ["./../../fonts/noto-sans"]
help: restart the command after checking file watcher permissions
exit 1
```

Build errors follow the design "print to stderr + serve an error page + server stays alive", so only watcher runtime errors dying instantly is an inconsistency.

## 2. Facts confirmed by measurement (notify 8.2.0 / notify-debouncer-mini 0.7.0)

- `PollWatcher::watch()` **does not return Err** on a non-existent path.
  `WatchData::new` emits an error **event** on `fs::metadata` failure and skips registration (`WatchData::new` / `watch_inner` in `poll.rs`).
  So registration failure arrives asynchronously as `DebounceEventResult::Err` on the event channel, not as a `Result` at the call site.
- When a registered root disappears, WalkDir emits a root-missing error on each poll's `rescan`, and **error events are emitted on every polling interval (200ms)**. PollWatcher does not remove the root from watches by itself.
  → Naive "log and continue" spams error notes forever.
- Known entries under the root **also arrive as normal (Ok) remove events** via `rescan`'s disappeared detection. So for many directory-disappearance cases, the rebuild trigger is available on the normal event path too.
- `DebounceEventResult = Result<Vec<DebouncedEvent>, notify::Error>`, and
  `notify::Error` has `paths: Vec<PathBuf>` (the target paths are available).

## 3. Broken invariants (root cause)

1. **Watch loop liveness**: `handle_watch_event_result` (main.rs:542)
   propagates `DebounceEventResult::Err` fatally with `?`, and preview
   kills the process with `std::process::exit(1)` in `spawn_preview_watch`
   (main.rs:1532). `build --watch` goes through the same `watch_paths_loop`
   and therefore also exits.
2. **Ownership of the watched set**: nobody holds "the set of directories
   actually watched right now"; `WatchTargets::watch_dirs()` is
   **recomputed from filesystem state each time**. When the set drifts
   (e.g., when a watched directory disappears, `is_dir()` returns false
   and the parent is returned), the diff calculation in
   `refresh_watch_targets_after_deck_change` → `update_watched_dirs`
   diverges from the actual registration state, and `unwatch`'s
   `watch_not_found` propagates fatally — a **latent bug** on the deck-change path too.
3. **Watched-set convergence trigger**: watched-set updates happen only
   on deck-file changes. When a new sub-directory appears in the fonts
   tree, or when a disappeared directory reappears, the watched set stays
   stale and does not catch up (non-recursive watch so changes under it
   are lost). Another symptom of the same root cause
   (the set has no owner, and the convergence step exists only on one path).

## 4. Approach comparison

### A. Just log the error and continue (rejected)

- Pros: minimal diff.
- Cons: PollWatcher keeps emitting errors on the deleted root every 200ms,
  spamming notes. The watched-set drift (§3-2, §3-3) is not fixed.
  Bandaid over symptoms.

### B. Rebuild the entire debouncer on watch error and re-register everything (runner-up)

- Pros: no state tracking, simple semantics. `watch_dirs()` reflects
  filesystem state, so a deleted root naturally switches to watching the parent.
- Cons: reconstruction requires swapping rx/debouncer inside the loop —
  significant loop-structure changes. The latent bug on the deck-change
  path (§3-2) remains unless "abandon incremental updates and unify on
  full reconstruction" is done. Unifying on full reconstruction causes
  frequent event-channel re-wiring.

### C. Own the actual watched set and converge via a single reconcile (adopted)

- Pros: fixes the root cause (all three points in §3) with one upstream
  seam. Diff application between "actually registered" and "desired
  (derived from targets + current FS state)" becomes the sole path for
  change, and the three triggers — watch error, deck change, normal
  change — all funnel into the same function. Future callers do not
  need to remember any filter.
- Cons: `WatchRuntime` gains one piece of state. Diff-application tests
  are required.
- Complexity: medium.

## 5. Adopted design (C)

### Invariants to restore

- **The watch loop, once started successfully, does not exit on runtime
  errors from the watcher or the filesystem.** Errors are diagnostic
  notes, treated on par with build errors.
- **The actual watched set is state owned by `WatchRuntime`, and all
  registration changes go through a single reconcile function.** Never
  guess "what am I watching now" by recomputing from the filesystem.

### Changes (revised 2026-07-08 after review. Revision reasons in §5.1)

1. Bundle watch-related state (`input` / `targets` / actual watched set
   `watched_dirs` / note-suppression state) into `WatchState`, and let
   `WatchRuntime` hold `state + debouncer + rx` (avoid argument bloat
   and `clippy::too_many_arguments`, and make ownership explicit).
2. New function `reconcile_watched_dirs(watcher, watched: &mut Vec<PathBuf>,
   desired: &[PathBuf], stderr, emitted_notes: &mut HashSet<String>)`
   → `ReconcileResult { changed, had_failures }`:
   - unwatch `watched - desired`, watch `desired - watched`.
     Diff calculation normalizes each path once (the canonicalized value
     on success, the original path on failure) into a HashSet key
     (avoids O(n×m) `same_watch_path` exhaustive comparison and double
     canonicalization).
   - unwatch/watch failures become stderr notes and continue
     (not fatal). Failure notes are suppressed via the caller's
     "already-emitted notes" set, and `had_failures` feeds the
     clear-condition decision in change 4.
   - **Only successfully registered directories are recorded in `watched`.**
     `WatchController::watch_dir` returns Err synchronously if the target
     does not exist (PollWatcher's `watch()` silently skips non-existent
     paths and returns Ok, so we do the existence check on the controller
     side; do not create the false state "recorded as registered but not
     actually registered"). A failed desired is not recorded in watched,
     so it is automatically retried on the next reconcile trigger.
3. Desired-set derivation (`WatchTargets::watch_dirs()`): when the root
   path does not exist, **walk up to the nearest existing ancestor
   directory** as the watch target (the previous fixed "immediate parent"
   left a hole where, if the parent also disappears, a non-existent
   directory ends up in desired, and combined with the existence check
   in 2, nobody watches = revival cannot be observed). Revival is
   observed as a normal event on the ancestor watch, and reconcile
   re-watches the tree in stages.
4. Non-fatal the error branch of `handle_watch_event_result`:
   - Run reconcile **first**, then write the `watch error: {err}` note
     to stderr. The help is a single mechanism-explanation wording that
     is "always true regardless of event order" (missing watch targets
     are dropped and re-watched automatically when they reappear or the
     deck frontmatter changes; if this error persists, check file
     watcher permissions). Branching the help on per-delivery reconcile
     results (the set stopped watching) produces notes that contradict
     reality when the remove Ok event arrives first and the Ok branch
     unwatches first, or when multiple Err deliveries land from the
     same incident (detected in self-review R1).
     Enumerating disappeared paths is unnecessary since they are in the
     err text itself.
   - Rebuild fires at most once when "reconcile changed the set".
     Do not use `err.paths` relevance judgment (§5.1 revision [2]: if
     the root's unwatch precedes that root's own error delivery, there
     is a race where relevance never becomes true, and preventing the
     rebuild loop is already covered by the set-change gate alone).
     If it is a missing explicit-frontmatter asset the rebuild will
     fail, and preview keeps the existing behavior: `build failed:` to
     stderr and continue to serve the last successful generation
     (runtime error-page swap remains an existing spec only on
     first-build failure).
   - Note suppression uses an "already-emitted notes" set held in
     `WatchState` (comparing only against the previous single entry
     lets a flood recur when persistent errors from multiple roots
     arrive interleaved — §5.1 revision [1]). The suppression key is
     the err text; the reconcile watch/unwatch failure notes go through
     the same set. The set is cleared only on "an Ok batch where
     reconcile had zero watch/unwatch failures" ("clear when we did not
     print" causes the print → suppress → clear → print half-rate flood
     to recur; unconditional clear does too).
5. The normal event path (`handle_watch_paths_with_rebuild`)
   **reconciles after every event batch** regardless of relevance
   (a no-op with zero watcher calls if the set matches). This lets the
   watched set follow "revival of a deleted directory",
   "new sub-directory inside the fonts tree", and "stepwise recovery
   via ancestor watch" (§3-3 fix). Rebuild fires at most once when
   "a relevant change happened" **or "reconcile changed the set"**
   (symmetric with the changed gate on the Err branch). Without the
   latter, a revival event for a completely disappeared asset
   (creation of an ancestor directory) does not match relevance, so
   even though the tree is re-watched, output stays stale until the
   next relevant change (real behavior observed in E2E).
   The set only changes when the watched topology changes, so this
   does not cause a rebuild loop.
6. `refresh_watch_targets_after_deck_change` only re-resolves targets
   and emits a note; reconcile is aggregated into the single per-batch
   call at the end of 5 (avoids a double reconcile on deck change).
   Delete the existing `update_watched_dirs`.
7. Keep the `process::exit(1)` path in `spawn_preview_watch`, but the
   only cases that reach it are ones where continuing the watch is
   meaningless (stdout/stderr write failure, etc.) — watcher-derived
   errors are consumed in 4 and do not propagate as Err.
   `build --watch`'s `watch_paths_loop` goes through the same function
   and is fixed at the same time.

### 5.2 Handling of hidden directories (2026-07-08 self-review R2/R3)

Exclude dot-prefixed **directories** under the fonts tree from both the
watch set and relevance judgment (making the existing dot-prefixed
**files** exclusion decision — pinned by
`watch_ignores_dotfiles_in_fonts_dir` — consistent with directories and
all path levels). Reason: with per-batch reconcile, creation of `.git`
etc. under fonts would grow desired and trigger a rebuild, and further
changes to non-dot-named files inside it (`refs/...` etc.) would leak
through leaf-name filtering and become rebuild sources. Build-time
copying stays verbatim (hidden included) as before, so this preserves
and extends the existing asymmetry "hidden is copied but does not
trigger rebuild". The tradeoff — no auto-rebuild on font edits inside a
hidden directory — is intended (the latest hidden contents are copied
whenever a visible change triggers a rebuild).

### 5.1 Revision log after review (2026-07-08)

The following were confirmed in verified review against the initial
implementation and are reflected in 2–6 above:

- **[0] Silent registration failures**: initial version had "converge
  watched to desired regardless of success/failure", but PollWatcher's
  `watch()` silently skips non-existent paths, so a root whose parent
  is also gone gets recorded as "registered", then matches desired
  forever and is never retried, and even on revival nobody watches it
  (silent degradation). → Sync-ify the existence check in the
  controller, record only successes in watched, and derive desired
  up to the nearest existing ancestor.
- **[1] Direct-predecessor-only suppression comparison**: different
  persistent errors arriving interleaved disable suppression and log
  flooding recurs. → Suppress via an already-emitted note set.
- **[2] Relevance-gate race**: if reconcile unwatches the root first,
  that root's error may never be delivered, and rebuild can be
  perpetually suppressed. → Simplify the gate to "set change only".
- **[3] Lying note**: fixed wording "removed missing paths" printed
  before reconcile contradicts reality on no-op cases like permission
  errors. → Emit wording after reconcile, tailored to the result.
- Also resolved O(n×m)×canonicalize diff calculation, double reconcile
  on deck change, unnecessary clones, and removed the "does nothing at
  runtime" function-pointer test added during implementation
  (`watch_loop_function_accepts_runtime_with_watch_state`)
  (the existing main-origin `watch_build_function_is_available_for_cli_dispatch`
  is left out of this PR's scope).

### Enumeration of reachable error paths (silent-drop-forbidden check)

| Event | Behavior |
| --- | --- |
| Watched directory disappears | One note + unwatch + watch the nearest existing ancestor. Set changes so one rebuild (build failure note if explicit frontmatter; preview keeps serving the last successful generation) |
| Parent also gone (only the parent comes back first on revival, etc.) | Normal event on the ancestor watch → per-batch reconcile stepwise re-watches the tree, and set change drives rebuild (output recovers without waiting for a relevant event) |
| A vanished directory reappears | Normal event on the parent/ancestor watch → rebuild + reconcile re-watches the tree |
| New sub-directory inside the fonts tree | Normal event → rebuild + reconcile adds it to the watch |
| Persistent watch error like permission denied | Note (suppressed via emitted-set; cleared by Ok event) + reconcile no-op + no rebuild. Loop continues |
| Watch registration failed (target vanished, etc.) | Note + not recorded in watched → automatic retry on the next reconcile trigger |
| Deck change alters asset paths | Re-resolve + single per-batch reconcile at end (owned-set-based, so no death from watch_not_found) |
| Watcher itself cannot be created / initial registration fails at startup | Fails to start as before (fatal. Correct because the server has not started yet. Initial registration failure requires a TOCTOU race since `watch_dirs()` only returns existence-checked dirs) |
| stdout/stderr write failure | Fatal as before (terminal lost. Continuing is meaningless) |

### Testing policy

In addition to existing unit tests using the `WatchController` fake:

- Watch error event (paths contain the vanished root) → note emitted,
  unwatch called, function returns Ok (loop continues).
- reconcile: watch/unwatch is called per the watched/desired diff;
  unwatch failure notes and continues.
- Watch registration failure (target vanished) → not recorded in
  watched, retried on the next reconcile.
- Desired derivation with a non-existent root → the nearest existing
  ancestor is watched.
- No set change after error (permission-error equivalent) → rebuild
  is not called, and the note has help like "check file watcher
  permissions".
- Set change after error → rebuild is called once.
- Persistent errors with different wording arriving interleaved
  produce one note per wording (Ok events reset so it can be emitted
  again).
- Deck-change refresh does not die when the actual set has drifted
  (a case where the old implementation makes watch_not_found fatal).
- E2E (manual, real browser): start preview → rename the fonts
  directory → server survives, watch-error note emitted, `build failed:` note
  (with explicit frontmatter; serving stays on the last successful generation)
  → rename back → next build succeeds and preview recovers.

### Verification points (confirm by measurement at implementation time)

- On directory disappearance, what order and granularity remove normal
  events and Err arrive (debouncer-mini's behavior of flushing errors
  as separate messages). The design works for both orders, but confirm
  before writing test expected values.

## 6. Record of rejected options

- "Fix only preview and leave build --watch as is": another symptom of
  the same root cause, so not allowed (CLAUDE.md root-cause rule).
- "Stop `process::exit` on error and quietly end just the watch thread":
  silent degradation where preview quietly stops updating, violating the
  silent-drop-forbidden pillar.
