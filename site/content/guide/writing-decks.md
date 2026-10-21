+++
title = "Writing Decks"
weight = 20
template = "guide-page.html"
description = "Write Markdown decks with slide separators, page settings, explicit slots, notes, and agenda sections."
+++

## Deck file shape

A deck is one Markdown file. A deck may start with YAML frontmatter. Slides are
split by `---`. A page settings comment belongs before the slide content when
the slide needs a stable key, an explicit layout, or an agenda marker.

This shape is adapted from `examples/peitho-tour/deck.md`:

````markdown
---
time: 5m
---

<!-- {"key":"cover","section":"Intro","time":"2m"} -->
# A Peitho Tour

<!-- Welcome! This deck is plain Markdown. -->

---

<!-- {"key":"pillars","section":"Design","time":"3m"} -->
# Three design pillars

- Content and design stay separate
- Design that fits in git
- Type-checked slot contracts
````

## Convention mapping

Convention mapping turns Markdown into slots without extra notation:

- The shallowest heading maps to `title`.
- Fenced code blocks map to `code`.
- An image-only paragraph maps to the image slot.
- All other blocks map to `body`.

A table ends at a blank line, so separate any following prose from it with one.

Markdown images are deck-relative local files. They must use supported local
image extensions (`png`, `jpg`, `jpeg`, `gif`, `webp`, `svg`) and must map to a
layout with exactly one unambiguous `accepts="image"` slot.

An SVG image is copied unchanged and shown through `<img>`, so scripts inside
it never run. Its root `<svg>` must declare
`xmlns="http://www.w3.org/2000/svg"` (without it browsers show a broken image)
and carry absolute `width` and `height` attributes; an SVG with only a `viewBox` (or `width="100%"`) can collapse to
nothing inside a slot, so the build rejects it with the line number. Copy the
`viewBox` width and height into `width` and `height` to fix it. An SVG shown
through `<img>` cannot load the deck's web fonts and draws its text with the
viewer's system fonts; convert text to paths, or embed the font in the SVG,
when the exact typeface matters.

## Code images

Fenced `math` blocks render to KaTeX HTML+MathML at build time and flow into
body slots. No client-side math JavaScript or `code_images:` entry is needed.

Fenced `mermaid` source becomes an SVG image during the build with Peitho's
built-in Mermaid renderer.

A fenced `qrcode` block becomes a QR code SVG image. Surrounding whitespace is
trimmed, and the remaining text must be one non-empty line; multiple lines are
a build error, never joined. The code uses at least Medium error correction, black
modules, a white background, and a four-module quiet zone. The colors are fixed
because page CSS cannot reach inside an SVG loaded through `<img>`; for other
rendering, set `code_images.qrcode` to an external command (it receives the
body verbatim, like any override). Like Mermaid, the QR code needs an
`accepts="image"` slot, and fence options are not supported.

````markdown
```qrcode
https://peitho.gosu.ke/
```
````

An `embed` fence containing one X status URL defaults to an official-widget PNG
screenshot; `mode=card` on the fence opts into selectable, themeable HTML
without Chrome:

````markdown
```embed mode=card
https://x.com/gosukenator/status/2074821309259973046
```
````

X cards flow into `accepts="blocks"` slots. X screenshots flow into
`accepts="image"` slots. Card cache misses fetch raw oEmbed JSON with system
`curl`; screenshot cache misses use Chrome. `mode=screenshot` is the explicit X
default.

A bare non-X HTTP(S) URL uses generic JSON oEmbed discovery and always becomes
a static card:

````markdown
```embed
https://www.youtube.com/watch?v=dQw4w9WgXcQ
```
````

Generic cards flow into `accepts="blocks"` slots. They use a build-time-downloaded,
locally published thumbnail when oEmbed supplies one and fall back to a text
card with title, author, and provider metadata otherwise. Peitho never injects
provider HTML and never screenshots generic URLs. The author-written page URL
is the card permalink. `mode=` is X-only and is rejected on generic URLs.

Generic cache misses use bounded system `curl` requests with at most five
HTTP(S) redirects: 8 MiB for discovery HTML, 1 MiB for raw JSON, and 8 MiB for
thumbnail bytes. Discovery HTML is not cached; JSON and validated JPEG, PNG,
WebP, or GIF thumbnails are stored in `.peitho/embeds-cache/`. Complete hits
work offline, while a JSON hit with a missing thumbnail fetches only the image.
Failure help identifies the exact cache paths to delete for refresh.

For other diagram tags, or to override Mermaid, QR codes, math, or embed with an
external SVG renderer, add `code_images:` frontmatter. Each entry uses the fence
language tag as the key and a command string as the value. Peitho sends the
complete fenced source verbatim to stdin, including its trailing newline, and
reads SVG from stdout. An `embed` override rejects `mode=` fence options.

```yaml
code_images:
  dot: dot -Tsvg
  mermaid: mmdc -i - -o - -e svg  # optional override
  math: latex-to-svg              # optional override
  embed: tweet-to-svg             # optional override; receives the full body
```

After external conversion, those blocks behave like normal images and need an
`accepts="image"` slot. See [Frontmatter](@/guide/frontmatter.md#code-images),
the [Code Images example](@/examples/code-images.md), and the
[Math example](@/examples/math.md). See [Tweet Embed](@/examples/tweet-embed.md)
for the built-in X and generic embed paths.

## Explicit slot syntax

Use `::: {slot=name}` when convention mapping cannot choose between slots. This
comes up in layouts with two columns or multiple `accepts="blocks"` slots.

This example is adapted from `examples/two-column/deck.md`:

````markdown
# Compare approaches

::: {slot=left}

Convention mapping sends headings to `title`, code to `code`, and the rest to
`body`.

:::

::: {slot=right}

Use `::: {slot=name}` when a two-column layout has `left` and `right` slots
with the same accepted content.

:::
````

The slot name must exist in the chosen layout, and the explicit content still
has to satisfy that slot's accepted content type and arity.

## Incremental reveal

Use `::: {reveal}` to mark a group of blocks that reveal step by step in
`peitho present` and on the `peitho preview` single-slide stage. Content outside
reveal groups is always visible. Each direct
child block is one step; a list contributes one step per top-level item, and
nested items appear with their parent. Multiple reveal groups on the same slide
number continuously.

Published distributions show the final state by default. Opt their viewer into
the same step sequence with deck frontmatter:

```yaml
public_reveal: steps
```

In that mode, next-entry starts a slide at step 0 and previous-entry returns to
the preceding slide fully revealed. A direct slide jump is fully revealed, and
a missing or invalid `step` URL value also selects the fully revealed state.
The URL records the current position as `?slide=N&step=M`.

[Watch reveal steps advance in peitho present](/guide-videos/reveal-demo.mp4)

````markdown
# Launch plan

Always visible context.

::: {reveal}

- Problem framing
- Demo path
  - Setup
  - Recovery path
- Decision

:::

Still visible while the first group steps.

::: {reveal}

First, show the architecture.

```rust
fn main() {
    println!("ship it");
}
```

:::
````

In this example, the three top-level list items are steps 1-3; the nested items
appear with "Demo path". The paragraph in the second group is step 4, and the
code block is step 5.

Plain-text paragraphs may start directly after an opening `:::` marker or end
directly before a closing `:::` marker. Images and other block shapes, such as
lists, still need blank lines around the markers; those shapes are not split if
Markdown fuses them to a marker, and the build fails with a line-numbered error.

Published distributions and PDF export show the final state by default. With
`public_reveal: steps`, the published viewer walks the same step sequence and
PDF export emits one page per position, step 0 through the fully revealed step
N. A slide with N steps therefore contributes N + 1 PDF pages; a slide without
steps contributes one. Repeated PDF pages keep the slide's own page number and
the logical slide total. The preview stage walks the same positions regardless
of `public_reveal`; next-entry starts at step 0 and previous-entry is fully
revealed. Every direct single-mode landing starts at step 0, including Up/Down,
filmstrip clicks, grid opens, Home/End, key/index navigation, and leaving the
grid. A valid saved step is clamped to the current slide's step count when
restored; a missing or invalid saved step in single mode starts at step 0, while
grid restoration stays fully revealed. Preview filmstrip thumbnails and
overview tiles, plus lint, always show the final state.

`{reveal=value}`, empty groups, unclosed fences, nested fences, and
multi-attribute fences are line-numbered build errors.

## Syntax highlighting

Tag a fenced code block with a language and Peitho highlights it:

````markdown
```rust
fn main() {
    println!("hello");
}
```
````

Highlighting runs at build time with syntect, so slides carry no client-side
highlighter and code never flashes unstyled before coloring in.

A fence with no language tag stays plain text. A tag Peitho cannot resolve is a
line-numbered build error rather than silently unhighlighted code:

```
error: slide 1 ('cover'), line 6: unknown code language 'crn'
 help: use a language name syntect recognizes (e.g. rust, js, ts, py, sh, toml,
       json, yaml, html, css, md, go, c, cpp, java, rb) or remove the tag
```

### Coloring code from CSS

Highlighting emits `<span>` elements carrying `hl-`-prefixed classes rather
than inline colors, so the palette lives in theme CSS like everything else. The
classes are syntax scope names — `hl-keyword`, `hl-string`, `hl-comment`,
`hl-function`, `hl-type`, `hl-constant`, `hl-storage`, and others depending on
the grammar:

```css
.slot-code .hl-keyword { color: #c678dd; }
.slot-code .hl-string  { color: #98c379; }
.slot-code .hl-comment { color: #7f848e; font-style: italic; }
```

The `hl-` prefix keeps these from colliding with layout or slot classes.

### Teaching Peitho a new language

To highlight a language syntect does not ship, put a `.sublime-syntax` grammar
in a `syntaxes/` directory next to the deck, or point the
[`syntaxes`](@/guide/frontmatter.md) frontmatter key at a file or directory.
Custom grammars augment the built-in set rather than replacing it.

A fence tag resolves against the grammar's name **and** its declared
`file_extensions`, so a grammar declaring:

```yaml
name: Carina
file_extensions:
  - crn
scope: source.crn
```

highlights ```` ```crn ```` blocks. Adding the grammar is what turns that tag
from a build error into highlighted code — see the
[Custom Syntax example](https://peitho.gosu.ke/examples/custom-syntax/).

## Code line emphasis

Point at specific lines of a code block — "the line I'm talking about right
now". This is separate from syntax highlighting: highlighting colors code by
what it *is*, emphasis marks where you are in the talk.

[Watch stepped line emphasis move through a function in peitho present](/guide-videos/emphasis-demo.mp4)

Write the lines in braces after the language tag:

````markdown
```rust {3}
pub struct Deck<Phase> {
    slides: Vec<Slide>,
    _phase: PhantomData<Phase>,
}
```
````

A `|` turns emphasis into a walkthrough: each group becomes one step in
`peitho present`, on the `peitho preview` stage, in a `public_reveal: steps`
distribution, and in PDF export when `public_reveal: steps` is set. Emphasis
uses equality with the current
step, so it moves from group to group rather than accumulating as you advance.

````markdown
```rust {3|6-8}
```
````

Groups may overlap: `{1-5|3}` emphasizes lines 1–5, then narrows to line 3 on
the next step.

A leading `|` makes a single group stepped, so `{|3}` consumes one step:

````markdown
```rust {|3}
```
````

The code is fully visible from the start either way — only the emphasis moves.
Emphasis steps share the same step space as [incremental reveal](#incremental-reveal),
so `next` walks through both in source order.

That is how to point at one line in each of two code blocks in turn: give
each block a single stepped group (`{|1}`), and source order gives each block
its own step.

Line numbers are 1-based and count every line of the block, including blank
ones. Trailing empty lines at the end of the block are not counted. Within a
group, `,` separates entries and `-` makes a range:
`{2,5-7|9}` emphasizes lines 2 and 5–7 on the first step, then line 9.

The language tag is optional — ` ```{2-4} ` works on an untagged block.

**Static vs stepped.** The `|` is the only difference:

| Notation | Steps used | Where it appears |
| --- | --- | --- |
| `{2-4}` (no `\|`) | none | everywhere, including PDF and published output |
| `{\|3}` | one | `peitho present`, the `peitho preview` stage, distributions with `public_reveal: steps`, and PDFs with `public_reveal: steps` |
| `{2-4\|6-8}` | one per group | `peitho present`, the `peitho preview` stage, distributions with `public_reveal: steps`, and PDFs with `public_reveal: steps` |

Static emphasis says "these lines are the important ones" — a property of the
content, so it ships with the deck. Stepped emphasis is a pointer that follows
your narration. The `peitho preview` stage moves it with the current step;
direct single-mode landings start at step 0, so the pointer activates only after
you advance to its matching step. Preview filmstrip thumbnails and overview
tiles, lint, and default/final published and PDF output leave it inactive. PDFs
with `public_reveal: steps` emit each exact-match emphasis state in the shared
step sequence.

When a deck uses static emphasis, its default styling travels with the deck,
including when the deck has its own `css/`. Restyle it with the
`--peitho-emphasis-background`, `--peitho-emphasis-marker`, and
`--peitho-emphasis-dim` custom properties, or override
`.code-line-emphasis` directly. Stepped emphasis is styled by `peitho present`,
the `peitho preview` stage, distributions that set `public_reveal: steps`, and
PDF exports that set `public_reveal: steps`.

Line numbers past the end of the block, malformed specs, stepped emphasis
inside a `::: {reveal}` group (which would nest two step spaces), and emphasis
on a `code_images` block are all line-numbered build errors.

## Code block titles

Add `title=` after the language to label a code block. Titles are one-line
inline Markdown: text, links, code spans, emphasis, strong emphasis, and
strikethrough are supported. Fence attributes are whitespace-separated, so
keep a space between `title=...` and an emphasis group. Quote a title when it
contains spaces or fence-syntax characters:

````markdown
```hcl title="network ([provider docs](https://registry.terraform.io/))"
name = upper(aws_vpc.main.id)
```
````

A backtick fence's info line cannot contain backticks under CommonMark. To use
a code span in a title, use a `~~~` fence:

````markdown
~~~python title="`__init__.py`"
def create_app(): ...
~~~
````

Markdown-significant characters follow normal Markdown rules. For example,
write `\_\_init\_\_.py` with backslash escapes, or use a `~~~` fence with the
code span shown above. HTML-looking text such as `Output<string>` is shown as
written. Reference-style links are not resolved in titles; use an inline link
such as `[text](url)`. Block syntax and images in a title are line-numbered build
errors.

A title combines with [code line emphasis](#code-line-emphasis) in either
order. For example, the form ```` ```ts {2} title="app.ts" ```` is equivalent
to the form ```` ```ts title="app.ts" {2} ````. Titles also work inside
`::: {reveal}`; the caption appears at the same reveal step as its code block.

The rendered caption is attached to its block:

```html
<figure class="code-block">
  <figcaption class="code-title">network (<a href="https://registry.terraform.io/" target="_blank" rel="noopener">provider docs</a>)</figcaption>
  <pre class="slot-code"><code class="language-hcl">…</code></pre>
</figure>
```

Theme the wrapper with `.code-block` and the caption with `.code-title`.
Peitho includes a small default stylesheet only when the deck uses a title: it
resets the figure margin, adds vertical space on both sides when a titled block
is adjacent to another element, and gives the caption a modest bottom margin
and reduced opacity. Existing `.slot-*` code styling still applies to the
`<pre>`.

A title must be non-empty and may appear only once on a fence. Titles on
`code_images` renderer blocks are build errors because those fences become
images or cards rather than code blocks. Titles on fences nested inside lists
or blockquotes are also line-numbered build errors; move those blocks to the
slide's top level.

## Links

A link to an `http:` or `https:` URL (`[source](https://example.com/post)`)
is rendered with `target="_blank" rel="noopener"`, so clicking a citation
mid-presentation opens it in a new tab (a new window when the slides run in
`peitho present`'s app-mode window) instead of replacing the deck. The click
itself never advances the slide either. This applies in `peitho preview`,
`peitho present`, and the built deck alike. Relative paths, `#fragment`
destinations, and non-navigating schemes such as `mailto:` keep the browser's
default behavior, and there is no switch — a link that leaves the deck should
never navigate the deck away.

## Speaker notes

Non-JSON HTML comments anywhere in a slide become presenter speaker notes. Empty
comments are ignored, and multiple comments on the same slide are joined with a
blank line.

Speaker notes ride into the presenter data, but notes never enter `dist/`.
`peitho present` reads `notes.json`; distributed slides do not contain notes,
and the publish contamination check enforces that boundary. Presenter notes are
rendered as plaintext.

![Notes appear in the presenter view alongside the current and next slides](/guide-shots/presenter-view.png)

## Footnotes

Footnotes keep a claim short while the supporting detail stays on the slide.
Reference a label inline with `[^label]` and define it with `[^label]: body`:

```markdown
# Claims stay short

- A claim can stay crisp while the evidence stays nearby[^study]
- The same source can support a second point without a new number[^study]
- A separate source receives the next number by first reference[^survey]

[^study]: The same label keeps the same number every time it appears here.
[^survey]: Numbering follows first-reference order, not definition order.
```

Footnotes are scoped to a single slide, so the same label may be reused on
another slide without collision. Markers are numbered by first-reference order
rather than definition order, and a label referenced several times keeps one
number. Bodies take inline Markdown — emphasis, `code`, and links.

A footnote body is one paragraph. Leave a blank line after the definition;
a definition with no paragraph content is a build error.

Every reference needs a definition and every definition needs a reference:
undefined references, unused definitions, and duplicate definitions of the same
label are all line-numbered build errors, so a missing citation never ships
silently.

Markers inside code spans and code blocks stay literal, so a regex character
class such as `` `/^[^a-z]+$/` `` is not read as a citation.

### Placing footnotes with a slot

By default footnote entries render as body content. A layout that declares a
`footnotes` slot receives them there instead, which pins them to a footer:

```html
<footer class="footnotes">
  <slot name="footnotes" accepts="blocks" arity="0..1"></slot>
</footer>
```

Routing is automatic — no explicit slot syntax is needed on the Markdown side.

With [incremental reveal](#incremental-reveal), a footnote entry appears with
the step of its earliest reference. If any reference sits in content that is
never revealed step-by-step, the entry stays visible from the start.

## Page settings comments

JSON HTML comments carry page settings. The supported settings are `key`,
`layout`, `section`, `time`, `draft`, `skip`, `present_only`, `page_number`,
and `include`.

```markdown
<!-- {"key":"checks","layout":"agenda","section":"Contracts","time":"3m"} -->
```

`key` gives the slide a stable target for CSS. `layout` requests a named
layout. `section` and `time` mark agenda sections. A slide accepts at most one
page settings comment, so combine the settings in one JSON object when a slide
needs more than one.

`draft:true` excludes a slide from the build. Draft slides are not rendered,
not counted, not written to the manifest, and do not contribute notes.
`skip:true` keeps a slide in the deck and manifest, but `next` and `prev`
navigation step over it in the presentation and single-slide preview shells.
Presentation and preview open on the first non-skipped slide (unless preview
restores a saved index). Direct navigation by key or index, Home/End first/last
navigation, and overview grid selection can still land on skipped slides; a
preview single-mode landing on one starts at step 0.

Draft slides cannot also be skipped or declare section markers. If every slide
is marked draft, the build fails instead of producing an empty deck.

`present_only:true` shows a slide when you present but keeps it out of
everything you share: `peitho present` and `peitho preview` include it, while
`peitho build` (and therefore `dist/` and `peitho publish`) and `peitho export pdf`
drop it the same way they drop a draft slide, so its content, images, and notes
never reach those outputs. `peitho lint` checks it like any other slide and
numbers slides the way `peitho preview` does, so its warnings point at the
number you see in preview. Use it for a live demo QR code, a
temporary URL, or anything that only belongs in the room. In present it counts
toward the surrounding section's time; published page numbers skip it. It can be
combined with `skip:true` for a backup slide you reach only by direct
navigation. A present-only slide cannot be a draft or declare a section marker,
and if every non-draft slide is present-only, `peitho build` fails rather than
publishing an empty deck. Keyed CSS rules for a present-only slide stay in the
built stylesheet (only the slide is removed), so do not put content that must
stay private in CSS.

`page_number:false` hides the page number on one slide when the deck sets
[`page_numbers`](@/guide/frontmatter.md). Only `false` is accepted; `true` is a
build error, as is using `page_number` on a deck that never turned page numbers
on.

## Agenda sections

Agenda sections use `section` and `time` together:

```markdown
<!-- {"section":"Basics","time":"2m"} -->
```

The marked slide starts a section that runs until the next marker. If any
section marker exists, the first slide must carry one. When deck frontmatter
sets `time`, the section totals must equal that deck time; when deck `time` is
absent, the section total becomes the deck's planned time.

## Splitting a deck across files

An `include` page settings comment on an otherwise empty slide splices the
referenced Markdown file's slides in at that position. Use it to share a
common intro or outro across a talk series, or to break a long deck into
smaller files.

```markdown
# Deck slide

---
<!-- {"include":"shared/intro.md"} -->
---
# Next deck slide
```

The include comment must be the only content of its slide, may not combine
with any other page settings key, and takes only `include` — a string with a
deck-relative Markdown path. The referenced file may contain any number of
slides, may nest further `include` comments up to 64 levels deep, and may
declare `section` and `time` markers that participate in the deck's normal
section-total-vs-`time` validation. Included files may **not** carry
frontmatter — the top-level deck is the sole source of deck-wide settings.

Paths are resolved relative to the including file and must stay under the
top-level deck's directory. Absolute paths, `..` escapes, missing targets,
cycles, chains exceeding 64 nested includes, empty included files, and any
of the above malformed shapes are all line-numbered build errors. Files that
end inside an unclosed code fence or HTML block are also rejected because the
unterminated block would otherwise swallow the following slide. Watch mode
observes the entire include tree, so edits to any included file trigger a
rebuild.
