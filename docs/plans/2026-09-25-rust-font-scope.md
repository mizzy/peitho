# Issue 670: Make Rust the font-scope CSS authority

## Problem

Present and preview mount slide CSS in shadow roots, so they also need the
theme's leading `@import` rules and rendered CSS's top-level `@font-face`
blocks at document scope. The shell currently derives that CSS by scanning
`peitho.css` in TypeScript, duplicating the Rust theme-prelude grammar and
disagreeing with it for semicolons and comment markers inside unquoted
`url(...)` tokens and for statement-form `@layer` rules.

## Author decision (2026-09-25)

Rust is the only authority for font-scope CSS. `Deck<Rendered>` computes it
once when the rendered phase is constructed. Present and preview receive the
result as a cache-only `fontscope.css`; the TypeScript shell installs the given
text and contains no CSS scanner.

## Implementation

1. Red: port the TypeScript extraction cases to Rust and add regressions for
   the two Issue 670 failures, charset/layer handling, unquoted URL comment
   markers, top-level-only font faces, and `font-display` replacement.
2. Green: factor the existing `ThemeCss` leading-prelude scanner to expose
   statement ranges and keywords, then derive font-scope CSS from:
   - only `@import` statements in that shared prelude;
   - top-level `@font-face` blocks in the complete rendered CSS;
   - a Rust-only present/preview rewrite that removes authored
     `font-display` declarations (comments around the property name
     included) and appends `font-display:block;`, inserting a `;` first when
     the last declaration is unterminated (the embedded KaTeX CSS is minified
     and every face ends without one).
   The prelude and font-scope scans share one comment/string/unquoted-`url()` walker
   (`CssCodeChars`). Strings follow CSS Syntax: an escaped newline, or the one
   whitespace after a hex escape, continues the string; an unescaped newline
   ends it. `CssStringState` is shared with the size-declaration validator,
   which gets the same, spec-correct string rules. Known limit (unchanged from
   the old TypeScript scanner): a backslash escape outside a string, such as
   `.a\{`, is not recognized. A malformed `@font-face;` is skipped
   and scanning resumes after it; a `}` in an `@font-face` prelude stays part
   of the prelude, as it does in Chrome.
3. Store the privately constructed result on `Deck<Rendered>` and expose it as
   `font_scope_css(&self) -> &str`.
4. Red/green CLI coverage: always write `fontscope.css` to the present cache
   and every preview generation, including an empty file, while keeping it out
   of `dist/` and the PDF workspace. Add it to the publish contamination list.
5. Red/green shell coverage: reduce `fontscope.ts` to installation,
   reference-counting, and cleanup for already-computed text. Fetch
   `fontscope.css` beside `peitho.css` in present and preview, and update every
   affected fetch mock.
6. Rebuild the committed presentation bundles.

## Documentation

Move the existing `font-display:block` rationale to the Rust implementation.
Add a `CLAUDE.md` invariant bullet recording that Rust is the single source of
font-scope CSS and the shell never scans CSS, so a later change cannot quietly
reintroduce a second scanner.

## Gates

- `cargo test --workspace` three consecutive times
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/`
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- Verify the four committed bundles are rebuilt and run `git diff --check`
- Do not run ignored Chrome E2E tests in the sandbox
