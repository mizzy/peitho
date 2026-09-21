# Guide demo videos (2026-09-21)

## Goal

Show the features whose point is motion — the preview editing loop, incremental
reveal, stepped code line emphasis — as short videos in the guide, and link
them from the README.

## Decisions

- **Recording**: `scripts/record-demo-videos.mjs` drives Playwright chromium
  with `recordVideo` against `peitho preview` / `peitho present --no-open` on a
  throwaway copy of an example deck (the preview tour edits the deck source).
  Playwright videos have no mouse pointer, so the script injects a dot that
  follows `mousemove`. The blank page before the first real frame is trimmed by
  measuring when the first frame is ready (+0.6s, because the encoder lags the
  page clock — measured: the bare offset left one black frame). ffmpeg converts
  webm → H.264 mp4 with `+faststart`.
- **Pacing**: after an inline edit commits, the tour waits for the rebuilt
  annotation to appear and then holds 3.5s, so the viewer can read the rendered
  result before the next action (author feedback).
- **Storage**: `site/static/guide-videos/*.mp4`, committed like `guide-shots/`
  (about 2.7 MB total). `make demo-videos` re-records by hand after a UI
  change; CI does not record (fonts and timing differ on Linux, and nothing
  depends on the bytes).
- **Embedding**: guide Markdown is also embedded in the binary for
  `peitho docs`, whose link rewriter only understands Markdown links. A video is
  therefore written as a paragraph that is only a link to
  `/guide-videos/<name>.mp4`; `guide-page.html` upgrades exactly that shape to
  an inline `<video controls muted playsinline preload="metadata">` with
  `regex_replace` at build time. No JavaScript on the site, and `peitho docs`
  prints a labeled absolute link.
- **README**: GitHub does not play repo-relative mp4s (only hand-uploaded
  `user-attachments` URLs), so the script also converts the videos the README uses (`readme: true`) to GIFs
  under `docs/images/` and the README embeds those (author decision: embed, do
  not link). 880px wide, 8fps, 64 colors, duplicate frames dropped — about
  1.3 MB for the 36-second preview tour, text still legible.

## Videos

| File | Deck | Shows |
| --- | --- | --- |
| `preview-demo.mp4` | `peitho-tour` | grid → single view → note edit (autosave) → list item edit with `**bold**` and a link → heading edit → grid |
| `reveal-demo.mp4` | `incremental-reveal` | reveal steps across the first two slides |
| `emphasis-demo.mp4` | `code-emphasis` | static emphasis, then stepped emphasis moving through a function |
