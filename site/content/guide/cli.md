+++
title = "CLI"
weight = 50
template = "guide-page.html"
description = "Scaffold, preview, lint, present, export, and publish a deck — plus inspection commands, offline docs, and shell completions."
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

It watches the deck, its referenced images, and its assets, serves locally,
and reloads while preserving the current slide and overview state. The
single-slide view keeps a filmstrip of thumbnails on the left and shows the
current slide's speaker notes in an editable panel below the slide, headed by
the slide's position (`3 / 25`); the overview grid shows neither. See
[Editing speaker notes in preview](#editing-speaker-notes-in-preview). Every thumbnail
and grid tile carries its slide number in the bottom-left corner. These
numbers are preview chrome only; a page number on the slide itself comes from
the `page_numbers` frontmatter key.

`--port <PORT>` pins the server port, which is otherwise an ephemeral port
chosen at startup. `--no-open` starts the server without launching a browser —
useful when a browser is already pointed at a pinned port, or when nothing
should open a window:

```sh
peitho preview --port 5173 --no-open
```

### Editing speaker notes in preview

The notes panel is an editable textarea. Type into it and the note is saved
when the panel loses focus (click away), when you move to another slide, and
when the page is reloaded or closed. While the textarea has focus, arrows,
Home, End, and every other key edit text; only PageUp and PageDown still
change slides (the note is saved first), and Esc leaves the textarea so that
a second Esc enters the overview. The other direction is symmetric: Enter in
the overview opens the selected slide, and Enter in the single-slide view
focuses the textarea with the caret at the end, so the whole loop works
without a mouse. A slide change (or entering the overview)
waits for the save and is cancelled if it fails, so unsaved text is never left
behind; the reason appears in red next to the position line and the text
stays in the panel until a later save succeeds. A rebuild triggered by the
save reloads the preview with the caret and focus where they were.

What a save writes:

- A slide without a note comment gets one appended after its last non-blank
  line, separated by a blank line. A single-line note becomes `<!-- text -->`,
  a multi-line note becomes `<!--` / text / `-->`.
- If the slide's first note comment sits on its own line, not indented, it
  is replaced in place. A first comment anywhere else (inline in a paragraph,
  indented under a list item, or inside a blockquote) is removed and the note
  is appended at the end of the slide instead.
- Any further note comments in the slide are also removed, so several
  note comments collapse into one after the first save. Where a removed
  comment sat between two non-blank lines, a blank line (a bare `>` inside a
  blockquote) is left in its place so the neighbours are never joined.
- Leading and trailing whitespace is trimmed; an empty or whitespace-only
  note removes the comment and inserts nothing. Saving the same text twice
  leaves the file byte-identical.
- A CRLF file stays CRLF.
- A slide that comes from an `include` is written to the included file, not
  to the deck that includes it.
- Text containing `-->` cannot be represented in a comment, and text starting
  with `{` would be read as page settings on the next build; both are
  refused, and the textarea keeps the draft.
- While the deck does not build (an edit in progress in your editor), a save
  is refused and the reason is shown. The text stays in the panel, survives
  the reload that the next successful build triggers, and is saved on the
  next click-away or slide change.

Edits go to the Markdown source only; notes never enter `dist/`.

## `peitho lint`

Lint renders every slide in headless Chrome and warns when layout content
overflows the slide box by more than 1px horizontally or vertically. It also
warns when non-footnote text renders below the recommended 24pt.

```sh
peitho lint
```

Overflow warnings include the slide number, axis, and overflow delta in pixels.
Text truncated by CSS `text-overflow` is reported as a `note:` and does not
affect the exit code.
Font-size warnings appear once per slide and report the smallest size in pt
with a short excerpt. The command exits 1 when either warning kind is found and
0 when the deck is clean. It requires Chrome or Chromium, using the same
discovery rules as PDF export and `PEITHO_CHROME_PATH`.

Text that is small on purpose, such as a caption or a source line, can lower
its own floor from the layout CSS with `--peitho-lint-min-font-size`. The
property inherits, so it can target a slot, one slide through a keyed
selector, or the whole deck:

```css
.slot-caption { font-size: 14pt; --peitho-lint-min-font-size: 12pt; }
[data-slide-key="stats"] .slot-body { --peitho-lint-min-font-size: 16pt; }
```

Text at or above its floor is reported as a `note:` and does not affect the
exit code; text below the floor is still a warning that names the floor. The
value is a length in `pt` or `px`, or `0` to accept any size; any other value
stops lint with an error naming the slide.

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

### Keys during a talk

| Key | Action |
| --- | --- |
| Space | Next step; in the presenter, starts or pauses the timer |
| Arrows, PageUp / PageDown | Previous and next |
| Home / End | First and last slide |
| `f` | Fullscreen the current window |
| `S` | Swap the slides and presenter displays |
| Esc | Close the presentation and stop the server |

`S` is the escape hatch for a misidentified display: each window navigates to
its counterpart, so the windows stay where they are and only their roles swap.
The presenter also exposes it as a Swap button. After a swap the slides window
sits windowed, so press `f` to go back to fullscreen; the presenter timer
resets. The shortcut is available only while the presenter is open, so a solo
slides window cannot swap itself away.

Keys combined with Cmd, Ctrl, or Alt are ignored, so browser shortcuts such as
Cmd+F keep their usual meaning.

### Controlling what opens

| Flag | Effect |
| --- | --- |
| `--port <PORT>` | Pin the server port. Plain local runs otherwise use a random port; with `--host` the port is fixed at 6173. |
| `--no-open` | Start the server without launching Chrome. |
| `--no-presenter` | Open the slides window only, without the presenter view. |
| `--no-serve` | Build the present cache and exit without serving. |
| `--shell <PATH>` | Swap in a different present shell bundle. A development and debugging override; the built-in shell ships with the binary. |

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

### Laser pointer

The remote's Off / Pointer toggle turns its slide preview into a laser pointer.
In Pointer mode, dragging a finger across the preview moves a pointer dot on the
slide display; lifting the finger clears it. Switch back to Off to use the
preview normally.

Set the dot's color with the deck's
[`pointer_color`](@/guide/frontmatter.md) frontmatter key:

```yaml
pointer_color: "#38bdf8"
```

Rehearse a talk with `--rehearsal` on a deck that declares
`{"section":...}` markers, and Peitho records each section's actual time plus
an absolute per-slide timeline into
`.peitho/rehearsals/rehearsal-YYYYMMDD-HHMMSS.json` as you present:

```sh
peitho present --rehearsal
```

When a run starts in this presenter or is adopted at `0:00`, the timeline
records the current slide at `0:00`. If the presenter adopts an already-running
timer, its first entry is the current slide at the adopted timer position.
After that, the timeline records the timer position whenever a different slide
is entered. Reveal steps add no entry; returning to a slide adds another entry,
so time from all visits can be totalled later. Reset clears both timing
accumulators for the current record. Every saved snapshot is the complete
absolute state, and the server checks each recorded slide index/key against the
deck it built before writing it.

Records accumulate over runs (nothing is pruned automatically). During a talk
the agenda's live Actual / Planned and delta are enough for pacing; review the
recorded actuals afterward with `peitho rehearsal`.

## `peitho rehearsal`

Print the most recent rehearsal as an aligned section / planned / actual /
delta table with a total row, followed by per-slide timing:

```sh
peitho rehearsal
```

```
rehearsal-20260719-135241  (recorded 2026-07-19 13:52)

  section     planned   actual    delta
  Setup          1:00     0:52    -0:08
  Problem        1:00     1:10    +0:10
  Approach       2:00     1:45    -0:15
  Wrap-up        1:00     0:48    -0:12
  total          5:00     4:35    -0:25

  slide   key        entered   visits   total
  #1      setup         0:00        1    0:48
  #2      problem       0:48        1    1:07
  #3      approach      1:55        1    2:40
  total                                  4:35
```

`key` is the recorded slide key, `entered` is the timer position of its first
entry, and `visits` is the number of timeline entries for that slide. `total`
adds every visit to the recorded index/key, ending each visit at the next
timeline entry and the final visit at the saved elapsed time. If the presenter
adopted an already-running timer, a `(before first entry)` row accounts for the
leading gap. The final slide-table total therefore matches the run's elapsed
time. The command uses the recorded index and key without reopening the deck,
so later title or deck edits cannot rewrite history. A reset record with no
entries prints `(no slide entries)`. Version 1 records created by older Peitho
releases remain readable and keep their original section-only output exactly.

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
peitho export pdf
```

`-o` / `--out` is optional; without it the PDF takes the deck's own path with a
`.pdf` extension, so `deck.md` becomes `deck.pdf`:

```sh
peitho export pdf slides.md -o handout.pdf
```

Page size comes from the deck's [`resolution`](@/guide/frontmatter.md)
frontmatter key when set. Export needs Chrome or Chromium, using the same
discovery rules as `peitho lint` and `PEITHO_CHROME_PATH`.

## `peitho publish`

Publish inspects the built output, then delegates deployment to a command you
already use.

```sh
peitho publish -- aws s3 sync dist/ s3://your-bucket/
```

`peitho publish` itself prints nothing on success — the output you see comes
from the deploy command you passed after `--`, so you keep whatever progress
reporting that command already gives you.

`--dist <DIR>` inspects a directory other than `dist`. The inspection is a
contamination check: it fails if presentation-shell or speaker-notes files
reached the distributable output, so a deploy never ships notes.

The deploy command runs with `PEITHO_DIST` set to the inspected directory, so a
script can find the built output without hardcoding a path — useful together
with `--dist`:

```sh
peitho publish -- sh -c 'aws s3 sync "$PEITHO_DIST" s3://your-bucket/'
```

## `peitho docs`

This guide is embedded in the binary, so it is readable offline and by agents
driving Peitho without network access. With no argument, `peitho docs` lists the
topic slugs and their descriptions:

```sh
peitho docs
```

Pass a slug to print one page as plain Markdown on stdout, or `--all` to print
every page in guide order:

```sh
peitho docs writing-decks
peitho docs --all
```

Output is unpaged plain Markdown with no ANSI escapes, so it pipes cleanly into
a pager, a file, or another tool. An unknown topic exits non-zero and lists the
valid slugs.

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

`--watch` rebuilds on every change to the deck, its referenced images, or its
assets. `--out <DIR>` writes somewhere other than `dist`.

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
