+++
title = "Layouts"
weight = 30
template = "guide-page.html"
description = "Design Peitho slides with HTML slot contracts, CSS overrides, and predictable layout selection."
+++

## Layout as schema

Layouts are plain HTML. The `<slot>` tags inside the layout define the schema:
slot name, accepted content type, and arity live in the layout itself rather
than in a separate contract file.

Peitho checks Markdown content against that schema. Slot excess, slot
deficiency, type mismatches, broken references, and unassigned content are
build errors with line numbers and help.

## Slot contract syntax

A layout can declare slots like this:

```html
<slot name="title" accepts="inline" arity="1"></slot>
<slot name="body" accepts="blocks" arity="0..*"></slot>
<slot name="code" accepts="code" arity="0..1"></slot>
```

The same shape appears in the built-in `title-body-code` layout.

## Accepted content and arity

`accepts` describes the kind of Markdown content the slot can receive. The six
supported variants are `inline`, `blocks`, `text`, `code`, `image`, and `list`.

`arity` describes how many items the slot can receive. The four supported
arity literals are exactly `1`, `0..1`, `1..*`, and `0..*`. Any other literal,
such as `2` or `0..3`, is an `unknown arity value` build error. If a slide
gives a slot too much content, too little content, or the wrong kind of
content, the build fails with a line-numbered error instead of dropping the
content.

![peitho build refusing a deck with two code blocks against a slot that allows 0..1](/guide-shots/build-error.png)

## Hybrid dispatch

When multiple layouts are available, Peitho chooses a layout in this order:

1. An explicit page setting such as `<!-- {"layout":"name"} -->`.
2. The single available layout, unconditionally.
3. Exactly one structural match between the slide's content and a layout's slot
   contract.

An unknown explicit layout name is a build error. With structural dispatch,
zero matches and multiple matches are also build errors; Peitho does not pick a
layout silently.

## Inspecting layouts

Use `peitho layouts deck.md` to print the resolved layout source and each
slot contract. Add `--json` when another tool needs the same information.

When dispatch is unclear, use `peitho layouts deck.md --explain <slide-key>`.
The trace shows the resolved layout source, the addressed slide, each structural
candidate, and the final dispatch result. A missing slide key exits with status
2 and prints the known keys; a dispatch failure trace exits with status 1.
Explicit and sole-layout no-match failures include a `reason:` line, such as
`reason: no slot accepts image in layout 'cover'`, with the underlying mapping
error.

## Keyed CSS overrides

Give a slide a stable key in its page settings comment:

```markdown
<!-- {"key":"arch-1"} -->
# Peitho Architecture
```

Then target it from CSS:

```css
[data-slide-key="arch-1"] .slot-code {
  grid-column: 2 / 3;
  width: 60%;
}
```

The key survives title edits. Peitho validates keyed selectors against the
slots of that slide's layout, and a keyed override that points at a missing
slide key stops the build.

## Asset placement

Put custom layout HTML and CSS next to the deck, or point at them from
[frontmatter](@/guide/frontmatter.md).

For layouts and CSS, asset resolution is: explicit frontmatter path, then a
deck-adjacent `layouts/` or `css/` directory, then the built-in default. A
frontmatter path can point at a file or a directory; layout directories read
`*.html`, and CSS directories read `*.css`.
