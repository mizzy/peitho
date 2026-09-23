+++
title = "Layout Scripts"
weight = 74
template = "example-page.html"
description = "A layout whose own script wires an interactive counter, running the same way in present, preview, the published deck, and PDF."

[extra]
deck = "layout-scripts"
demo_path = "/demo/layout-scripts/"
source_path = "static/deck-sources/layout-scripts/deck.md"
github_path = "examples/layout-scripts"
+++

## What it demonstrates

Interactive behaviour is design, so it lives in the layout next to the HTML it
drives. This deck's layout carries a button and the `<script>` that wires it;
the Markdown only says which slides use that layout.

Each slide lives in its own shadow root, so a layout script cannot find its
slide with `document.querySelector`. peitho announces every slide it mounts
with a `peitho:shadow-mounted` event carrying `{ root, key, index }`, and keeps
the same objects in `window.__peithoShadowRoots` for scripts that load later.
The layout installs its handler once, mounts every slide it is told about, and
skips roots that are not its own:

```js
const mount = ({ root }) => {
  const button = root.querySelector(".counter");
  if (!button || button.dataset.mounted) return;
  button.dataset.mounted = "true";
  // wire the button
};
window.__peithoShadowRoots.forEach(mount);
document.addEventListener("peitho:shadow-mounted", (event) => mount(event.detail));
```

## What to look at

The first two slides share one layout and each keeps its own count: the handler
wires every announced root with its own closure, and the install guard makes the
script's second run on the second slide a no-op. Its declarations sit inside
that guard's block rather than at top level, which is what lets the same script
run unchanged in PDF export, where scripts are not scoped. Clicking the button
or pressing Space on it counts; it never advances the deck, while PageDown
still does.

The same script runs in `peitho present`, on the `peitho preview` stage (not in
its thumbnails), in the published deck (again on every visit to a slide), and
in `peitho export pdf`. The full contract is in the guide's
[Scripts in a layout](@/guide/layouts.md#scripts-in-a-layout) section.
