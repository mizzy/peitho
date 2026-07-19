# Footnotes in slide bodies

Issue: #323. Author decision (2026-07-19): rendering model is **Option A — the
footnote block is appended at the end of the `body` slot**, following the
`FragmentKind::Math` precedent. A dedicated `footnotes` slot (Option B) and the
hybrid "explicit slot > body tail" dispatch (Option C) were considered; A was
chosen because it is zero-config (works with every layout that has a `body`
slot, no layout changes), minimal, and can later be extended to C
non-destructively if placement control is ever needed.

## Scope

- `[^label]` inline references render as superscript links; `[^label]: ...`
  definitions render as an ordered list inside a `.peitho-footnotes` block
  appended as the **last fragment of the `body` slot**.
- Footnotes are **per-slide**. A reference must resolve to a definition on the
  same slide and every definition must be referenced on the same slide.
  Cross-slide references are out of scope (they surface as the
  undefined-reference / unused-definition errors below).
- Definition position within the slide is unconstrained (like speaker notes):
  wherever the author writes `[^x]: ...`, the rendered block lands at the end
  of the body slot. Definitions written inside a `::: {slot=...}` group are
  still collected slide-wide (the group captures fragments; definitions are
  not fragments).
- No backlinks (↩) in v1 — on a slide everything is visible at once.

## Grammar: `ENABLE_OLD_FOOTNOTES`, one grammar, no text scanning

Measured (2026-07-19, pulldown-cmark 0.13): with GFM-style
`ENABLE_FOOTNOTES`, a `[^x]` reference with no definition in the same parse
input emits plain `Text` events — undefined references are invisible to the
event stream, which would force a hand-rolled raw-text scanner (a second
markdown tokenizer that drifts from pulldown's). With
`ENABLE_OLD_FOOTNOTES` instead, `[^x]` emits `FootnoteReference`
**regardless of whether a definition exists**, while escaped `\[^x]`,
inline-code `` `[^x]` ``, and fenced-code occurrences stay literal. Both
parse sides therefore use `ENABLE_OLD_FOOTNOTES` (replacing
`ENABLE_FOOTNOTES` in `parser_options` / `slide_split_options`, in the
render-time markdown-run / heading re-parses, and in the manifest
plain-text extraction in `plain.rs` — which drops `[^x]` markers from
`ManifestSlide.text` and appends footnote body text) and **all footnote
behavior comes from pulldown's event stream — no raw-text scanning
anywhere**.

Consequence of the old grammar (measured): a definition's body is exactly
one paragraph (lazy continuation lines join it; a GFM-style 4-space
indented continuation parses as a separate indented code block *outside*
the definition). So the v1 restriction is **single-paragraph definitions**,
which the grammar itself enforces.

Known grammar consequences (accepted, measured):

- A footnote reference inside image alt text (`![alt[^x]](i.png)`) or link
  text (`[click[^x]](url)`) breaks image/link tokenization in the old
  grammar — no `Image`/`Link` event is emitted, so the text renders as a
  visibly broken literal paragraph (plus the superscript). This mirrors how
  any malformed image/link Markdown degrades to text; it is visible, not
  silent, and `\[^x]` is the escape hatch.
- Definitions that only reference each other (`[^a]: see[^b].` /
  `[^b]: see[^a].`) satisfy the unused-definition check without any in-body
  reference, because references inside definition paragraphs count. This is
  the documented "validated like any other" rule applied consistently.

## Parser (`crates/peitho-core/src/parser.rs`)

Changes:

- Remove `Tag::FootnoteDefinition` from `unsupported_tag` /
  `unsupported_tag_name`.
- New per-slide state in `parse_slide`:
  - definitions: ordered entries `{ label, content_markdown, line }` plus
    duplicate detection by label
  - references: label → first-reference line, **in order of first reference**
    (this order assigns display numbers 1..n per slide)
- `Event::FootnoteReference(label)`: record the label + line (defined or
  not — the old grammar emits both; undefined ones fail validation below).
  **The arm must be placed before the `_ if list_depth > 0 => {}` swallow
  arm** so references inside list items are still recorded for validation
  (the list's raw slice keeps the `[^x]` text for render-time re-parsing,
  which is fine).
- `Tag::FootnoteDefinition(label)` start/end: while inside a definition,
  suppress normal fragment capture and capture the definition's single
  paragraph as its content markdown. **v1 restriction: definition bodies are
  one paragraph of inline Markdown** (grammar-enforced; see above). Any
  unsupported event inside a definition — block-level structures, but also
  images and inline HTML, matching the main body's rejection of inline
  HTML — is a line-numbered build error ("unsupported content in footnote
  definition", help: "keep footnote bodies to one paragraph of inline
  Markdown and leave a blank line after the definition"; the help doubles as
  guidance for the lazy-absorption case where a following block is swallowed
  because the blank line is missing) — exhaustive match, no `_ => {}`.
  Sibling errors: "unclosed footnote definition" (slide ends mid-capture)
  and "footnote definition needs paragraph content" (empty body).
  References *inside* definition paragraphs are recorded and validated like
  any other. Definitions inside list items are a line-numbered error.
- A footnote reference inside an open paragraph also drives the
  `ParagraphInline` state machine exactly like text, so a reference adjacent
  to an image (`![alt](i.png)[^a]` in either order) fails with the existing
  "mixed image paragraph" error instead of the marker being silently
  dropped when the paragraph collapses into an Image fragment.
- Validation at slide end (same place as the unclosed-slot-fence check), all
  `ErrorKind::Parse`, line-numbered, with help — no silent path:
  - reference with no same-slide definition → error at the reference line
    ("undefined footnote reference `[^x]`", help: add `[^x]: ...` on the same
    slide)
  - definition never referenced on the slide → error at the definition line
    ("unused footnote definition `[^x]`", help: reference it with `[^x]` or
    remove it)
  - duplicate definition label → error at the second definition's line
- If the slide has footnotes, append a single
  `FragmentKind::Footnotes { entries }` as the **last** fragment of the slide.
  Entries are ordered by first-reference order; each carries
  `{ number, label, markdown, line }`. The fragment's constructor is
  `pub(crate)` (Math precedent) so it cannot be fabricated outside the parser.
  Draft slides validate their own footnotes during parse (the draft drop
  happens at parse end, after per-slide validation — intentional).

## Mapping (`mapping.rs`)

`FragmentKind::Footnotes` routes to the `body` slot by convention (next to the
`Math` arm). A layout without a `body` slot sends it to `unassigned`, which the
existing `check_no_unassigned` turns into a line-numbered `ResidualContent`
error — no new error path.

## Check (`check.rs`)

Accept the pair `(Accepts::Blocks, FragmentKind::Footnotes)`. Everything else
falls through to the existing accepts error.

## Render (`render.rs`)

Two seams:

1. **Inline references.** `render_markdown_run` and `render_heading_inline`
   parse with `Options::empty()`, so `[^x]` would pass through literally.
   Enable `ENABLE_OLD_FOOTNOTES` there (same grammar as the parser — old
   mode emits `FootnoteReference` even though definitions were lifted out of
   the runs at parse time) and intercept `Event::FootnoteReference`,
   emitting `<sup class="peitho-footnote-ref"><a href="#fn-<slide>-<n>"><n></a></sup>`
   from a slide-scoped label → number map threaded into the renderers. No
   string replacement or raw-text scanning on the render side either. IDs
   are namespaced per slide (PDF export puts all slides in one document)
   using the slide identity available at render time. A label missing from
   the map, or a `FootnoteDefinition` appearing in a run, is unreachable
   after parse validation and must be a hard render error, not a silent
   literal.
2. **The block.** `render_block_slot` renders the `Footnotes` fragment last
   (it is the last body fragment by construction):

   ```html
   <div class="peitho-footnotes"><ol>
     <li id="fn-<slide>-1"><p>…</p></li>
   </ol></div>
   ```

   Entry bodies render through the same markdown-run renderer (inline markup
   works; the v1 paragraph-only restriction guarantees no block-level
   surprises).

## Theme (`themes/base.css`)

Add `.peitho-footnotes` (reduced font size, top border, top margin — separator
look) and `.peitho-footnote-ref` (superscript link styling) following existing
`.slot-*` conventions. Colors via the theme's existing palette only.

## Tests (TDD — write failing tests first)

- Parser: happy path (fragment appended last, entries in first-reference
  order, numbering); repeated references to one label; undefined reference /
  unused definition / duplicate definition errors with line numbers; reference
  on one slide + definition on another → errors in both directions;
  reference inside a list item is validated; lazy-continuation definition
  joins into one paragraph; escaped `\[^x]` and `` `[^x]` `` in code stay
  literal (no reference recorded, no error); definitions before/after their
  references; footnotes inside a `::: {slot=...}` deck still collect
  slide-wide.
- Mapping/check: `Footnotes` → body; layout without `body` slot →
  `ResidualContent`; accepts pairing.
- Render: reference HTML, block HTML, IDs unique across slides, escaping of
  label-derived content, inline markdown inside definition bodies.
- Gates: full workspace tests ×3, clippy, fmt, bindings drift, shell drift.
