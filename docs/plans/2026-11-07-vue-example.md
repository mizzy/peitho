# Vue example deck

Goal: show that layout scripts can mount a framework app, using Vue 3.

- `examples/vue/`: two layouts whose `<script type="module">` imports Vue from
  a pinned CDN URL and mounts one app per slide through
  `window.__peithoShadowRoots` and `peitho:shadow-mounted`.
  - `cover`: the Markdown title seeds a `v-model` input; `<TransitionGroup>`
    animates each letter in.
  - `poll`: the Markdown list supplies the options; `computed` ranks them and
    `<TransitionGroup>` animates the re-sort.
  - `morph`: each Markdown code block is a step; Vue holds the step state and
    Shiki Magic Move (`core` + `renderer`, not its Vue component, which imports
    a bare `vue` and would load a second Vue) animates between them. Pinned to
    shiki/@shikijs/magic-move 4.5.0 (4.6.0 is under two weeks old).
  - `marker`: Rough Notation 0.5.1 marks each `strong` in a `::: {reveal}`
    list when a `MutationObserver` on the slide's own body sees its
    `data-reveal-hidden` clear, and hides it again on step back. Each
    annotation's `svgRect` is overridden to divide by the slide's transform
    scale (rough-notation measures in screen pixels); its `@keyframes` are
    redefined in the deck CSS because the library injects them only into
    `document.head`. The deck sets `public_reveal: steps`.
  - `motion`: `@vueuse/motion` 3.0.3 springs the Markdown list in as cards
    (`visible` variant, so it replays on every visit). Its bare `import 'vue'`
    is resolved by an import map added at runtime that points at the layout's
    own Vue URL; `@media print` puts the cards at rest for PDF, and the initial
    variant has no `scale` so lint's text-size check is unaffected.
  - `types`: Twoslash (`twoslash-cdn` 0.3.9, `@shikijs/twoslash` 4.5.0,
    TypeScript 6.0.3) renders `// ^?` queries and hover types, with Vue's
    types fetched from the CDN.
  Dropped from Slidev's feature list: drawing (a click on a canvas advances the
  deck, and pens belong to the shell) and slide transitions (shell territory,
  needs an author decision).
  - `marker`: Rough Notation 0.5.1 marks each `strong` in a `::: {reveal}`
    list when a `MutationObserver` on the slide's own body sees its
    `data-reveal-hidden` clear, and hides it again on step back. Each
    annotation's `svgRect` is overridden to divide by the slide's transform
    scale (rough-notation measures in screen pixels); its `@keyframes` are
    redefined in the deck CSS because the library injects them only into
    `document.head`. The deck sets `public_reveal: steps`.
  - `motion`: `@vueuse/motion` 3.0.3 springs the Markdown list in as cards
    (`visible` variant, so it replays on every visit). Its bare `import 'vue'`
    is resolved by an import map added at runtime that points at the layout's
    own Vue URL; `@media print` puts the cards at rest for PDF, and the initial
    variant has no `scale` so lint's text-size check is unaffected.
  - `types`: Twoslash (`twoslash-cdn` 0.3.9, `@shikijs/twoslash` 4.5.0,
    TypeScript 6.0.3) renders `// ^?` queries and hover types, with Vue's
    types fetched from the CDN.
  Dropped from Slidev's feature list: drawing (a click on a canvas advances the
  deck, and pens belong to the shell) and slide transitions (shell territory,
  needs an author decision).
  The Markdown source is hidden only after mounting, so a slide still shows its
  content when Vue cannot load.
- Vue is not vendored: a layout-referenced asset is published under a hashed
  path, which an inline module's `import` cannot name.
- Wiring: `DEMO_DECKS`, `site/content/examples/vue.md`, and the
  edit-annotations-off slide hash snapshot.
- Verified: `peitho lint` is clean; in the built `dist/` typing, voting, and the
  morph button never advance the slide, and arrow keys draw and clear the marks
  step by step; the `peitho preview` stage (shadow roots) mounts every app,
  draws the marks with PageDown, and renders Twoslash; `peitho export pdf` was
  checked page by page.
