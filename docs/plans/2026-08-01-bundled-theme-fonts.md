# Bundle fonts with the built-in theme

Issue: (filed with this work)
Date: 2026-08-01
Branch: `issue-385-slot-overflow` (found while fixing #385)

## Problem

A deck does not render the same on every machine. `themes/base.css` asks for `system-ui` and `ui-monospace`, which resolve to a different face per OS — San Francisco / SF Mono on macOS, DejaVu or Liberation on the Linux box that builds the demo site. Line heights differ, so content that fits locally clips in the published build.

This is not hypothetical. It surfaced as a CI failure on #385: `peitho-tour` slides 6 and 7 clipped 36px and 5px on Ubuntu while reporting zero overflow on macOS. The author develops on macOS, the demo site builds on `ubuntu-latest`, so **peitho.gosu.ke has been serving a clipped deck that is invisible to the person who wrote it**.

Measured on `peitho-tour` slide 6 (`markdown`), macOS, before any fix: the slide's flex children total 622px against 608px of available height — it was already 14px over locally, absorbed silently because `.code-panel` has `min-height: 0` and shrinks to hide the excess.

## Why the CSS-tuning approach was abandoned

The first attempt shrank `line-height` and `font-size` on the affected slides. That was treating the symptom. Measured: dropping the `sections` slide's code from 16px to 13px changed its headroom not at all, because `.slot-code`'s height follows its content — the box shrinks with the text and the ratio stays fixed. Headroom is decided by the slide's flex distribution (prose block plus panel chrome), not by the code font.

Even where tuning did buy headroom, it only widened the margin against *one* measured environment. The next font, the next OS, the next Chrome text-shaping change puts it back. The invariant that matters — a deck looks the same everywhere — cannot be restored by making the numbers slightly less tight.

## Decision (author, 2026-08-01)

**Bundle the fonts with the built-in theme.** Not with `peitho-tour`, and not with the examples that happen to have failed: `themes/base.css` is the default every deck inherits, and 8 of the 15 example decks use it directly with no CSS of their own. Fixing anything downstream of it leaves the same trap set for the next deck and for every user who runs `peitho new`.

| Face | Font | License | Size |
| --- | --- | --- | --- |
| Body | Inter (400/600/700) | OFL | ~24KB each |
| Code | JetBrains Mono (400) | OFL | 21KB |

Inter was chosen because it is designed for the same role `system-ui` fills on each platform, so decks keep their present character rather than acquiring a new one. JetBrains Mono is already vendored in `examples/custom-fonts`, license file included.

## Design

Follow the KaTeX precedent exactly — it already solves this shape. `crates/peitho-core/src/math.rs` embeds `katex.min.css` with `include_str!`, embeds each `.woff2` with `include_bytes!` behind a small macro, and writes the fonts into the output only for decks that need them.

- Assets live in `crates/peitho-core/assets/fonts/`, beside `assets/katex/`.
- A `ThemeFontAsset` mirrors `MathFontAsset`: a name and a `&'static [u8]`.
- `themes/base.css` declares `@font-face` for both families and names them first in its `font-family` stacks, keeping the existing OS names as fallbacks so a missing asset degrades to today's behavior rather than to an unstyled page.
- Fonts are written into `theme-fonts/` next to `peitho.css`, on **every** path that renders slides: build, preview cache, present cache, PDF export workspace, lint workspace. Missing them in any one of those reintroduces the divergence in that surface.
- A deck that supplies its own `css/` still replaces `themes/base.css` wholesale, so it opts out — as it does today. That is correct: the deck author owns their typography. The examples that ship CSS get the same `@font-face` block so they are environment-independent too.

## Scope

1. Vendor the assets and embed them.
2. `themes/base.css`: `@font-face` declarations and updated stacks.
3. Emit `theme-fonts/` on every slide-rendering path.
4. The 7 example decks with their own CSS: adopt the bundled families.
5. Revert the #385 CSS tuning on `peitho-tour` — with fonts pinned, those slides no longer need shrunken type, and leaving the workaround in would hide whether the real fix works.
6. Tests: assets present in output on each path; a deck with its own CSS is unaffected; `peitho-tour` lints clean.

## Consequence: the lint measurement script needed a bounded window-load wait

Bundling the fonts broke a lint E2E test on Linux — deterministically, confirmed across two CI runs, while passing five consecutive times on macOS. `lint_accepts_trivially_small_deck` (a deck containing only `# Tiny`) failed with:

```
Error: Chrome completed before one-shot output was ready
  help: expected lint measurement payload before Chrome exited
```

`font-display: swap` does **not** exclude a font from `window.load`'s completion criteria, so the four new `@font-face` fetches gate the load event. `lint_measure.js` chains `waitForWindowLoad → waitForImages → waitForFonts → waitForFrame → publish`, and the first link waited unconditionally. Under `--virtual-time-budget=10000` on Linux headless, the fetches pushed `load` past the budget; Chrome printed and exited before anything was published.

The asymmetry that makes this subtle: `setTimeout` advances with *virtual* time, which Chrome fast-forwards, while fetch and decode work proceeds in *real* time. A timeout inside this script cannot be reasoned about as if it were wall-clock.

`waitForFonts` already guarded exactly this shape, with a comment explaining that publishing an early measurement beats publishing none. That reasoning applies verbatim to `waitForWindowLoad`; the guard simply had never been extended there, because nothing in the built-in theme previously made `load` slow. Fixed by racing the load event against `WINDOW_LOAD_TIMEOUT_MS = 2000`, matching the existing `FONT_READY_TIMEOUT_MS`. The two bounds together consume at most 4s of the 10s budget.

`waitForImages` was reviewed and deliberately left alone: peitho rejects remote image URLs and copies validated local assets into the workspace before Chrome starts, and the script uses terminal `load`/`error` events rather than `image.decode()` — the Linux-safe pattern from `docs/plans/2026-07-06-pdf-flatten-linux-decode-hang.md`. Adding a timeout there would also risk measuring geometry before an image's dimensions settle. Do not add one without evidence that the event path hangs.

This belongs to the same pitfall family as the `decode()` hang already recorded in CLAUDE.md: **an unbounded wait inside a script running under virtual time is a latent Linux-only failure**, invisible on macOS until something makes that wait slow.

## Verification

The claim is "identical rendering across environments", so verifying on macOS alone cannot establish it. CI is the instrument: the `e2e` job runs the lint E2E tests on `ubuntu-latest`, and `lint_peitho_tour_has_no_overflow_warnings` is the assertion that the Linux rendering matches what macOS sees. A green run there is the evidence; a local run is not.

Also confirm output size: five faces at ~100KB total is added to every built deck. Acceptable against the alternative of decks that silently clip, but it should be a deliberate number rather than a surprise.
