# Cache the user-syntax SyntaxSet relink (Issue #431)

Date: 2026-08-17
Issue: #431 — with the two-face base set, `with_user_dir`/`with_user_files`
pay a ~224ms `SyntaxSetBuilder::build()` relink per construction (measured;
was ~85ms with the 75-syntax default set), and `build_artifacts` constructs
a Highlighter on every watch/preview rebuild — ~224ms of fixed latency per
save for decks shipping custom syntaxes.

## Design

A process-wide single-entry cache in highlight.rs keyed by the **content**
of the user syntax inputs:

- Key: SHA-256 over the ordered list of (path string, file bytes) for
  `with_user_files`, and for `with_user_dir` the ordered `.sublime-syntax`
  files discovered in the dir (same order the builder consumes). Content
  hashing makes the key exact — no mtime-granularity hazard; an edited
  grammar changes the key and relinks once.
- Store: `Mutex<Option<(Key, SyntaxSet)>>` — one entry. The watch loop
  rebuilds the same deck repeatedly, so one entry captures the win;
  replacement on key change keeps memory flat. `SyntaxSet` clones cheaply
  relative to `build()`.
- Miss path unchanged: build as today, store, return a clone. Errors
  (unreadable file, bad grammar) keep their exact current shapes and are
  never cached.
- `defaults()` stays on the existing OnceLock base-set cache; decks
  without user syntaxes are unaffected.

The repo already reads every user syntax file on each construction, so
hashing adds no new IO — with_user_dir's `add_from_folder` reads
internally, so the dir variant lists and reads the files itself for the
key and then loads via the same per-file loader `with_user_files` uses
(one read, one seam), rather than calling `add_from_folder` blind.

## Tests (TDD order)

1. Same inputs twice → second construction returns an equivalent set
   (spot-check token resolution) without rebuilding (assert via the cache
   key path — e.g. a counter behind `#[cfg(test)]`, or timing-free
   observable: mutate the file between constructions and assert the
   change IS picked up, while identical content hits the fast path — the
   correctness property is "content change always invalidates").
2. Editing a syntax file between constructions changes highlighting
   accordingly (no stale cache).
3. Error paths byte-identical: missing file / malformed grammar errors
   unchanged, and a failed build leaves the previous cache entry usable.
4. Existing user-syntax tests stay green.

## Non-goals

- No multi-entry LRU, no on-disk persistence.
- No change to the defaults() path.
