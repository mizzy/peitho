<!-- {"key":"cover"} -->
# Draft and skip flags

Per-slide page settings for decks that are still moving.

---
<!-- {"key":"skipped","skip":true} -->
# A skipped slide

- Present and preview `next`/`prev` step over this slide
- Direct navigation — number keys, Home/End, the preview grid — can still land on it
- It stays counted and remains in build output, publish, and PDF export

<!-- Skipped slides keep their speaker notes: you are reading one right now, in the presenter, after navigating here directly. -->

---
<!-- {"key":"draft","draft":true} -->
# A draft slide

This slide never leaves the parser. It is dropped at parse end, so it appears in no output at all — not in build, preview, present, publish, or PDF export. Build this deck and count the slides.

---
<!-- {"key":"rules"} -->
# The rules

- `draft` and `skip` on the same slide is a build error
- `draft` combined with a `section` marker is a build error
- Marking every slide draft is a build error
- `skip` rides into `manifest.json` (`"skip": true`); `draft` never reaches any phase downstream of the parser
