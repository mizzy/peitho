# Theme `@import` prelude stays first in `peitho.css` (Issue #667)

## Problem

`render_deck` assembles `peitho.css` as `[math, embed card, generic card,
static emphasis]... + theme`. A theme that starts with `@import` (web fonts)
loses those imports whenever any built-in stylesheet is prepended: per CSS
Cascade, `@import` after any other rule is ignored, silently.

## Root cause and seam

The theme reaches the renderer as an opaque `String`, so the only place that
knows where built-in CSS may go has no idea where the theme's statement
prelude ends. All four built-in stylesheets break the same way; fixing each
site would repeat the bug for the next built-in stylesheet.

## Design

- `theme.rs` gains `ThemeCss`: the concatenated theme CSS plus the byte
  offset where its leading prelude ends. The offset is private and computed
  only by scanning the content (`ThemeCss::new(css)`), so a wrong split is
  unrepresentable. `build_theme_css` returns `ThemeCss`.
- Prelude = the longest prefix made of whitespace, comments, and the
  statements the CSS grammar allows before ordinary rules: `@charset`,
  `@import`, and `@layer` statements (`@layer a, b;`, not blocks). At-keywords
  are ASCII case-insensitive; trivia whitespace is CSS whitespace only
  (space, tab, LF, CR, FF). A statement ends at the first `;` outside
  strings, parentheses, comments, and unquoted `url(...)` tokens (Google Fonts
  URLs contain `;`; an unquoted URL may contain `/*`). The
  first anything-else (a rule, `@layer x {`, `@font-face`, unterminated
  statement) ends the prelude.
- `render_deck` takes `ThemeCss` and assembles `prelude + built-ins + rest`.
  Built-in CSS still precedes every theme rule, so "the theme overrides
  built-in defaults" holds. With no built-ins, or an empty prelude, the output
  is byte-identical to today.
- `build_theme_css` strips one leading UTF-8 BOM from every CSS file before
  validation and concatenation. Rust's `trim()` keeps U+FEFF, so a BOM-saved
  `00-fonts.css` hid its `@import` from the scanner, and a BOM in any
  non-first file already landed mid-stylesheet. This is the one intended
  byte change for decks without built-in CSS.
- Out of scope: `@import` that follows a rule inside the theme itself (already
  invalid regardless of peitho), and `@namespace`.

## Tests

- Scanner units: `@import` with `;` inside a quoted and an unquoted `url()`,
  comments/whitespace between statements, `@charset`, `@layer` statement vs
  block, uppercase `@IMPORT`, no prelude, prelude-only theme, prelude split
  across concatenated files (`00-fonts.css` + `base.css`).
- Render: every built-in stylesheet lands after the prelude and before the
  first theme rule; no-built-in decks unchanged.
- CLI build test reproducing the issue: `css/00-fonts.css` with an `@import`,
  a static emphasis block, `dist/peitho.css` starts with the `@import`.
