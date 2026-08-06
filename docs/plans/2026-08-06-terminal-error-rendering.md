# Terminal error rendering: peitho-owned cargo-style diagnostics

Issue: #414. Author decisions (2026-08-06): make the real terminal output at
least as readable as the hand-typeset README mock; fidelity to the mock is not
required when the result is more readable.

## Problem

`site/static/guide-shots/build-error.png` (README + Layouts guide) is a
hand-typeset mock (commit 9801088), and the real output never matched it:

- `Error:    × …` — the Rust runtime prints `Error: ` for a `main`-returned
  `Err`, then miette's graphical output adds its own `  × ` indent, leaving an
  awkward gap.
- miette styles both the `×` marker and every wrap-gutter `│` with the severity
  color, so a heavy red vertical bar runs down the whole block. This pairing is
  hardcoded in `GraphicalReportHandler::render_causes` (both indents are built
  from `severity_style`), so "dim gutter, red marker" is unreachable through
  theme configuration.
- Help gets no styling and no separation: `BuildError::Display` bakes
  `\n  = help: …` into the message, and `core_for_deck` flattens the whole
  error into `miette::miette!("{}")`, so the reporter sees one multi-line
  string. Long messages wrap inside the red gutter, chopping backtick lists
  mid-token.

## Root cause

`BuildError` carries structure (message, help, line, origin file, slide) to
the CLI boundary, where it is flattened into an unstructured string report and
handed to a third-party renderer whose styling peitho cannot control.

## Design

Own the terminal rendering at the existing single seam (`render_diagnostic`,
the unification point from #412), and keep the help structured across the CLI
boundary. Rendering style follows lint's existing house style
(`warning: …` / `  help: …`), i.e. cargo/rustc-shaped, with no wrap gutter:

```
error: deck.md:3: invalid deck frontmatter: unknown field `fontss`, expected
       one of `time`, `aspect_ratio`, `resolution`, `breaks`, `page_numbers`,
       `pointer_color`, `lang`, `layouts`, `css`, `syntaxes`, `fonts`,
       `code_images`
  help: use only the supported deck frontmatter keys: time, aspect_ratio,
        resolution, breaks, page_numbers, pointer_color, lang, layouts, css,
        syntaxes, fonts, code_images
```

Multi-line messages (e.g. the per-layout rejection list from "no layout
matches this slide") keep their embedded newlines, aligned under the message
start:

```
error: slide 2 ('whoami'), line 16: no layout matches this slide
       books: unassigned content remains for missing 'body' slot
       code: no slot accepts image in layout 'code'
  help: adjust the slide content or pick a layout explicitly with
        <!-- {"layout":"…"} -->
```

Styling: `error:` bold red, message unstyled, `help:` bold yellow, help body
unstyled. Colors use raw SGR codes like `doctor.rs::status_glyph` (no new
color dependency). Color is enabled only when stderr is a terminal, `NO_COLOR`
is unset or empty, and `TERM != dumb`. Width: terminal width on a tty
(clamped to a sane minimum, fallback 80); **no wrapping at all when stderr is
not a terminal** — piped output (CI logs, grep, assert_cmd) keeps each logical
line whole, like cargo and rustc. Wrapping uses `textwrap` with hanging
indents (7 spaces under `error: `, 8 under `  help: `); each embedded message
line wraps independently. Wrapping is greedy (`FirstFit`), splits only on
ASCII spaces, and never breaks inside a token — not even at hyphens
(`break_words(false)` + `NoHyphenation`): paths, URLs, CSS class names, and
cache hashes must stay contiguous so they can be copied and grepped, even when
that overflows the width. (Both decisions were forced by real E2E assertions
that grep stderr for full cache filenames and hyphenated selectors.)

### Mechanics

1. **peitho-core** (`error.rs`): extract `BuildError::headline()` — the
   location-prefixed message without the help tail. `Display` is refactored to
   `"{headline}\n  = help: {help}"` so its output is byte-identical (the
   preview error page and doctor keep their current text). Core gains no
   miette dependency.
2. **CLI** (`main.rs` or a new `diagnostics` module):
   - `DeckDiagnostic(BuildError)` implementing `Display` (= `headline()`),
     `std::error::Error`, and `miette::Diagnostic` with
     `help() = Some(help)` (empty help maps to `None` — doctor produces
     help-less BuildErrors). Both `core()` helpers and `core_for_deck` build
     `miette::Report::new(DeckDiagnostic(…))` instead of flattening. Existing
     typed diagnostics (`RemoteDefaultPortInUseError`) already expose `help()`
     and need no change.
   - Composition seams must not drop structured help now that `{err}`
     interpolation prints only the message. `diagnostics::report_help`
     is the accessor: `keep_workspace_for_error` appends its
     "workspace kept at …" note to the inner help,
     `append_chrome_stderr_log_write_failure` merges the two reports'
     distinct helps, and `CliEmbedRenderer` carries the report help into the
     `BuildError` it hands back to core (which already forwards renderer
     help into `embed_cache_error`).
   - `render_diagnostic(&miette::Report)` rewritten to the format above,
     reading the message from `Display` and the help from `Diagnostic::help()`
     (`Report` derefs to `dyn Diagnostic`). A pure inner function
     `render_diagnostic_with(message, help, colors, width)` keeps tests
     deterministic.
   - `main` no longer returns `miette::Result`: a thin `fn main` calls the
     current body (`run()`), prints failures through `render_diagnostic` to
     stderr, and exits 1. This removes the runtime's `Error: ` prefix (the gap)
     and makes propagated and swallowed error paths byte-identical.
   - Ad-hoc reports that bake `\nhelp: …` into the message
     (`keep_workspace_for_error`, the rehearsal record error) move to
     `miette!(help = …, "…")` so their help is structured too.
3. **Preview error page**: it needs plain text with help but without ANSI. Add
   a small helper that renders `"{message}\n  = help: {help}"` from the
   `Report` (matching today's page content) and use it at the
   `emit_preview_error_page` call site instead of `err.to_string()`.
4. **Watch/preview stderr**: `build failed: {render}` becomes
   `build failed:` on its own line followed by the rendered block. The prefix
   stays because in watch/preview it signals "previous good build still
   served", which a bare `error:` block would not.
5. **miette `fancy` feature**: dropped — nothing renders through the graphical
   handler anymore and no user-facing path formats a `Report` with `{:?}`
   (verified by grep). Direct `textwrap` (default-features off, unicode-width
   only) + `terminal_size` dependencies replace it.
6. **Ad-hoc `\nhelp:` embeds**: every `miette!` site that baked `help: …` into
   the message string (55 in main.rs plus cdp/lint/new_cmd/server) moved to
   the structured `miette!(help = …, "…")` form — same class of bug, one
   sweep. `\ncaused by:` / `\nstderr:` tails stay in the message; the CDP
   export help's "the Chrome stderr below" became "above" because help now
   renders after the message. Two sites are deliberately untouched because
   they are direct stdout/stderr notes, not reports (the watch-error note and
   the `peitho rehearsal` no-records message, same family as lint warnings).
7. **Docs**: `site/static/guide-shots/build-error.png` regenerated from the
   new real output (HTML mock typeset from a captured real run at 80 columns,
   screenshotted with headless Chrome at 2x, byte-matching the real output
   text and prefix styling).

## Amendment (2026-08-06, post-v1.22.0)

Author feedback after using v1.22.0: `error:` at column 0 with `  help: `
indented two spaces left the two labels visibly misaligned. The `help:` label
is now right-aligned under `error:` (` help: `, one leading space) so the
colons line up and both bodies plus all continuation lines share the same
7-column start. `peitho lint` gets the matching treatment (`warning: ` /
`   help: `, both bodies at column 9) — same visual class. Doctor's
`      help:` lines are untouched: they hang under a glyph-prefixed check list,
a different structure. Rendering output is pinned with insta snapshots
(`crates/peitho/src/snapshots/`, `cargo insta review` to accept intentional
format changes), and the guide's build-error.png was regenerated again.

## Tests

- Renderer unit tests (no-color, fixed width): headline wrapping with hanging
  indent, embedded newlines, help block, no help case, SGR placement in the
  color path, `NO_COLOR`/non-tty fallback via the pure inner function.
- `core_for_deck` produces a report whose `Diagnostic::help()` is populated
  and whose `Display` has no `= help:` tail.
- Update existing assertions: `render_diagnostic_formats_help_like_miettes_reporter`
  (new shape), watch/preview `build failed:` tests, preview error page test
  (help still present in HTML, still no ANSI).
- `BuildError::Display` byte-identical before/after the `headline()` refactor
  (existing core tests already pin this).
- Manual E2E: run the real binary on a broken deck in a real terminal (wide
  and narrow), verify build, `build --watch`, and `preview` outputs.

## Out of scope

- Coloring `peitho lint` warnings (house style already matches; styling can
  follow up).
- miette source-snippet rendering (would need spans, not just line numbers) —
  possible future readability upgrade, noted in #414.
