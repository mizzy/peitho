+++
title = "Getting Started"
weight = 10
template = "guide-page.html"
description = "Install Peitho, write a first deck, build it, preview it, and present it."
+++

## Install

Install with Homebrew on macOS or Linux:

```sh
brew install mizzy/tap/peitho
```

Homebrew installs shell completions for bash, zsh, and fish automatically.

For a prebuilt binary, download the tarball for your target from GitHub
Releases, verify the checksum, unpack it, and move the single `peitho` binary
onto your `PATH`. Layouts, the base theme, and the presentation shell are
embedded, so Node.js and npm are not runtime dependencies.

## Create a first deck

Create `deck.md` in an empty directory — or run `peitho new my-deck` to
scaffold a starter `deck.md` with layouts and a theme (see
[CLI](@/guide/cli.md#peitho-new) for the variants). This example is adapted from
`examples/minimal/deck.md`: it has a title, body text, a fenced code block, a
slide separator, and a speaker note written as an HTML comment.

````markdown
# Peitho Architecture

Markdown is the source of truth, while HTML and CSS own layout.

```rust
enum Phase { Parsed, Mapped, Checked, Rendered }
```

<!--
Open with the two-line pitch: separation of content and design,
then move to the pipeline.
-->

---

# Convention Mapping

- Shallowest heading maps to title
- Code blocks map to code
- Remaining blocks map to body
````

## Build once

Run a build from the directory that contains `deck.md`:

```sh
peitho build
```

The deck argument defaults to `deck.md`, so `peitho build` is the same as
`peitho build deck.md`. A build writes the distributable `dist/` directory with
slide fragments, `manifest.json`, `index.html`, and `peitho.css`.

## Preview while editing

Use preview for the daily editing loop:

```sh
peitho preview
```

Preview watches the deck and its assets, serves the deck locally, and reloads
the browser after each successful rebuild while preserving the current slide
and overview mode.

![peitho preview showing a single slide in the browser](/guide-shots/preview-single.png)

In preview, `o`, Enter, or Esc toggles single-slide and tile overview modes;
arrows move through slides and overview tiles, and clicking a tile opens it.

![The overview view: every slide as a tile in a scrollable grid](/guide-shots/preview-overview.png)

## Present

Use present when you are ready to speak:

```sh
peitho present
```

`peitho present` opens the slides full-screen and opens a presenter view with
current and next slides, speaker notes, and a timer. The agenda and planned-time
scale appear when the deck declares sections and time; see
[Writing Decks](@/guide/writing-decks.md) and
[Frontmatter](@/guide/frontmatter.md) for those settings.

![Presenter view with current and next slides, notes, a timer, and a per-section agenda](/guide-shots/presenter-view.png)

For debugging, open the presenter in a normal window instead of full-screen:

```sh
peitho present --presenter-windowed
```

## Drive the deck from your phone

To use a phone as a clicker, run:

```sh
peitho present --host
```

Peitho binds `/remote` on your LAN with a stable `:6173` port, prints a terminal
QR code, and prefers a VPN address (such as Tailscale) when one is available.
Scan the QR once in Safari, use the share sheet's Add to Home Screen action, and
later `peitho present --host` runs reuse the same home-screen URL. See
[`peitho present`](@/guide/cli.md#peitho-present) for `--host <IP>` binding and
`--port` overrides.

<div class="remote-shots">

![Peitho remote in portrait: preview on top, speaker notes and stacked Previous/Next below](/guide-shots/remote-portrait.png)

![Peitho remote in landscape: preview on the left, notes in the center, Previous and Next on the right edge rail](/guide-shots/remote-landscape.png)

</div>

Next, learn the deck syntax in [Writing Decks](@/guide/writing-decks.md);
for diagrams-as-code, see [Frontmatter](@/guide/frontmatter.md#code-images).
