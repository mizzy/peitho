+++
title = "Vue"
weight = 77
template = "example-page.html"
description = "Vue apps mounted by layout scripts: a v-model cover whose letters animate in as you type, a live poll that re-ranks itself with TransitionGroup, code that morphs with Shiki Magic Move, hand-drawn marks that follow the reveal steps, spring-in cards with v-motion, and live TypeScript types with Twoslash."

[extra]
deck = "vue"
demo_path = "/demo/vue/"
source_path = "static/deck-sources/vue/deck.md"
github_path = "examples/vue"
+++

## What it demonstrates

A layout script can bring in a framework. Each layout in this deck imports Vue
in a `<script type="module">` and mounts one app per slide, while the Markdown
stays plain content: the cover's title and the poll's list.

- **Cover**: the Markdown title becomes `v-model` state. Edit it in the input
  and each new letter pops in through `<TransitionGroup>`.
- **Poll**: the list items become options. Each vote updates `computed` state
  that re-sorts the ranking, and `<TransitionGroup>` slides the rows into
  their new places.
- **Morph**: each Markdown code block is one step. Vue holds the step state,
  and [Shiki Magic Move](https://github.com/shikijs/shiki-magic-move) animates
  the tokens from one block to the next.
- **Marker**: a `::: {reveal}` list whose bold words get a hand-drawn
  underline, circle, highlight, or box from
  [Rough Notation](https://roughnotation.com/) as each step shows them, and
  lose it when stepping back. The layout only watches its own slide's
  `data-reveal-hidden` attributes with a `MutationObserver`; peitho drives the
  steps, so PageDown and a presentation clicker work as on any other slide.
- **Motion**: the list items spring in as cards with
  [`v-motion`](https://motion.vueuse.org/), replaying every time the slide
  comes into view.
- **Types**: [Twoslash](https://twoslash.netlify.app/) runs the TypeScript
  compiler in the browser, so `// ^?` comments in the Markdown code block
  become type popups and every identifier shows its type on hover. Vue's own
  type definitions are fetched from the CDN.
- **Marker**: a `::: {reveal}` list whose bold words get a hand-drawn
  underline, circle, highlight, or box from
  [Rough Notation](https://roughnotation.com/) as each step shows them, and
  lose it when stepping back. The layout only watches its own slide's
  `data-reveal-hidden` attributes with a `MutationObserver`; peitho drives the
  steps, so PageDown and a presentation clicker work as on any other slide.
- **Motion**: the list items spring in as cards with
  [`v-motion`](https://motion.vueuse.org/), replaying every time the slide
  comes into view.
- **Types**: [Twoslash](https://twoslash.netlify.app/) runs the TypeScript
  compiler in the browser, so `// ^?` comments in the Markdown code block
  become type popups and every identifier shows its type on hover. Vue's own
  type definitions are fetched from the CDN.

A module script runs after the slides have mounted, so each layout drains
`window.__peithoShadowRoots` and listens for `peitho:shadow-mounted` to catch
slides mounted later. `createApp(...).mount()` accepts the element inside the
slide's shadow root directly:

```js
import { createApp, reactive, computed } from "https://cdn.jsdelivr.net/npm/vue@3.5.13/dist/vue.esm-browser.prod.js";

const mount = ({ root }) => {
  const source = root.querySelector(".poll .options");
  const target = root.querySelector(".poll .app");
  if (!source || !target || source.dataset.mounted) return;
  source.dataset.mounted = "true";
  const labels = [...source.querySelectorAll("li")].map((li) => li.textContent.trim());
  createApp({ /* setup + template */ }).mount(target);
};
window.__peithoShadowRoots.forEach(mount);
document.addEventListener("peitho:shadow-mounted", (event) => mount(event.detail));
```

## What to look at

The input and the poll options are real controls, so typing and voting never
advance the deck, while PageDown still does. Vue's transition classes land on
elements inside the shadow root, so the transition CSS lives in the deck's own
stylesheet like any other rule. The Markdown stays in the slide and is hidden
only once Vue has mounted, so without Vue the slide still shows its content.

The morph slide imports Magic Move's framework-free `core` and `renderer`
rather than its Vue component, whose bare `import 'vue'` would load a second
copy of Vue next to the one the layout imports. `@vueuse/motion` has the same
bare import, so the motion layout adds an import map that points `vue` at the
layout's own Vue URL before importing the plugin. Shiki highlights in the browser
and ignores peitho's build-time highlighting; until it loads, and wherever it
cannot, the slide shows the first code block as peitho rendered it.

Rough Notation measures elements in screen pixels, while peitho scales each
slide with a CSS transform, so the layout converts its measurements back to
slide pixels; the deck also defines Rough Notation's `@keyframes` in its own
stylesheet, because the library adds them only to `document.head`. The deck
sets `public_reveal: steps`, so the published deck steps through the marks too.

In `peitho export pdf` the cards print at rest through an `@media print` rule,
and the marks appear on the slide's final page only: PDF step pages are copied
before any mark is drawn. Twoslash usually takes longer to load than PDF export
and lint wait, so they show the code block as peitho rendered it.

Vue, Shiki, and Rough Notation load from a CDN, so presenting, publishing, and `peitho export pdf` need
network access. The full contract for layout scripts is in the guide's
[Scripts in a layout](@/guide/layouts.md#scripts-in-a-layout) section.
