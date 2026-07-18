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

Markdown images are deck-relative local files. They must use supported local
image extensions (`png`, `jpg`, `jpeg`, `gif`, `webp`) and must map to a layout
with exactly one unambiguous `accepts="image"` slot.

## Code images

Fenced `math` blocks render to KaTeX HTML+MathML at build time and flow into
body slots. No client-side math JavaScript or `code_images:` entry is needed.

Fenced `mermaid` source becomes an SVG image during the build with Peitho's
built-in Mermaid renderer. For other diagram tags, or to override Mermaid or
math with an external SVG renderer, add `code_images:` frontmatter. Each mapping
entry uses the fence language tag as the key and a command string as the value;
Peitho sends the fenced source to stdin and reads SVG from stdout.

```yaml
code_images:
  dot: dot -Tsvg
  mermaid: mmdc -i - -o - -e svg  # optional override
  math: latex-to-svg              # optional override
```

After external conversion, those blocks behave like normal images and need an
`accepts="image"` slot. See [Frontmatter](@/guide/frontmatter.md#code-images),
the [Code Images example](@/examples/code-images.md), and the
[Math example](@/examples/math.md).

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

## Speaker notes

Non-JSON HTML comments anywhere in a slide become presenter speaker notes. Empty
comments are ignored, and multiple comments on the same slide are joined with a
blank line.

Speaker notes ride into the presenter data, but notes never enter `dist/`.
`peitho present` reads `notes.json`; distributed slides do not contain notes,
and the publish contamination check enforces that boundary. Presenter notes are
rendered as plaintext.

![Notes appear in the presenter view alongside the current and next slides](/guide-shots/presenter-view.png)

## Page settings comments

JSON HTML comments carry page settings. The supported settings are `key`,
`layout`, `section`, `time`, `draft`, and `skip`.

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
navigation, and overview grid selection can still land on skipped slides.

Draft slides cannot also be skipped or declare section markers. If every slide
is marked draft, the build fails instead of producing an empty deck.

## Agenda sections

Agenda sections use `section` and `time` together:

```markdown
<!-- {"section":"Basics","time":"2m"} -->
```

The marked slide starts a section that runs until the next marker. If any
section marker exists, the first slide must carry one. When deck frontmatter
sets `time`, the section totals must equal that deck time; when deck `time` is
absent, the section total becomes the deck's planned time.
