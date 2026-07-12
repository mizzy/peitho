# Build-time lint: same-specificity width/height overrides on layout root classes

Issue #260, follow-up to Issue #256 / PR #259.

## Context

The renderer puts `.peitho-slide` on each slide's root `<section>`, and the
theme CSS sizes that element to the canvas
(`width: var(--peitho-canvas-width, 1280px); height: var(--peitho-canvas-height, 720px)`).
Layout HTML adds its own classes to the same `<section>`
(`<section class="code-images code-images-diagram">`). PR #259 fixed two
example decks whose CSS declared `width: 100%; height: 100%` on such root
classes at equal specificity — source order let them beat `.peitho-slide`,
the slide sized to its parent instead of the canvas, and PDF export
(`transform: scale(...)` inside a page-sized wrap) overflowed the page.
The HTML shell coincidentally sized its container to the canvas var, so the
bug was invisible there.

PR #259 removed the collisions but nothing prevents re-introduction. This
plan adds the build-time lint proposed in Issue #260 so the collision is a
line-numbered build error.

## Corrections to the issue text

The issue says the lint should extend `check.rs` "which already validates
deck CSS selectors against slot names". That validation actually lives in
`crates/peitho-core/src/theme.rs` (`build_theme_css` →
`validate_override_selectors`); `check.rs` only validates slot contracts.
The issue's intent is "the CSS validation pass alongside the existing
selector checks", so the lint lives in `theme.rs`.

## Design decisions (three-lens check)

- **Hard error, no opt-out.** All peitho diagnostics are line-numbered
  build errors; a warning or opt-out flag would be a convention-weakening
  escape hatch. The legitimate way to size the slide root is the
  `.peitho-slide` rule itself, and a root-class declaration whose value is
  *identical* to `.peitho-slide`'s is allowed (harmless duplication), so no
  separate opt-out mechanism is needed. This resolves the issue's "false
  positives" design question.
- **Root-cause seam.** The lint fires once, inside `build_theme_css`, which
  every build path already funnels through (single call site in
  `crates/peitho/src/main.rs`). No per-consumer checks.
- **Precise scope.** Only classes that appear on the layout's root
  `<section>` are linted. `parse_layout` already enforces exactly one
  `<section>` per layout, so "root section classes" is well-defined and a
  `width: 100%` deeper in the DOM stays legal (that is what `%` is for).

## Lint semantics

1. **Root classes**: for each provided layout, the classes on its single
   `<section>` element. Union across layouts, minus `peitho-slide` itself
   (the renderer adds it; a layout may also carry it explicitly — it is the
   reference, never a violation).
2. **Reference values**: the effective `width` / `height` declared by
   `.peitho-slide` rules across the composed CSS file stack, in load order,
   last declaration wins (cascade order). A reference selector member is
   either bare `.peitho-slide` or a combinator-free, pseudo-free compound
   containing the `.peitho-slide` class token (`section.peitho-slide`);
   members with combinators (`.wrap .peitho-slide`), pseudo-classes or
   pseudo-elements (`.peitho-slide::after` sizes a different box and must
   not poison the reference), and `@`-scoped rules are not references.
   Values compare after removing all whitespace (case-sensitive, so
   `var()` custom-property names stay exact).
3. **Violation**: a rule whose selector (any comma-separated member) is
   exactly `.X` where `X` is a root class, containing a direct `width` or
   `height` declaration whose normalized value differs from the reference
   value for that property. If `.peitho-slide` declares no reference value
   for the property anywhere in the stack, any such declaration is a
   violation (there is nothing to legally duplicate).
4. **Error**: `ErrorKind::Theme`, line number of the offending declaration
   (the line where the property token starts), file name prefixed by the
   existing `build_theme_css` wrapper. Message names the class, the
   property, and that the class sits on the slide root `<section>`. Help is
   context-sensitive: when a reference exists it quotes the actual
   reference value to match; when none exists it says to size the slide
   root on `.peitho-slide` instead (quoting the canvas var pattern).

### Scanner requirements (post-review hardening)

The scanner is a char-level state machine over comment-stripped text, not a
per-line matcher, because three confirmed defect classes follow from naive
tokenization:

- **String-aware everywhere**: braces/semicolons inside quoted strings
  (`content: "}"`, `url("a}b.png")`) are literal text in selector,
  declaration, and skipped-block contexts. Comment stripping is also
  string-aware (`content: "/*"` must not blank the rest of the file and
  silently disable validation), with quote state resetting at each
  newline so an unterminated quote cannot suppress stripping for the
  remainder of the file (CSS strings cannot contain raw newlines).
  Braces inside an unquoted function token (`url(a{b.png)` — legal per
  CSS url-token grammar) are likewise literal, tracked by paren depth,
  so they cannot desynchronize the scan.
- **Nesting-aware**: a nested block inside a style rule (native CSS nesting
  such as `&:hover { … }`) is skipped without terminating the outer rule —
  otherwise a later `width:` in the outer rule is silently missed, the
  exact invariant violation the lint exists to prevent. Nested-block
  interiors themselves are not linted (non-goal).
- **Cross-line declarations**: declaration text accumulates until `;` or
  the rule's closing `}`, so formatter-wrapped values
  (`width:\n  var(…);`) compare by their full value, reported at the
  property token's line.
- Top-level `;` ends a statement at-rule (`@import`, `@charset`) and
  discards the selector buffer, so the following rule is scanned normally.

### Explicit non-goals (from the issue)

- Not a general-purpose CSS linter. Compound selectors as violation
  targets (`section.code-images`, `.a.b`), keyed selectors, `@media`-scoped
  rules, and `min-/max-width` are out of scope; the lint only catches the
  same-specificity bare-class collision that caused #256. The scanner must
  still not crash, mis-report line numbers, or let such input
  desynchronize the scan — it just does not flag it.
- `!important` is deliberately NOT out of scope: a root-class declaration
  whose value differs from the reference only by a trailing `!important`
  is still a violation — an `!important` duplicate outranks the cascade
  the invariant relies on, and removing the `!important` satisfies the
  help text (decision 2026-07-12, review round 3).
- Not a runtime fix.

## Implementation tasks (TDD, red → green each)

### Task 1: `Layout` exposes root section classes

`crates/peitho-core/src/layout.rs`

- `parse_layout` records the `class` attribute of the (single) `<section>`
  into a `BTreeSet<String>`, stored on `Layout`.
- `Layout::root_classes() -> &BTreeSet<String>`;
  `Layouts::root_classes() -> BTreeSet<String>` union across layouts.
- Tests: multi-class extraction, no `class` attribute → empty set,
  whitespace splitting, union across layouts.

### Task 2: lint in `build_theme_css`

`crates/peitho-core/src/theme.rs`

- New parameter `root_classes: &BTreeSet<String>` on `build_theme_css`
  (breaking signature change; single production call site).
- Pass A over all files (comments stripped once per file, same input as
  the existing validation): collect reference `width`/`height` from
  `.peitho-slide` rules per the reference-member definition in "Lint
  semantics" (bare or combinator-free pseudo-free compound), last wins.
- Pass B per file: the char-level scanner from "Scanner requirements"
  flags `width`/`height` declarations directly inside a rule whose
  selector has a comma member exactly `.X` with `X` in
  `root_classes − {peitho-slide}`, when the normalized value differs from
  the reference.
- Tests: flag `width` after `.peitho-slide` (source-order win), flag
  `height` before it (order-independent), identical value allowed,
  var-pattern value allowed when it matches the reference, non-root class
  untouched, `peitho-slide` itself never flagged, reference in file 1 /
  violation in file 2, last-wins reference, no `.peitho-slide` declaration
  → still flagged, selector lists, comment stripping keeps line numbers,
  error kind/line/help assertions.

### Task 3: wire the call site

`crates/peitho/src/main.rs` (`build_theme_css` call, ~line 1406): pass
`layouts.root_classes()`. Integration test in `crates/peitho/tests/build.rs`
building a deck whose CSS re-introduces `width: 100%` on a layout root
class → line-numbered build error; and the existing example decks still
build (they were fixed in PR #259 and must stay lint-clean).

One example needed migration in this PR: `image-showcase` sized the slide
root via `.image-showcase` with no `.peitho-slide` rule anywhere in its
stack — the pattern PR #259's sweep missed and the no-reference rule
correctly flags. Its canvas sizing moved to a `.peitho-slide` rule,
matching every other example.

The lint-clean canary enumerates `examples/` via `read_dir` rather than a
hardcoded name list. An example is linted when it has deck CSS (a `css/`
directory, or a `css:` frontmatter key — which must point at `./css`);
its layout set is the deck-adjacent `layouts/` directory when present,
otherwise the built-in default layout (parsed from the same
`layouts/title-body-code.html` the binary embeds, so the test stays in
lockstep with the embed source — `custom-fonts` is this shape: `css/` +
`fonts/`, built-in layout). Examples with no deck CSS are skipped
(nothing to lint). Genuinely divergent configurations (asset keys
pointing outside the convention) fail loudly, telling the author to
extend the canary. It deliberately does not run `peitho build` per
example: a real build of `code-images` requires graphviz/mermaid
binaries, which this test must not depend on.

### Task 4: docs

Add the lint to the CSS validation bullet in `CLAUDE.md`'s frontmatter/assets
paragraph is NOT needed (CLAUDE.md is maintained by the author); instead the
design record is this plan file. No user-facing docs exist for CSS
validation errors beyond error text itself.

## Gates

```
cargo test --workspace          # 3 consecutive runs
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
```

Plus a real `peitho build` / `peitho export pdf` of `examples/code-images`
with a deliberately re-broken CSS to see the error end-to-end, then reverted.
