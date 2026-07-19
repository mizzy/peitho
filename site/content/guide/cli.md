+++
title = "CLI"
weight = 50
template = "guide-page.html"
description = "Scaffold, preview, lint, present, export, and publish a deck — plus inspection commands and shell completions."
+++

Start a deck with `peitho new`; the day-to-day commands are `preview`,
`lint`, `present`, `export`, and `publish`. Each command that reads a deck takes a
deck path and defaults to `deck.md` in the current directory, so the argument
can be omitted when the file follows the convention:

```sh
peitho preview slides.md
```

## `peitho new`

Scaffold a starter deck into a directory (the current directory when omitted):

```sh
peitho new my-deck
```

The scaffold writes `deck.md`, `layouts/`, `css/base.css`, and a `.gitignore`.
Pick a layout variant with `--layouts default|split|cover` and a theme with
`--theme light|dark`. In a non-empty directory, `--force` overwrites the
scaffold-owned files and leaves everything else alone.

## `peitho preview`

Preview is the daily editing loop: watch, serve, open, and reload on every
successful rebuild.

```sh
peitho preview
```

It watches the deck and its assets, serves locally, and reloads while
preserving the current slide and overview state.

## `peitho lint`

Lint renders every slide in headless Chrome and warns when layout content
overflows the slide box by more than 1px horizontally or vertically.

```sh
peitho lint
```

Warnings include the slide number, axis, and overflow delta in pixels. The
command exits 1 when any overflow is found and 0 when the deck is clean. It
requires Chrome or Chromium, using the same discovery rules as PDF export and
`PEITHO_CHROME_PATH`.

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

Use a phone as a clicker by exposing the present server on a reachable IP:

```sh
peitho present --host 100.64.0.5
```

The local slides and presenter windows still use loopback. A specific
`--host <IP>` adds a listener for that address and prints exactly one
`/remote` URL; bare `--host` picks the best non-loopback address
automatically with VPN (e.g. Tailscale) preferred, then binds only that
address plus loopback. Wildcard binding is explicit via `--host 0.0.0.0` or
`--host ::`; with the bare form, a token immediately after `--host` is read
as the IP value, so use `peitho present deck.md --host` rather than
`peitho present --host deck.md`. Peitho renders a terminal QR code for the
top-ranked remote URL, and the top line plus QR prefer VPN (e.g. Tailscale)
when available.

For Add to Home Screen, run `peitho present --host` so the remote keeps a
stable `http://<ip>:6173/remote` URL. Scan the QR once, open the share sheet,
choose Add to Home Screen, and later `peitho present --host` runs reuse the
same home-screen URL. The remote opens full-screen without the Safari address
bar, in portrait or landscape, with iOS safe-area insets already accounted for:

<div class="remote-shots">

![Peitho remote in portrait: preview on top, speaker notes and stacked Previous/Next below](/guide-shots/remote-portrait.png)

![Peitho remote in landscape: preview on the left, notes in the center, Previous and Next on the right edge rail](/guide-shots/remote-landscape.png)

</div>

Rehearse a talk with `--rehearsal` on a deck that declares
`{"section":...}` markers, and Peitho records each section's actual time
into `.peitho/rehearsals/rehearsal-YYYYMMDD-HHMMSS.json` as you present:

```sh
peitho present --rehearsal
```

Records accumulate over runs (nothing is pruned automatically). During a
talk the agenda's live Actual / Planned and delta are enough for pacing;
review the recorded actuals afterward with `peitho rehearsal`.

## `peitho rehearsal`

Print the most recent rehearsal as an aligned section / planned / actual /
delta table with a total row:

```sh
peitho rehearsal
```

```
rehearsal-20260719-135241  (recorded 2026-07-19 13:52)

  section     planned   actual    delta
  Setup          1:00     0:52    -0:08
  Problem        1:00     1:10    +0:10
  Approach       2:00     1:45    -0:15
  Wrap-up        1:00     0:58    -0:02
  total          5:00     4:35    -0:15
```

Pass `--all` to list every record oldest first, one table per run:

```sh
peitho rehearsal --all
```

Records live in the current directory's `.peitho/rehearsals/`; the
command needs no deck argument. When there are no records it prints a
short pointer at `peitho present --rehearsal` and exits 0. A corrupt or
future-version record is a hard error naming the file so it can be
moved or deleted.

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

`peitho publish` itself prints nothing on success — the output you see comes
from the deploy command you passed after `--`, so you keep whatever progress
reporting that command already gives you.

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

## `peitho layouts`

Print the resolved layouts and their slot contracts, and explain layout
dispatch for a slide:

```sh
peitho layouts
peitho layouts --explain intro
```

`--json` prints the same information for programmatic use. See
[Layouts](@/guide/layouts.md) for the dispatch rules.

## `peitho doctor`

Diagnose the runtime environment — Chrome discovery, display enumeration, the
embedded shells, and (when the deck file exists) deck asset resolution — as
pass/warn/fail checks with remediation hints:

```sh
peitho doctor
```

`--json` emits machine-readable output. The exit code is non-zero when any
check fails; warnings (such as a single display) do not fail it.
