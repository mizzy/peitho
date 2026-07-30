# Deck `lang` frontmatter

Date: 2026-07-30
Issue: none (direct request from the author)

## Problem

Every HTML shell peitho emits hardcodes `<html lang="en">`. For Japanese decks
this is not just semantically wrong — Chromium's `word-break: auto-phrase`
(BudouX phrase-based line breaking) only activates when the document language
is Japanese, so a theme that opts into phrase breaking renders differently in
`peitho build` output patched downstream (e.g. a site generator rewriting the
`lang` attribute) than in `peitho preview` / `peitho present`, which cannot be
patched. The deck's language is deck-level metadata, so per the zero-config
policy it belongs in the deck frontmatter.

## Design

- New frontmatter key `lang`, a BCP 47-style language tag (`en`, `ja`,
  `zh-Hans`, …). Default stays `en`, so existing decks render byte-identically.
- New validated newtype `DeckLang` (same pattern as `PointerColor`): trimmed,
  non-empty, ASCII-alphanumeric subtags separated by `-`, first character
  alphabetic, ≤ 35 chars. Invalid values are line-numbered parse errors with
  help; a bare `lang:` key is a "lang has no value" error. The constructor is
  the only way to obtain a non-default value, so the render layer can embed it
  in an HTML attribute without escaping.
- `DeckSettings` carries `lang: DeckLang` (not `Option` — the default rides the
  type) through every phase, like the other deck-level settings.
- The `lang` attribute is emitted on every page whose primary content is slide
  HTML: the distribution `index.html`, the PDF export document, the lint
  document, the present slides page, the preview page, and the presenter page
  (its stage mounts slide HTML, and notes are deck content).
- The remote controller and the preview error page are peitho UI in English;
  they keep `lang="en"`.
- No manifest / ts-rs contract change: the attribute is server-rendered into
  each page, and no TS consumer needs the value.

## Touch points

- `crates/peitho-core/src/phase.rs`: `DeckLang` newtype (+ `Default` = `en`),
  `DeckSettings` field, constructor arg, accessor.
- `crates/peitho-core/src/parser.rs`: `DeckFrontmatter.lang`,
  `parse_frontmatter_lang` (pointer_color pattern), supported-keys help list,
  `frontmatter_help` branch.
- `crates/peitho-core/src/render.rs`: `render_distribution_index`,
  `render_present_index`, `render_preview_index`, `render_presenter_index`
  gain a `lang: &DeckLang` parameter; `render_pdf_document` and
  `render_lint_document` read it from the deck they already take.
- `crates/peitho/src/main.rs`: pass `settings.lang()` at the call sites.
- Docs: `site/content/guide/frontmatter.md` key table, CLAUDE.md supported-keys
  sentence.

## Tests

- `DeckLang::parse`: valid (`en`, `ja`, `zh-Hans`), rejects empty, embedded
  whitespace, non-ASCII, leading digit, over-long values.
- Parser: `lang: ja` lands in settings; `lang:` (no value) and `lang: "no good"`
  are line-numbered errors with `DeckLang::HELP`; omitted key defaults to `en`.
- Render: each affected page emits `<html lang="ja">` for a `ja` deck and
  `<html lang="en">` by default; remote and preview-error pages stay `en`.
