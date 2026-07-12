# Custom fonts example (Issue #266)

Date: 2026-07-12

## Goal

Give the `fonts:` asset pipeline a working example deck. The feature (deck-adjacent
`fonts/` auto-detect, verbatim copy without an extension filter, `@font-face` CSS
resolving `fonts/*.woff2` relative to the emitted `peitho.css`) currently appears only
as a string inside a code sample in the peitho-tour deck.

## Shape

`examples/custom-fonts/` — a fully zero-config deck (no frontmatter): both `css/` and
`fonts/` are picked up by deck-adjacent auto-detect, which is itself part of what the
example demonstrates.

```
examples/custom-fonts/
  deck.md                    # no frontmatter; built-in title-body-code layout
  css/
    theme.css                # @font-face + full theme (replaces built-in base.css)
    overrides.css            # keyed override for the cover slide
  fonts/
    playfair-display-latin.woff2   # variable, weight 400-700, latin subset
    jetbrains-mono-latin.woff2     # weight 400, latin subset
    OFL-playfair-display.txt       # licenses ride along verbatim (no extension filter)
    OFL-jetbrains-mono.txt
```

Fonts are the latin-subset woff2 files served by Google Fonts (both SIL OFL). The OFL
texts sit inside `fonts/` deliberately: the copy step has no extension filter, so the
license ships with the font files in every output — that behavior is one of the slides.

## Content

Four slides on the built-in layout: a cover (large Playfair Display treatment via a
`[data-slide-key="cover"]` override — also the gallery screenshot), the `fonts/`
auto-detect convention, the `@font-face` CSS as a code slide (JetBrains Mono showing
itself), and the licensing/verbatim-copy story.

## Wiring

- `Makefile`: add `custom-fonts` to `DEMO_DECKS` (docs-sources / demo-site /
  demo-screenshots all loop over it)
- `site/content/examples/custom-fonts.md`: weight 35 (between Image Showcase and
  Keynote), `template = "example-page.html"`, standard `[extra]` deck wiring

## Non-goals

- No explicit `fonts:` frontmatter variant (the guide documents it; the example shows
  the zero-config path)
- No CJK subset (font payloads stay tens of kilobytes)
