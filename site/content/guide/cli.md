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
[Editing speaker notes in preview](#editing-speaker-notes-in-preview); text on
the slide itself can be fixed in place too — see
[Editing slide text in preview](#editing-slide-text-in-preview), or
[Editing a whole slide in preview](#editing-a-whole-slide-in-preview) to rework
the entire slide body at once. Every thumbnail
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

[Watch the 35-second preview tour: overview, single-slide view, editing a speaker note, and editing slide text in place](/guide-videos/preview-demo.mp4)

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

### Editing slide text in preview

Click a paragraph, heading, tight list item, or table cell in single view to
replace its rendering with its inline Markdown source. Enter or blur saves,
Shift+Enter inserts a newline, and Escape cancels. Markdown is the only source
of truth; Peitho never converts rendered HTML back to Markdown.

It is meant for the small fixes — a typo, a reworded sentence — that are not
worth a trip to the editor. The clicked block shows exactly what is in the
file (`Peitho is a *fast* tool`), so inline syntax such as `**bold**`,
`` `code` ``, and links can be added, changed, and removed. A footnote
reference is editable text too, but the deck must still build: removing the
last reference to a footnote, or referencing one that has no definition, is
refused with the parser's own message. The save writes that text back into the deck, the ordinary rebuild
runs, and the preview reloads on the same slide.

- **Where it works.** The single-slide view, on the current slide only. The
  overview grid keeps its click-to-open meaning and the filmstrip thumbnails
  stay inert. A click on a link still opens the link instead of starting an
  edit.
- **Keys.** Enter saves and Shift+Enter inserts a newline (with
  `breaks: true` that is a visible line break). Escape cancels and puts the
  rendered block back — deliberately unlike the notes panel, where Escape
  keeps the text, because a click on a slide is easier to make by accident.
  While a block is being edited the other preview shortcuts are off; only
  PageUp and PageDown still change slides, and they save first. The Enter
  that confirms an IME conversion never saves.
- **Text only, not structure.** An edit may change the words and the inline
  syntax of that one block. Anything that would change the slide's structure
  is refused with the reason and the edit stays open: emptying the block,
  starting it with a list or heading marker, a blank line that would split
  it, a newline inside a heading or a table cell, a slide separator, a `:::` fence, a note comment, a `|` that would add
  a table cell, or leading spaces that would re-nest a list. Make structural
  changes in your editor.
- **A failed save blocks the way out.** The reason appears next to the
  position line, the text stays in the block, and changing slides or entering
  the overview is cancelled until the save succeeds or Escape cancels the
  edit — the same rule as the notes panel.
- **Headings and slide keys.** The heading's level cannot be edited (the `#`
  markers are outside the editable text). Editing a heading may change that
  slide's derived key; an explicit `{"key": …}` never changes. If a keyed CSS
  selector still names the old key, the next build fails — see the banner
  below.
- **Build errors show in the page.** When a rebuild fails while the preview is
  open — after an inline edit or after a save from your editor — the
  diagnostic appears in a banner across the top and the last good build stays
  on screen underneath. The banner disappears with the next successful build.
- **Your editor wins.** A rebuild that arrives while a block is open does not
  reload the page and throw the text away; the reload waits until the edit is
  saved or cancelled. If the file really changed under the edit (your editor,
  or a second preview tab), the save is refused with "the deck changed on
  disk; reload and retry": press Escape and the preview reloads with the
  current text.
- **What is written.** Only the bytes of that block. A CRLF file stays CRLF,
  a leading BOM is kept, and a block that comes from an `include` is written
  to the included file, not to the deck that includes it.

Not editable, by design: code blocks, generated `code_images` output
(diagrams, math, embeds), images, footnote definitions, raw HTML, page
settings and frontmatter, any block that contains a speaker-note comment
(edit the note in the notes panel) or other inline HTML, and anything that comes from the layout HTML
rather than the Markdown. Inline editing exists only in `peitho preview`;
`peitho present`, `peitho build`, and `dist/` never see it.

### Editing a whole slide in preview

Press `e` in single mode to replace the rendered slide with its body Markdown.
Cmd/Ctrl+Enter or blur saves; plain Enter inserts a newline; Escape cancels.

Where clicking a block is for a typo, this is for reworking the slide: the
textarea holds the whole body, so you can add a list, split a paragraph, move
a code block, or rewrite the slide from scratch. The caret starts at the
beginning of the text, and changing slides saves first, like the notes panel.

- **Body only.** The textarea contains the slide's body Markdown and nothing
  else. The page settings comment and the speaker notes are not in it, are not
  shown, and cannot be changed from here — edit notes in the panel below and
  settings in your editor. A note comment that sits in the middle of the body is
  spliced out of the text you see, and the body is shown with LF line endings
  and without its leading and trailing blank lines, whatever the file uses — the
  original line endings and a leading BOM are restored when it is written back.
- **Where it works.** The single-slide view, on the current slide. The overview
  grid has no whole-slide editor.
- **When `e` does nothing.** It is a no-op while another edit is open or a slide
  change is still settling. Some slides cannot be edited this way at all, and
  those say so next to the position line instead of opening: a deck containing a
  lone CR (an old Mac line ending) makes every slide unavailable, since Peitho
  will not guess how to rewrite it, and a slide whose recorded spans no longer
  match the source asks you to reload. Inline block editing and the notes panel
  still work on such a deck.
- **Structure is allowed, inside one slide.** Unlike a click-to-edit block, this
  edit may change the slide's structure freely. What it may not do is change the
  deck around it: adding or removing a slide separator, changing the sections, or
  touching another slide's content is refused, and so is anything that would
  alter this slide's own settings or notes. Every refusal is reported with its
  reason and the editor stays open with your text.
- **Markdown that does not parse is refused** with the parser's message (422),
  and nothing is written. Markdown that parses but fails a later check — two
  code blocks where the layout allows one, say — is written to the file, and
  then the rebuild fails: the last good slide stays on screen with the error
  banner above it. Press `e` again and the editor reopens with the body you
  saved, not the one still rendered, so you can fix it in place.
- **Headings and slide keys.** Editing the heading may change a derived key, and
  the save answers with the key the deck now has. The editor keeps using that
  key, so if the rebuild is failing — a keyed CSS selector still naming the old
  key, for instance — the next save from the same stale page still lands on the
  right slide and can repair it.
- **Two known tradeoffs.** The notes panel beside a renamed slide can still show
  the previous key's note until the next successful rebuild. And after an inline
  edit or an external change followed by a failed rebuild, a save against the
  stale source map returns an honest 409 ("the deck changed on disk; reload and
  retry") rather than writing over something it cannot see; the open editor keeps
  your draft and the message until a successful generation reload.
- **What is written.** The slide's body bytes. A CRLF file stays CRLF, a leading
  BOM is kept, and a slide that comes from an `include` is written to the
  included file, not to the deck that includes it.

Like the other preview editors, this exists only in `peitho preview`.

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
sits windowed, so press `f` to go back to fullscreen. The timer keeps running:
the new presenter page adopts the timer position the server holds. What does
start over is everything the old presenter page measured itself — the agenda's
per-section actuals and, under `--rehearsal`, the slide timeline — so a swap in
the middle of a rehearsal leaves a record that only describes the part after
the swap (the time before it shows up as `(before first entry)`). With
`--audio` the swap also starts a new recording that replaces the old one, and
the other window belongs to a different Chrome profile, which needs its own
microphone permission: swap before you start a rehearsal, not during one. The
shortcut is available only while the presenter is open, so a solo slides
window cannot swap itself away.

Keys combined with Cmd, Ctrl, or Alt are ignored, so browser shortcuts such as
Cmd+F keep their usual meaning.

### Controlling what opens

| Flag | Effect |
| --- | --- |
| `--port <PORT>` | Pin the server port. Plain local runs otherwise use a random port; with `--host` or `--audio` the port is fixed at 6173. |
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

Add explicit microphone recording with both flags:

```sh
peitho present --rehearsal --audio
```

`--audio` requires `--rehearsal` and a presenter view. It is also the privacy
boundary: without it, the presenter does not touch the browser media APIs and
the server does not accept audio. With it, the presenter asks for microphone
permission immediately and shows one compact status beside the clock. Browser
microphone permission is scoped to the complete origin, including its port, so
audio mode defaults to stable port 6173 rather than a random port. An explicit
`--port` wins. If 6173 is occupied, Peitho reports that another presentation is
probably running and asks you to pass `--port`; it never silently changes the
origin.

The timer uses one state pill with a `REC` segment. The segment's dot reports
capture state without inheriting the timer state's color:

- hollow, dim dot — waiting for microphone permission or microphone ready
- pulsing warning-color dot — timer running and audio recording
- pause-color dot — timer paused and recording paused

Any error changes the segment label to `ERR`. Its full reason is clamped to two
lines above the pill, with the unclamped text available on hover, and is
positioned out of flow so the clock row never changes height:

- `ERR` plus `mic unavailable: ...` — permission, device, or recorder failure;
  the dot is hollow
- `ERR` plus `audio upload failed: ... (retrying)` — capture continues, the dot
  keeps its recording or paused color, the current chunk is retained and
  retried, and Peitho never skips ahead to a later chunk
- `ERR` plus `audio upload failed: server returned N` — a `400`, `404`, or `413`
  response cannot succeed unchanged, so the chunk remains queued and that take
  stops retrying until reset
- `ERR` plus `audio upload failed: another window took over the recording (retrying)`
  — another presenter owns the current take
- `ERR` plus `audio upload failed: recording out of order; restart the run (retrying)`
  — the current take's sequence no longer matches the server

Below a 440px clock-card width, the state word hides while the `REC`/`ERR`
segment remains visible so it cannot overlap the non-wrapping timer.

When a run starts in this presenter or is adopted at `0:00`, the timeline
records the current slide at `0:00`. If the presenter adopts an already-running
timer, its first entry is the current slide at the adopted timer position.
After that, the timeline records the timer position whenever a different slide
is entered. Reveal steps add no entry; returning to a slide adds another entry,
so time from all visits can be totalled later. Reset clears both timing
accumulators for the current record. In audio mode it also stops the current
take and removes that session's WebM; the next timer start begins a fresh take.
Every saved snapshot is the complete absolute state, and the server checks each
recorded slide index/key against the deck it built before writing it.

Once the server accepts the first audio chunk, the files share one stem:

```text
.peitho/rehearsals/rehearsal-20260918-120000.json
.peitho/rehearsals/rehearsal-20260918-120000.webm
```

Audio start, pause, resume, and reset follow the timer's actual state. A take
can nevertheless begin after timer position zero when the presenter opens late
or microphone permission is still pending. The record therefore stores
`audio.startMs`: for an entry at or after that offset, the WebM seek position is
`atMs - audio.startMs`; earlier entries have no audio. When the offset rounds
to at least one second, `peitho rehearsal` prints it and a ready-to-use `seek`
column beside the WebM path. Chrome's WebM carries no duration metadata;
`ffmpeg -i in.webm -c copy out.webm` adds it for players that require one. The
JSON and WebM are rehearsal data and are never copied into `dist/`.

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

  slide   key        entered      seek   visits   total
  #1      setup         0:00      0:00        1    0:48
  #2      problem       0:48      0:40        1    1:07
  #3      approach      1:55      1:47        1    2:40
  total                                           4:35
  audio   .peitho/rehearsals/rehearsal-20260719-135241.webm
  offset  0:08
```

`key` is the recorded slide key, `entered` is the timer position of its first
entry, `seek` is the ready-to-use position in the WebM, and `visits` is the
number of timeline entries for that slide. A `-` seek means the slide's first
visit ended at or before the audio began; when audio begins during that visit,
the seek is `0:00`. For example, jump directly to Approach with:

```sh
ffplay -ss 1:47 .peitho/rehearsals/rehearsal-20260719-135241.webm
```

The terminal intentionally shows only each slide's first entry. Every revisit
is retained in the JSON `timeline`; calculate its audio position as
`atMs - audio.startMs`. `total` adds every visit to the recorded index/key,
ending each visit at the next timeline entry and the final visit at the saved
elapsed time. If the presenter adopted an already-running timer, a
`(before first entry)` row accounts for the leading gap. The final slide-table
total therefore matches the run's elapsed time. The command uses the recorded
index and key without reopening the deck, so later title or deck edits cannot
rewrite history. A reset record with no entries prints `(no slide entries)`.
Version 1 records created by older Peitho releases remain readable and keep
their original section-only output exactly. The `audio` line appears only when
that v2 record has accepted audio; rehearsal runs without audio and older
records omit it. An offset that rounds to at least one second adds the `seek`
column and `offset` line. If the record names its correct same-stem WebM but the
file is missing, the command prints a `note:` naming that path. A mismatched
audio filename instead makes the record corrupt and produces a file-named hard
error without opening an unrelated path.

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
