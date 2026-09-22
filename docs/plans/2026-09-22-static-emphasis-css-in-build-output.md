# Static code line emphasis CSS in build output (Issue #564)

## Problem

Static line emphasis (```` ```rust {2-4} ````) emits `code-line` /
`code-line-emphasis` markup into every output, but the rules that style that
markup live in two places that do not cover every case:

- `themes/base.css:220-236` — not loaded at all when the deck ships its own
  `css/` (a deck-supplied stylesheet replaces the built-in theme wholesale),
  and scoped to `.slot-code`, which misses a code fragment routed to a
  differently-named slot via `::: {slot=snippet}`.
- `EMPHASIS_ACTIVE_CSS` in `packages/peitho-present/src/shell.ts:147-157` —
  injected into each slide's shadow root by the **present** shell only.

So the styling exists in present and nowhere else for those decks:

| surface | built-in theme, `code` slot | deck ships `css/`, or non-`code` slot |
|---|---|---|
| present | styled | styled (shell injection) |
| preview | styled | **unstyled** |
| dist / PDF / lint | styled | **unstyled** |

Measured on this branch before the fix: a deck with `css/own.css` and a
`::: {slot=snippet}` code fence builds markup containing
`code-line-emphasis` while the emitted `peitho.css` contains **zero**
`.code-line` rules.

The root cause is a single broken invariant: **static emphasis markup is
emitted without any guarantee that its styling ships with it.** The markup
is produced by the renderer; the styling is left to a theme file the deck may
never load. Every unstyled surface is a symptom of that one gap.

## Decision — Option 1 from the issue

The issue lists three options. All three lenses select Option 1, so it is
taken without a separate author decision (noted in the PR):

1. **Long-term** — all five outputs (build/dist, preview cache, present
   cache, PDF workspace, lint workspace) already funnel their CSS through
   one function, `write_shared_assets` (main.rs:2411), fed by
   `RenderedDeck::css()`. Fixing the seam that produces `css()` covers every
   current surface and every future one. Option 2 fixes preview only and
   leaves dist/PDF broken; Option 3 makes every custom-CSS deck author
   re-implement the rules.
2. **Type safety** — the condition is derived from the same
   `FragmentKind::Code` + `SourceFragment::emphasis()` data that emits the
   markup, in the same render loop as the three existing detectors. Markup
   and styling cannot diverge, because one loop decides both. Options 2/3
   rely on a human remembering.
3. **Root cause** — Option 1 restores the invariant at the emitter. Option 2
   is a guard at one consumer site, with the sibling consumers left broken.

## Design

Follow the existing embedded-CSS prepend mechanism exactly (KaTeX CSS and
embed-card CSS), at `crates/peitho-core/src/render.rs:93-142`.

### New asset

`crates/peitho-core/assets/code-emphasis.css` — a new `include_str!`ed file,
in the shape of `assets/embed-card.css` (pure CSS: no `url()`, no
`@font-face`, so no companion asset writing).

Its rules are the **static** half of the shell's `EMPHASIS_ACTIVE_CSS`, using
the shell's scoping (`pre:has(...)`, bare `.code-line`) rather than
base.css's `.slot-code` prefix — that is what closes the non-`code`-slot hole:

```css
.code-line {
  display: inline-block;
  width: 100%;
}

.code-line-emphasis {
  background: var(--peitho-emphasis-background, rgba(217, 163, 0, 0.18));
  box-shadow: inset 3px 0 0 var(--peitho-emphasis-marker, #d9a300);
}

pre:has(.code-line-emphasis) .code-line:not(.code-line-emphasis) {
  opacity: var(--peitho-emphasis-dim, 0.45);
}
```

Only `--peitho-emphasis-*` fallbacks, and the block is prepended *before*
the theme CSS, so an author's own `.code-line*` rules still win on equal
specificity — same contract as the card CSS.

The stepped half (`[data-emphasis-active]`) stays in the shell only: stepped
emphasis is present-only by design and must not appear in build output.

### Detection

A fourth detector beside `slide_uses_math` / `slide_uses_embed_card` /
`slide_uses_generic_embed_card` (render.rs:146-171). Unlike those three it
reads `fragment.emphasis()` rather than only `kind()`, because emphasis is a
field on `SourceFragment`, not a `FragmentKind` payload:

```rust
fn slide_uses_static_emphasis(
    slots: &BTreeMap<SlotName, CheckedSlot<ResolvedImagePath>>,
) -> bool {
    slots.values().any(|slot| {
        slot.fragments().iter().any(|fragment| {
            fragment
                .emphasis()
                .is_some_and(|emphasis| !emphasis.stepped())
        })
    })
}
```

Folded in the existing render loop (`uses_static_emphasis |= ...`) and
pushed into `css_parts` in a stable position. Order: katex → base card →
generic card → **code emphasis** → theme last. Theme stays last.

### Scope boundaries

- `themes/base.css:220-236` — the `.slot-code`-scoped static rules are
  **removed**, because the prepended block now supplies them for every deck
  including the built-in-theme one. Leaving both would double the rules for
  built-in-theme decks and keep the under-scoped selector alive.
- `EMPHASIS_ACTIVE_CSS` in shell.ts — the static half is **removed**,
  leaving only the `[data-emphasis-active]` stepped rules, since the build
  output now always carries the static rules. (The issue notes this: "the
  static half of `EMPHASIS_ACTIVE_CSS` could then go.") The `.code-line`
  `display:inline-block` rule stays in the shell, as stepped emphasis needs
  it and a stepped-only deck gets no prepended block.
- The old theme's `.slot-code .code-line { display: inline-block; width: 100%; }`
  covered static and stepped markup. `slide_uses_static_emphasis` leaves stepped-only dist/PDF/lint/preview CSS
  without it (present still gets the shell rule); measured invisible today because `pre-wrap` preserves geometry and
  no build output paints `data-emphasis-step`. Author decision (2026-09-22): keep `!emphasis.stepped()`, not `.is_some()`;
  revisit if stepped markup is styled statically, such as a print fallback that renders all steps.
- `preview.ts` — untouched. It consumes the deck CSS, which now carries the
  rules.
- Decks with no static emphasis keep byte-identical slide HTML; no emphasis CSS
  is prepended, so for fixed `theme_css`, `RenderedDeck::css()` equals it.
  Built-in-theme `peitho.css` intentionally shrinks by the removed
  `.slot-code .code-line*` rules in `themes/base.css`; this is not a regression.

## Tasks (TDD)

1. **Red** — add `rendered_deck_prepends_emphasis_css_before_theme_only_when_used`
   in `crates/peitho-core/src/render.rs` tests, mirroring
   `rendered_deck_prepends_card_css_before_theme_only_when_used`
   (render.rs:4432): a static-emphasis deck's `css()` equals
   `{emphasis_css}\n{theme_css}`; a stepped-only deck and a no-emphasis deck
   equal `theme_css` exactly. Assert the rules appear exactly once, and that
   the asset contains no `url(` / `@font-face`.
2. **Green** — add `assets/code-emphasis.css`, the accessor, the
   `slide_uses_static_emphasis` detector, and the `css_parts` push.
3. Extend the existing byte-exact order tests that a new part breaks:
   `math_x_and_generic_css_order_keeps_theme_last` (render.rs:4731) and
   `decks_without_generic_cards_keep_existing_css_bytes` (render.rs:4749).
4. Add a test proving the non-`code`-slot case is styled — a checked deck
   whose code fragment sits in a slot **not** named `code`, asserting the
   emitted CSS carries rules that match it (no `.slot-code` prefix).
5. Remove the static rules from `themes/base.css`, keeping the comment
   accurate about where static emphasis is now styled.
6. Trim `EMPHASIS_ACTIVE_CSS` in `packages/peitho-present/src/shell.ts` to
   the stepped-only rules; update its doc comment; rebuild the bundles
   (`npm run build`) so `dist/shell.js` drift checks pass.
7. CLI-level test in `crates/peitho/src/main.rs` beside
   `build_artifacts_prepends_katex_css_only_for_math_decks` (main.rs:6271):
   a static-emphasis deck's emitted `peitho.css` carries the rules before
   the theme.
8. Update `docs/specs/2026-08-01-code-line-emphasis-design.md` and
   `CLAUDE.md`'s code-line-emphasis bullet, which currently say the static
   form is styled by the theme.

## Verification

- `cargo test --workspace` ×3, clippy, fmt, `git diff --exit-code bindings/`
- `npm run build && npm test && npm run typecheck`, plus the three
  `dist/*.js` drift checks
- Repo CI scripts: `grep -rE "run: bash" .github/workflows/`
- E2E in a real browser: the custom-`css/` + `::: {slot=snippet}` deck used
  to reproduce the bug, checked in `peitho preview`, `peitho build` output,
  and `peitho export pdf` — emphasis must now render in all three, and
  stepped emphasis must still work in `peitho present`.
- No-emphasis regression check: slide HTML is byte-identical, and
  `RenderedDeck::css()` equals the same `theme_css` with no emphasis CSS
  prepended. Built-in-theme `peitho.css` intentionally differs from main only
  by the `.slot-code .code-line*` rules removed from `themes/base.css`.
