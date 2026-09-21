# Detect same-second watch writes (Issue #590)

## Problem and dependency evidence

`peitho preview` and `peitho build --watch` used notify 8.2.0's
`PollWatcher` on a 200 ms interval. In `notify-8.2.0/src/poll.rs`,
`system_time_to_seconds` stores `SystemTime` with `as_secs()` (lines 83–87),
and `PathData::new` records that whole-second value as the mtime (line 364).
`PathData::compare_to_event` emits a metadata event only when the new mtime is
greater (lines 408–411). Its separate hash-difference event branch is at lines
412–416. With content comparison disabled, both hashes are `None`, so a second
byte change in the same mtime second satisfies neither branch.

`Config::with_compare_contents(true)` does close that correctness hole, but at
the wrong scope. Every poll walks every entry under each registered directory
(lines 277–331), constructs `PathData` for every entry, and hashes every regular
file when comparison is enabled (lines 365–371). The hash helper opens the file
and reads it to EOF in 512-byte chunks (lines 378–395), even when its metadata
has not changed.

## Watcher history and required input set

Commit `2f755cb` chose polling because native filesystem events were unreliable
in sandboxed runs and polling survives editor atomic-save renames. It also fixed
an infinite rebuild loop caused by treating output changes as inputs when
`dist/` shared the deck's parent directory.

The final design keeps those properties without notify: it periodically
captures the filesystem state itself. Atomic replacement is visible as a
changed final path, and only build inputs enter the capture, so writes under
`dist/`, `.peitho/preview-cache/`, and the code-image cache cannot loop back
into another build.

The tracked set comes from `WatchTargets.roots` and contains:

- the deck and every included Markdown file;
- every referenced image;
- resolved layout HTML, CSS, and syntax files, filtering directory children by
  `.html`, `.css`, and `.sublime-syntax` respectively;
- every non-hidden file and directory under a resolved fonts root, recursively;
- the conventional deck-adjacent `layouts/`, `css/`, `syntaxes/`, and `fonts/`
  candidates, including an explicit `Missing` state while a candidate is
  absent.

Re-resolving targets after a stable change handles frontmatter edits, include
set changes, referenced-image set changes, highlighter recovery, and a
zero-configuration asset directory appearing or disappearing while the
process is running.

Text asset directories use the same `collect_asset_files` selection as the
build itself. In particular, a directory entry must resolve to a regular file
and have the requested extension. Hidden regular files retain the build's
existing behavior, while dangling symlinks such as Emacs's `.#talk.css` lock
file are ignored by both capture and build.

## Rejected designs

### Global or split notify content comparison

Global `compare_contents` hashes all regular files reached from every
registered directory five times per second, including images and fonts. The
repository's `examples/custom-fonts/fonts` tree is 107,408 bytes, already about
0.54 MB/s of continuous reads; a 100 MiB unsubsetted CJK font tree would cause
about 500 MiB/s.

Splitting text and binary roots across two stock debouncers does not close the
cost hole because Peitho registered directories, not individual files. Marking
the deck's parent as text makes notify hash every regular file directly beside
the deck, including PDF exports, videos, archives, and images. Shared text and
binary directories make the “text wins” rule even broader. The two debouncers
also created separate debounce windows and initially added a full 200 ms wait
to every rebuild while trying to coalesce rare cross-class events.

### Single-debouncer routing wrapper

The first split implementation placed two pollers behind a custom
`notify::Watcher`. It needed an `Arc<Mutex<_>>` handler, a private registration
map, and repurposed `RecursiveMode` as internal cost-routing syntax. Besides
retaining the directory hashing cost, it obscured notify's public semantics and
added roughly 600 lines of plumbing.

### Two detectors with an echo heuristic

The next design let both a Peitho snapshot and notify events request rebuilds,
then called an event batch an echo when every relevant event path matched the
installed snapshot. Real PollWatcher batches defeat that rule: an atomic save
inside `css/`, `layouts/`, `syntaxes/`, or a fonts tree includes the directory
path beside the file path. Directories were relevant event paths but not file
snapshot keys, so a `tmp + rename` probe over `css/talk.css` rebuilt twice while
main rebuilt once. The echo early return also skipped watch reconciliation and
error-note clearing.

### Notify as a wake-up hint

Making the snapshot the only decision and notify merely a wake-up removed the
event-shape problem, but left a debouncer, channel, directory registration,
re-registration, error suppression, and two loop arms solely to request work
that the 200 ms timer already requested. PollWatcher normally delivered the
wake after the periodic snapshot had acted, so it added machinery without
improving detection or shutdown behavior. Removing notify also removes the
registration failures that required all of that bookkeeping.

## Rebuild-race evidence

Refreshing the installed snapshot after a build was incorrect. A second save
could land after the build read its inputs but before the refresh; that refresh
then labeled bytes the build never consumed as already built. A same-second
change could also be invisible to notify.

The real-process reproduction used `examples/peitho-tour` and placed the second
write 0.15 seconds after the first:

| Implementation | Served the second write within 5 seconds |
| --- | ---: |
| `origin/main` | 6/6 |
| post-build-snapshot redesign | 1/6 |

The required invariant is capture-before-build: install only the fingerprints
captured before invoking the rebuild callback, whether that callback succeeds
or fails. A write during the callback therefore differs on a later tick and
cannot be silently marked as consumed.

## Final design

There is one change detector and no filesystem-watcher backend. Every 200 ms,
the watch loop sleeps and runs one snapshot step:

1. Capture the current tracked inputs. If that map equals the installed map,
   clear any pending candidate and do nothing.
2. If it differs, store it as a candidate. Do not rebuild yet.
3. On the next tick, rebuild only when the new capture equals the candidate.
   If it changed again, replace the candidate and continue waiting. This
   two-capture stability debounce merges truncate-then-write, backup-rename,
   partial-copy, and other multi-step saves without exposing their intermediate
   state.
4. Before rebuilding, refresh `WatchTargets` and capture the refreshed set. If
   target discovery added or removed keys, that refreshed capture becomes the
   next candidate and must also be stable on a later tick.
5. Invoke the existing shared rebuild callback and install that stable,
   pre-build capture afterward, on success or failure.

A single write is detected on one tick and rebuilt on the following tick. A
file that changes on every tick is rebuilt once after it settles. If a write
lands during a rebuild, the installed capture still describes the bytes known
before the callback, so the later bytes start a new two-capture cycle. Reading
a partial file can therefore delay a rebuild or conservatively cause a later
one, but cannot hide the final state.

The fingerprints are:

| Input shape | Representation |
| --- | --- |
| Deck, includes, layout HTML, CSS, syntax files | `DefaultHasher` over bytes |
| Referenced images and regular font files | `(modified(), len)` |
| Relevant directory | Stable presence marker plus separately keyed relevant children |
| Relevant root or binary-tree symlink | Link target and target metadata; explicit text-file roots also hash content |
| Text-directory symlink resolving to a regular file | Same content hash as any other loaded text asset |
| Missing tracked input or conventional asset candidate | Explicit `Missing` marker |

Binary contents are never opened or hashed. `modified()` remains a
full-resolution `SystemTime`; Peitho does not truncate it to seconds.

## Cost and residual risk

Each tick performs one directory listing per tracked asset directory (recursive
for non-hidden font subdirectories), one metadata lookup per tracked root or
listed entry, and reads and hashes only tracked text files. Symlinks need an
additional target lookup. The deck's parent directory is not listed, so an
unrelated export, video, archive, or image beside `deck.md` contributes no read
traffic.

For `examples/peitho-tour`, the tracked text files total 19,405 bytes: the deck,
two CSS files, and four layouts. At five ticks per second that is 97,025 bytes/s
(about 95 KiB/s), plus the small listings and metadata calls. Its 1,959,893
bytes of referenced images are statted but never read.

### Final design — measured

Black-box run of the final two-tick design on release binaries of this branch
and `origin/main` (separate target dirs, binaries verified to differ), driven
as real `peitho preview --no-open --port N` and `peitho build --watch`
processes on a copy of `examples/peitho-tour` plus a font and an include.

| Measurement | Final design | `origin/main` |
| --- | ---: | ---: |
| Same-second race, deck (write A, gap ∈ {0.05,0.15,0.30,0.45,0.70} s, write B; 30 trials each mode) | B built 30/30, max 0.63 s (0.85 s on a 600-slide deck) | B missed 15/30 (preview), 14/30 (watch) |
| Same-second race, include / layout / css / font / image, plain and atomic | 30/30 each | font 11/30 and image 11/30 missed |
| Save landing inside a 1.9 s build (3000-slide deck) | 6/6 picked up | — |
| Single-save rebuild distribution (20 saves, random offsets) | {+1: 20} | {+1: 20} |
| Atomic save (temp + rename), vim-style save, deck+font together, css+font+image together | +1 each | +1 |
| truncate, 300 ms, write (10 runs; `/sync` polled every 50 ms) | +1, `buildError` never non-null | — |
| 40 MB font copied slowly (1.6–2.5 s) | +1 after it settles | — |
| Irrelevant / hidden / swap files beside the deck; output-dir churn; unchanged tree 10 s | +0 | +0 |
| Median save → visible latency (10 runs) | preview 485 ms, watch 429 ms | 462 ms, 460 ms |
| Idle CPU over 20 s (cputime): baseline / 560 MB beside `deck.md` / 60 MB font / 200 fonts / 2000 nested fonts / 200 images | 0.06 / 0.04 / 0.03 / 0.07 / 0.83 / 0.05 s | 0.06 / 0.04 / 0.02 / 0.07 / 1.04 / 0.05 s |
| chmod 000 on a css file / include / `fonts/` dir, then back | one failed build with the real diagnostic, one recovery | css and include chmods invisible (+0) |
| Deck directory renamed away and back | one failed build, one rebuild, no crash | two failures plus a watch-error note |
| SIGINT / SIGTERM (5 runs each mode) | exit in 2–11 ms | 5–22 ms |

Design ceiling, not a defect: a truncate-then-write whose pause exceeds the
two-tick window (about 400 ms) builds the empty intermediate once before the
good build (0.30 s pause: 12/12 clean; 1.0 s: 12/12 flash). The 300 ms
requirement holds; a longer window would trade save-to-visible latency for it.

The accepted residual is a same-second double write to a binary input on a
filesystem exposing only whole-second mtimes when the file length is also
unchanged. Filesystems with sub-second mtimes detect it through the metadata
fingerprint. A permission-denied text file has the constant `Other`
fingerprint, so changes made while it remains unreadable are not visible until
it becomes readable again. Finally, a collision in the 64-bit text content
hash would miss a rebuild; that risk is negligible for this watch-loop use.

## User-visible diagnostics

The frontmatter target-refresh note remains. A stable disappearance of an
explicit input still reaches the ordinary rebuild path and prints the existing
build diagnostic; a missing conventional candidate remains normal and silent.

The following watcher-backend diagnostics were removed:

- startup failures to create a watcher or register a directory;
- runtime watch/unwatch failure notes and their duplicate suppression;
- generic notify backend errors and re-registration notes.

Those failure modes no longer exist because there is no OS watcher to create,
register, or reconcile. Process lifetime is unchanged: `build --watch` runs
until the process is terminated, and preview's watch loop remains owned by the
preview process.

## TDD and verification

The deterministic tests exercise the real snapshot step against on-disk state:

- the pure two-capture candidate transition;
- a truncate-then-write sequence with identical forced mtimes producing no
  failed intermediate build;
- a file changing on every tick rebuilding once after it settles;
- one ordinary write rebuilding on its second identical capture;
- a write performed inside the rebuild callback being detected later;
- installing the pre-build capture even when the callback returns an error;
- capturing before both the initial `build --watch` build and the initial
  preview build;
- reporting build and emit failures without ending the loop, followed by a
  successful fix rebuild;
- deck, include, layout/CSS, referenced-image, new-font, nested-font, and
  combined deck-plus-font changes;
- preserving prior targets after invalid frontmatter asset paths; retaining
  includes/assets through a parse failure; removing dropped includes from the
  tracked set; image-target refresh after reference changes and highlighter
  recovery; initial-build failure and recovery; and a conventional `css/`
  directory appearing at runtime;
- target resolution not running code-image renderers, and a Mermaid rebuild
  leaving the snapshot unchanged despite writing its generated cache;
- build/capture agreement for text-directory entries, including ignoring a
  dangling Emacs lock symlink while tracking a regular CSS file;
- extension filtering, hidden font entries, outputs, preview cache, and
  unrelated deck-directory files causing no rebuild.

Run `cargo test --workspace` three times, then
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all --check`, and `cargo tree -i notify`. Do not run Chrome or
ignored tests.

## Summary

<!-- derived-from #final-design -->
<!-- derived-from #cost-and-residual-risk -->
<!-- derived-from #user-visible-diagnostics -->

Peitho now owns one narrowly scoped periodic fingerprint snapshot. A two-tick
stability debounce avoids rebuilding intermediate save states, and the stable
capture is installed only after the build it precedes. This closes same-second
text misses without hashing binaries, reading unrelated neighbors, depending
on backend event shapes, or retaining notify-only plumbing.
