+++
title = "CLI"
weight = 50
template = "guide-page.html"
description = "Preview, present, export, and publish a deck — plus shell completions."
+++

Peitho's day-to-day commands are `preview`, `present`, `export`, and `publish`.
Each command takes a deck path and defaults to `deck.md` in the current
directory, so the argument can be omitted when the file follows the convention:

```sh
peitho preview slides.md
```

## `peitho preview`

Preview is the daily editing loop: watch, serve, open, and reload on every
successful rebuild.

```sh
peitho preview
```

It watches the deck and its assets, serves locally, and reloads while
preserving the current slide and overview state.

## `peitho present`

Present generates a volatile cache, starts a local server, launches the browser,
and places full-screen slides plus the presenter view across displays.

```sh
peitho present
```

Use windowed presenter mode while debugging:

```sh
peitho present --presenter-windowed
```

## `peitho export`

Export a PDF:

```sh
peitho export pdf -o deck.pdf
```

## `peitho publish`

Publish inspects the built output, then delegates deployment to a command you
already use.

```sh
peitho publish -- aws s3 sync dist/ s3://your-bucket/
```

## `peitho completions`

Generate shell completion scripts for bash, zsh, fish, powershell, or elvish.

```sh
peitho completions zsh
```

## `peitho build`

`peitho build` is a lower-level command that writes the distributable `dist/`
directory. The daily commands above invoke it internally, so authors rarely
call it directly. Use it when you need a one-shot build for an external
pipeline:

```sh
peitho build --watch
```
