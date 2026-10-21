# Inline Markdown in code block titles (Issue #732)

## Goal

````markdown
```iac-plan title="apply plan ([carina-rs/carina#146](https://github.com/carina-rs/carina/issues/146))"
```
````

renders

```html
<figcaption class="code-title">apply plan (<a href="https://github.com/carina-rs/carina/issues/146" target="_blank" rel="noopener">carina-rs/carina#146</a>)</figcaption>
```

instead of the raw link syntax. `target`/`rel` come from the existing
`open_external_links_in_new_tab` pass at the `render_slot` seam; nothing new is
needed for that.

## Design

`CodeTitle` (in `fence_info.rs`, module-private constructor) changes meaning
from "trimmed plain text" to "validated one-line inline Markdown". The value is
validated once, at the constructor, and rendering cannot fail afterwards:

- The title is parsed with pulldown-cmark (title-owned options, below). The event
  stream must be exactly one paragraph whose children are drawn from a closed
  allowlist: `Text`, `Code`, `Emphasis`, `Strong`, `Strikethrough`, and `Link`
  (inline links and autolinks).
- Everything else is a line-numbered build error with focused help — no silent
  drop (pillar 3):
  - block-level structure (`# x`, `- x`, `1. x`, `> x`, `---`, …): "code block
    title must be inline Markdown", help: escape the leading character with a
    backslash;
  - images: titles cannot hold images;
- Inline HTML and HTML blocks (`Output<string>`, `<!-- x -->`) are not an
  error: their raw text is rendered escaped, as literal characters (author
  decision 2026-10-21, after the first real deck hit `Output<string>` on
  v1.46.0: generics are common in code titles and HTML in a title has no use).
  Nothing is dropped and no HTML from a title reaches the output.
  A title that pulldown-cmark reads as a whole HTML block (`<div> **x**`) is
  shown entirely as written, with no Markdown, escape, or entity processing;
  a type-7 tag followed by text (`<custom> **x**`) is a paragraph, so its
  Markdown still applies.
- Titles parse with their own options (`ENABLE_STRIKETHROUGH` only), so
  `[^1]` and undefined `[ref]` stay literal text, as in any Markdown where they
  are undefined.
- The title is read from the author's raw fence-info source, not from
  pulldown-cmark's decoded info string: CommonMark already processes backslash
  escapes and entities in info strings, so parsing the decoded value as
  Markdown decoded `\_\_init\_\_.py` twice (found in review).
- The constructor renders the validated events once with
  `pulldown_cmark::html::push_html`, minus the paragraph wrapper, and stores
  only that HTML; `CodeTitle::html()` borrows it, so rendering has no failure
  or panic path.
- `render_code_block` inserts `title.html()` into the figcaption instead of
  `encode_text(as_str())`.

### Compatibility

Plain titles (`main.rs`, `main.tf`) render byte-identically. Titles that contain
Markdown-significant characters now follow Markdown rules, the same as
headings (`__init__.py` becomes `<strong>init</strong>.py`; write
`\_\_init\_\_.py`, or a code span in a `~~~` fence, since a backtick fence's
info line cannot contain backticks). Entities such as `&lt;` decode once as Markdown. Reading the raw info also means escapes and entities
in language and option tokens are no longer decoded (`rus&#116;` is an
unknown-language error instead of `rust`). Accepted: the title is
now inline Markdown like every other text on a slide.

## Tests

- fence_info: allowlisted titles accepted; each refused class is an error with
  the fence line number.
- render: link title renders an `<a>`; code span renders `<code>`; plain title
  unchanged; `<&>` still escaped.
- An end-to-end build asserting the external link gains `target="_blank"`.

## Follow-up (v1.46.1)

v1.46.0 refused `title="...Output<string>..."` as HTML. The author chose to
render title HTML as literal text instead; see the Design bullet above.
