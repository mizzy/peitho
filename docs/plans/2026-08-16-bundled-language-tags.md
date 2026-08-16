# Bundled syntax set for common language tags (Issue #427)

Date: 2026-08-16
Issue: #427 — typescript/ts, toml, dockerfile, text are unknown out of the box

## Problem

The highlighter builds every `SyntaxSet` from
`SyntaxSet::load_defaults_newlines()` (`crates/peitho-core/src/highlight.rs`),
and syntect's default set lacks TypeScript, TOML, and Dockerfile. `text`
fails for a different reason: the default "Plain Text" syntax matches only
the token `txt` (its extension) — the most natural "no highlighting" tag
errors while `txt` works.

## Decision

Replace the base set with the `two-face` crate's extended registry (bat's
syntax collection, 213 syntaxes, fancy-regex-compatible dump) and add a
plain-text alias seam. Measured before deciding (2026-08-16):

- `two_face::syntax::extra_newlines()` with `syntect-fancy` resolves
  `typescript`, `ts`, `tsx`, `toml`, `dockerfile` (and keeps `rust`, `js`,
  …); `text` still misses, `crn` still needs user syntaxes as designed.
- Dump payload is ~3.8MB in the crate (both engines + themes); the binary
  grows by roughly the fancy syntax dump only. The project already accepted
  +12MB for built-in mermaid.

Bundling just three vendored `.sublime-syntax` files was rejected: it fixes
this blog post's five tags, not the class (the next deck hits kotlin or
zig). All three project lenses (long-term, root-cause at the single base-set
seam, no per-tag carve-outs) select the extended registry.

## Changes

1. Workspace dependency: `two-face = { version = "0.5", default-features =
   false, features = ["syntect-fancy"] }` (must NOT pull onig — peitho's
   syntect is `default-fancy`, pure Rust).
2. `highlight.rs`: extract one `base_syntax_set()` used by `defaults()`,
   `with_user_dir()`, `with_user_files()` — returns
   `two_face::syntax::extra_newlines()`. User syntaxes keep building on top
   via `into_builder()`.
3. Plain-text alias as DATA, not control flow (revised 2026-08-16 after
   review): `base_syntax_set()` appends a Plain Text syntax definition whose
   `file_extensions` are `text` and `plaintext` (scope `text.plain`, no
   rules), so plain `find_syntax_by_token` resolves them. This keeps every
   token on one uniform path: case-insensitive like all other tokens, and
   user syntaxes (added after the base set; syntect resolves `.rev()`, later
   wins) can shadow `text`/`plaintext` exactly like any other tag. A
   lookup-site `matches!(token, "text" | ...)` carve-out was rejected — it
   was case-sensitive and silently shadowed user syntaxes for those two
   tokens only.
3b. The deserialized base set is cached in a process-wide `OnceLock` —
   two-face's `extra_newlines()` re-parses a ~1MB dump on every call
   (measured 0.7ms release / 31ms debug), and the workspace constructs
   hundreds of `Highlighter`s across the test suite.
3c. `two-face` is pinned `=0.5.2` (katex-rs precedent): the syntax
   collection rides build metadata, so a semver-compatible bump could swap
   the whole token surface silently.
4. Help text: `example_tokens`'s PREFERRED list gains `ts` (it filters
   through the loaded set, so it appears only because it now resolves —
   the #105 drift guard keeps holding).
5. Invert the regression test that asserts `ts` is NOT recognized
   (highlight.rs:311-319 guarded the old help-text drift; the new reality is
   `ts` resolves).

## Tests (TDD order)

1. `typescript`, `ts`, `toml`, `dockerfile` fences validate and highlight
   (spans present) — fails before the swap.
2. `text` and `plaintext` fences validate and render like `txt` does.
3. Unknown tag (e.g. `crn`) still errors with line number and the derived
   help list; help list contains `ts` and `toml`.
4. User `syntaxes/` dir still augments the (new) base set.
5. Existing highlight tests: update any snapshot/exact-class assertions that
   legitimately change with the newer syntax definitions; do not weaken
   semantic assertions.

## Non-goals

- No re-vendoring of individual syntaxes.
- No change to the "unknown tag is a build error" contract.
- No docs-history rewrites (the 2026-07-12 plan's "toml absent (measured)"
  stays as a historical record).
