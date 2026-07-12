---
layouts: ./layouts
css: ./css
code_images:
  mermaid: mmdc -i - -o - -e svg
  dot: dot -Tsvg
---
<!-- {"key":"mermaid-flow"} -->
# Mermaid becomes a build artifact

Fenced `mermaid` source is sent to `mmdc` at build time. The SVG lands in
Peitho's code image cache, then enters the normal image pipeline.

```mermaid
flowchart LR
  Source["deck.md"] --> Fence["Mermaid fence"]
  Fence --> Runner["mmdc stdin"]
  Runner --> Cache[".peitho/code-images-cache"]
  Cache --> Slot["image slot"]
```

---
<!-- {"key":"graphviz-flow"} -->
# Graphviz uses the same contract

The `dot` entry is just another user-declared command. Graphviz emits XML,
comments, and a DOCTYPE before `<svg>`; Peitho accepts that real-world SVG
preamble.

```dot
digraph Peitho {
  graph [rankdir=TB, bgcolor="transparent", pad="0.2", nodesep="0.45", ranksep="0.55"];
  node [
    shape=box,
    style="rounded,filled",
    color="#64748b",
    fillcolor="#f8fafc",
    fontname="Inter",
    fontsize=18,
    margin="0.18,0.12"
  ];
  edge [color="#0f766e", penwidth=2, arrowsize=0.75];

  code [label="dot fence"];
  stdout [label="dot -Tsvg stdout"];
  cache [label="cached SVG"];
  image [label="image fragment"];

  code -> stdout -> cache -> image;
}
```

---
<!-- {"key":"before-after"} -->
# Before and after stay visible

The left pane is the same Mermaid graph shown as ordinary Markdown source.
The right pane is the matching fenced block after `code_images:` turns it into
an SVG image.

::: {slot=code}

````md
```mermaid
flowchart LR
  Write["write graph"] --> Build["peitho build"]
  Build --> Ship["ship SVG"]
```
````

:::

::: {slot=image}

```mermaid
flowchart LR
  Write["write graph"] --> Build["peitho build"]
  Build --> Ship["ship SVG"]
```

:::

---
<!-- {"key":"config-source"} -->
# The deck owns the commands

No diagram tool is built into Peitho. The deck declares which language tags are
commands, and each command receives the fenced source on stdin.

```yaml
---
layouts: ./layouts
css: ./css
code_images:
  mermaid: mmdc -i - -o - -e svg
  dot: dot -Tsvg
---
```
