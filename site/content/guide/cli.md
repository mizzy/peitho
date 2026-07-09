+++
title = "CLI"
weight = 50
template = "guide-page.html"
description = "Run Peitho commands for building, previewing, presenting, exporting, publishing, and shell completions."
+++

## Default deck path

Commands default to `deck.md` in the current directory, so the deck argument can
be omitted when the file follows the convention.

Use an explicit path when the deck has another name:

```sh
peitho build slides.md
```

## `peitho build`

Build writes the distributable `dist/` directory with slide fragments,
`manifest.json`, `index.html`, and `peitho.css`.

```sh
peitho build
```

Rebuild on every save for an external server or pipeline:

```sh
peitho build --watch
```

## `peitho preview`

Preview is the daily editing loop: watch, serve, open, and reload on every
successful rebuild.

```sh
peitho preview
```

It watches the same deck and asset roots as `build --watch`, serves locally,
and reloads while preserving the current slide and overview state.

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
