# User-supplied syntaxes for code highlighting (Issue #116)

## Goal

Allow decks to highlight code blocks in languages that `syntect`'s default set doesn't cover, by loading user-supplied `*.sublime-syntax` files from a `syntaxes/` directory sitting next to the deck (zero-config convention, same shape as `layouts/` and `css/`).

Concretely: writing a deck with a `carina` fenced code block plus a `syntaxes/carina.sublime-syntax` next to it must build successfully and produce spans with `hl-*` classes; unknown language tags remain a build error.

## The three lenses

Author confirmed **Convention のみ (`syntaxes/` auto-detect)** as the discovery method. Under the three lenses:

1. **Long-term** — any DSL (Carina `.crn`, plus a future TypeScript / any language a repo wants to ship its own definition for) gains highlighting without peitho having to know about it. Also resolves Issue #105 (unknown-language help suggests `ts` but syntect defaults don't have it) as a side effect: with a user-supplied TS syntax present, the tag validates; without it, the help text stays honest.
2. **Type safety** — a `Highlighter` newtype owns the `SyntaxSet`. `validate_language` and `highlight_html` become methods on it, so it is *impossible* to look up a language token against an unconfigured default set from a caller that has one. The old `syntax_set()` global (`OnceLock<SyntaxSet>`) is deleted — there is no surviving path that skips user syntaxes.
3. **Root cause** — the reason the current code can't accept a `.crn` block is not that "Carina is missing" (that's the symptom); it's that the highlighter's syntax set is baked in at process init with no injection point. The one upstream fix is to thread a `Highlighter` through the pipeline. Alternative bandaids (bundling HCL, per-language allowlists) leave the injection gap and only cover today's known-missing languages.

All three lenses agree, so the choice is not a judgement call.

## Design

### New type: `peitho_core::highlight::Highlighter`

```rust
pub struct Highlighter {
    syntax_set: SyntaxSet,
}

impl Highlighter {
    /// syntect's built-in default set only.
    pub fn defaults() -> Self { ... }

    /// Default set + user-supplied `*.sublime-syntax` files from `dir`.
    /// A `dir` whose walk yields no `.sublime-syntax` files is accepted
    /// (an empty `syntaxes/` directory is not an error — same policy as
    /// an empty `css/` directory being disallowed only because we require
    /// at least one CSS file to concatenate; here there is no analogous
    /// requirement because the defaults cover most languages).
    /// A file that fails to parse (syntect returns `LoadingError`) is a
    /// line-numbered `BuildError` with the file path — no silent drop.
    pub fn with_user_dir(dir: &Path) -> Result<Self> { ... }

    pub fn validate_language(&self, token: &str, line: usize) -> Result<()> { ... }
    pub(crate) fn highlight_html(&self, code: &str, token: &str, line: usize) -> Result<String> { ... }
}
```

- The old free functions `highlight::validate_language` and `highlight::highlight_html`, plus the `syntax_set()` `OnceLock`, are **deleted**. The typestate + newtype together enforce that every highlighter lookup went through a caller-owned `Highlighter`.
- `Default` for `Highlighter` = `Self::defaults()`, so tests that don't care about user syntaxes are one line.

### Threading through the pipeline

`parse_markdown` and `render_deck` are the two call sites. Both need to see the same `Highlighter` (otherwise a language accepted at parse time could fail at render, or vice versa — a silent path).

Add a `&Highlighter` argument to both:

```rust
pub fn parse_markdown(source: &str, highlighter: &Highlighter) -> Result<Deck<Parsed>>;
pub fn render_deck(deck: Deck<Checked<...>>, highlighter: &Highlighter) -> Result<Deck<Rendered>>;
```

This is a wide diff (tests reach into these functions), but it makes the injection point explicit at the type level and removes the global. **Do not add a `parse_markdown_with_defaults` wrapper** — that recreates the "forgot to pass the syntaxes" bug at the API surface. Tests get `&Highlighter::defaults()` inline.

### CLI wiring

In `crates/peitho/src/main.rs`:

- New helper analogous to `effective_layouts` / `effective_css`:
  ```rust
  fn effective_syntaxes(&self) -> Option<PathBuf> {
      effective_asset_path(&None, &self.input, "syntaxes")
      // no CLI flag field — convention-only per author decision
  }
  ```
  `effective_asset_path` already handles "return `None` when `syntaxes/` doesn't exist next to the deck". No new CLI flag is added.
- `build_artifacts` builds a `Highlighter` at the top:
  ```rust
  let highlighter = match syntaxes_path {
      Some(dir) => Highlighter::with_user_dir(&dir).map_err(...)?,
      None => Highlighter::defaults(),
  };
  ```
  and passes `&highlighter` to `parse_markdown` and `render_deck`.
- `BuildOptions::watch_roots` gains a `(syntaxes_dir, "sublime-syntax")` entry so `--watch` rebuilds when a syntax file changes.
- Note: `PresentOptions` also calls `build_artifacts`; convention discovery is deck-relative, so it works uniformly. No `PresentOptions` field is needed.

### Errors

- `syntaxes/foo.sublime-syntax` that syntect can't parse → `BuildError::Parse` with the file path in the message and a "check the sublime-syntax file" help line. No line number is available from syntect, so the error carries the file path only.
- Unknown language tag with user syntaxes loaded → same `unknown code language 'X'` as today, unchanged help text (still points at defaults). **Issue #105 is a *sibling* problem to this one**: it complains that the help says `ts` when defaults don't have TS. Fixing #105 in this PR would be scope creep; keep the help text change out and leave #105 to its own fix. This PR does, however, give #105 a workable answer for users today ("drop a TS sublime-syntax in `syntaxes/`").

### Docs

- `README.md` gets a short "Custom syntaxes" section: drop `*.sublime-syntax` files into `syntaxes/` next to the deck; they augment (not replace) syntect's defaults; unknown tags stay build errors.
- `CLAUDE.md` gets a one-line update in the highlighting bullet: "Custom syntaxes: `syntaxes/` next to the deck, same convention as `layouts/`/`css/`; augments the built-in set."

### What is NOT in scope

- No CLI flag (`--syntaxes`) — author chose convention-only.
- No frontmatter key (belongs to Issue #62's migration).
- No bundled HCL / TypeScript syntax (Option 2 was ruled out by the lenses).
- No Issue #105 help-text fix — file a follow-up if needed after landing.
- No theme changes — the existing `hl-*` class prefix stays.

## Tests (TDD, Red-Green-Refactor)

Red tests to write first, one per behavior:

1. `Highlighter::defaults()` validates known tokens (rust, js) — sanity port of existing tests.
2. `Highlighter::with_user_dir(<dir with carina.sublime-syntax>)` validates a `carina` token; `Highlighter::defaults()` rejects it. **This is the headline test.**
3. `Highlighter::with_user_dir(<dir with malformed .sublime-syntax>)` returns a `BuildError::Parse` mentioning the file path.
4. `Highlighter::with_user_dir(<empty dir>)` = `Highlighter::defaults()` behavior.
5. `Highlighter::with_user_dir(<nonexistent dir>)` — call site guarantees existence (`effective_asset_path` filters), so this is unreachable in the CLI; core-level API returns an error rather than silently succeeding.
6. `Highlighter::with_user_dir` also validates *existing* defaults still work (no regression on `rust`/`js`).
7. `highlight_html` on a user-supplied syntax produces `hl-*` classes.
8. CLI integration: `build_artifacts` with a `syntaxes/` dir next to the deck picks it up automatically (mirrors `layouts/`/`css/` zero-config tests).
9. CLI `--watch` rebuild on `.sublime-syntax` change (mirror the existing layout/css watch tests).

Existing tests that call `parse_markdown(src)` or `render_deck(deck)` all need `&Highlighter::defaults()` — this is a mechanical widening, no behavior change.

## Verify

Full gate set from CLAUDE.md:
```
cargo test --workspace   # x3
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
```

Plus an end-to-end smoke: build a real deck with a `carina` block and a `syntaxes/carina.sublime-syntax` and confirm the rendered HTML has `hl-` spans. `.sublime-syntax` sourcing: use a minimal hand-written definition (matches `carina`, `crn` file extension, one `keyword` scope) — small enough to inline in the test as a string.

## Delegation

Codex writes the code following this plan and the Red-Green-Refactor discipline. Opus (me) reviews each pass against the design above and against the essence: does the diff *make the broken state unrepresentable* (the `Highlighter` newtype replaces the global) or does it still leave a "default-only" path lying around?
