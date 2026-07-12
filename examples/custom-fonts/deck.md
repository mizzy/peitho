<!-- {"key":"cover"} -->
# Typography travels with the deck

Bundled webfonts, zero config.

<!-- Everything on screen is a bundled font: Playfair Display for text, JetBrains Mono for code. Nothing here loads from a CDN. -->

---
<!-- {"key":"convention"} -->
# A fonts/ directory next to the deck

- Peitho auto-detects `fonts/` beside `deck.md` — no frontmatter needed
- Files are copied verbatim into build output, preview, present, and PDF export
- An explicit `fonts:` key in frontmatter can point anywhere else

---
<!-- {"key":"font-face"} -->
# The theme declares @font-face

The emitted `peitho.css` sits next to the copied `fonts/` directory, so a relative URL is all it takes.

```css
@font-face {
  font-family: "Playfair Display";
  src: url("fonts/playfair-display-latin.woff2") format("woff2");
  font-weight: 400 700;
}
```

---
<!-- {"key":"license"} -->
# Licenses ride along

- The copy step has no extension filter: `.woff2`, `.ttf`, and plain text all travel
- `OFL-*.txt` sits beside the font files and ships with every output
- This deck bundles Playfair Display and JetBrains Mono under the SIL Open Font License

<!-- Point out that shipping the license next to the font is not an accident — the no-extension-filter copy exists exactly so licensing and @font-face CSS can live with the fonts. -->
