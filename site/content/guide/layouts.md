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

## Styling empty slot wrappers

Rendered slide roots list empty slots in `data-empty-slots`, so CSS can hide
their wrappers without relying on whitespace-sensitive `:empty` matching:

```css
.peitho-slide[data-empty-slots~="quote"] .quote { display: none; }
```

Here, `.quote` is whatever class the layout author put on the wrapper element;
`quote` in the attribute selector is the slot name, and Peitho does not tie the
two together.

## Hybrid dispatch

When multiple layouts are available, Peitho chooses a layout in this order:

1. An explicit page setting such as `<!-- {"layout":"name"} -->`.
2. The single available layout, unconditionally.
3. Exactly one structural match between the slide's content and a layout's slot
   contract.

An unknown explicit layout name is a build error. With structural dispatch,
zero matches and multiple matches are also build errors; Peitho does not pick a
layout silently.

Code-image conversion runs before layout dispatch. Built-in Mermaid fences and
fences covered by `code_images:` entries are image fragments by the time
dispatch runs, so they route to `accepts="image"` slots rather than
`accepts="code"` slots. Layouts intended for rendered diagrams should expose an
image slot. Source panes that must stay visible should wrap the fence in a
four-backtick ``` ````md ``` block (as in the
[Code Images example](@/examples/code-images.md)) or use a different language
tag.

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

## Files a layout references

A layout may name a file directly rather than going through Markdown:

```html
<video class="backdrop" src="media/loop.mp4" poster="media/poster.png"
       autoplay muted loop playsinline></video>
```

Peitho reads that reference at parse time, resolves it against the deck
directory, copies the file under a content-hashed name, and rewrites the
attribute to point at the copy. The same applies to `<img src>`,
`<script src>`, `<link href>`, `<object data>`, and the other attributes that
load a subresource. A missing path is a build error naming the layout and the
attribute, and while `peitho preview` runs, replacing the file rebuilds the
deck.

This is where a background image or video belongs. CSS `url()` is not
scanned, so a file named only from a stylesheet is never copied and never
checked — see
[Referencing files from CSS](@/guide/frontmatter.md#referencing-files-from-css).
Put the element in the layout and keep the styling in CSS:

```css
.cover .backdrop {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
```

External, protocol-relative, `data:`, fragment, and rooted values are left
untouched. `srcset` and `imagesrcset` are refused with a named error rather
than silently ignored.

## Scripts in a layout

A layout can carry its own `<script>`, which is how a slide gets interactive
behaviour: a counter, a chart drawn from a data island, a small widget. It
lives in layout HTML, so it is design, not content, and nothing changes in
Markdown.

### Where scripts run

| Surface | Runs | Notes |
|---|---|---|
| `peitho present`, slides window | yes | once per slide when the deck loads |
| presenter panes, remote preview | yes | mirrors: their slides are inert, so a click never operates a copy the audience does not see |
| `peitho preview` | stage only | filmstrip thumbnails never run scripts, so whatever a script draws is missing from them |
| `peitho build` (`dist/`) | yes | again on every visit to the slide |
| `peitho export pdf`, `peitho lint` | yes | the page is parsed once; nothing re-runs |

### Finding your own slide

Each slide lives in its own shadow root, so `document.querySelector` inside a
layout script does not see the slide the script belongs to, and
`document.currentScript` is `null` there. Instead, peitho announces every
slide it mounts with a `peitho:shadow-mounted` event whose `detail` is
`{ root, key, index }`. `root` is the slide's shadow root, or the slide's
`<section class="peitho-slide">` element where there is no shadow root (the
distribution viewer, PDF, lint). The same objects are also kept in the
`window.__peithoShadowRoots` array, which exists before any layout script
runs, so a script that loads later can still find every slide.

A layout script runs once for every slide that uses the layout, in every copy
of the deck that is mounted (the presenter window holds two: the current and
the next slide), while the event is global: it announces every
slide, including slides that use other layouts. So install the handler once,
drain the array, listen for later mounts, look up your own elements and return
early when they are missing, and mount each root at most once. A handler that
throws while draining stops the drain, so the remaining slides are never
mounted:

```html
<section class="peitho-slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body"><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure class="code"><slot name="code" accepts="code" arity="0..1"></slot></figure>
  <footer class="footnotes"><slot name="footnotes" accepts="blocks" arity="0..1"></slot></footer>
  <button class="counter" type="button">0</button>
  <script>
    if (!window.counterLayoutInstalled) {
      window.counterLayoutInstalled = true;
      const mount = ({ root }) => {
        const button = root.querySelector(".counter");
        if (!button || button.dataset.mounted) return;
        button.dataset.mounted = "true";
        let count = 0;
        button.addEventListener("click", () => {
          count += 1;
          button.textContent = String(count);
        });
      };
      window.__peithoShadowRoots.forEach(mount);
      document.addEventListener("peitho:shadow-mounted", (event) => mount(event.detail));
    }
  </script>
</section>
```

`key` and `index` in the detail tell a script which slide it is looking at
without reading the DOM. The event is a notification only: a slide may listen,
but it must not dispatch `peitho:navigate` or any other request.

### Scope and loading order

- Write every inline script so its declarations live inside a block, as the
  example does with its install guard. In `peitho present`, `peitho preview`,
  and `dist/`, peitho wraps each classic inline script in its own function
  scope, so a stray top-level `let` or `const` would not collide there — but
  `peitho export pdf` and `peitho lint` render the page as-is, where two slides
  sharing a layout run the same script twice at top level and the second
  declaration throws "already declared". Publish anything shared as
  `window.something = …`.
- An external classic `<script src>` is never wrapped. It runs once per mounted
  slide, and again on every visit in `dist/`, so a top-level `let`, `const`, or
  `class` in it throws "already declared" from the second run. Load libraries
  as `<script type="module">`, which the browser runs once per URL, or guard
  them.
- An inline script after a `<script src>` waits for it, as in a normal page.
- Module scripts run after the slides have mounted; they find every slide in
  `window.__peithoShadowRoots`. Leave that array as an array.
- External SVG `<script href>` files run in the order they load, not in
  document order.

### Controls on a slide

Keys and clicks aimed at a control on the slide go to the control, not to the
deck: typing in a text field, pressing Enter on a link, Space or Enter on a
button or summary, and arrow keys on a range or radio input do not change
slides. Clicking a link, button, summary, label, or form field does not
advance either; a click anywhere else on the slide still does, including on a
`<canvas>` or `<div>` a script listens to, so give clickable things a real
control. Unshifted PageUp and PageDown still navigate from a text field, so a
presentation clicker keeps working; on a focused range slider they, and Home
and End, move the slider instead. Escape inside a slide text field does not
close the presentation; move focus out of the field first, for example with
Tab.

### PDF export and lint

`peitho export pdf` and `peitho lint` render every slide in one ordinary page,
so layout scripts run there as well and receive the same event. Only drawing
that finishes synchronously, or before the page's `load` event, is guaranteed
to be in the output, and both commands stop waiting for `load` after two
seconds: work after a `fetch` or an `await` races the print or the
measurement. A layout script must not duplicate or remove its own slide
section — moving it is fine — because both commands then fail with the slide's
key rather than printing a slide whose script never found its root.
