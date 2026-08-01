# Lint font-size check — design

Date: 2026-08-01
Status: approved (author Q&A 2026-08-01)

## Goal

`peitho lint` warns when any slide text renders below the recommended minimum
font size of **24pt**. Small text is the most common way a deck becomes
unreadable from the back of the room; the existing overflow check catches
"too much content", this check catches "content shrunk to fit".

## Chosen approach

Extend the existing lint measurement pass (approach A). `lint_measure.js`
already runs each slide through headless Chrome and publishes per-slide
measurements over the console-log chunk channel; it additionally measures the
minimum computed font size of visible text per slide, and `lint.rs` gains a
second warning collector next to the overflow one.

Static CSS analysis (approach B) was rejected: it cannot resolve the cascade,
inheritance, or relative units (`em`, `%`, `clamp()`), so it cannot answer
"what size does this text actually render at" — which is the whole point.

## Author decisions (2026-08-01)

- **Scope**: all visible text, excluding only the footnote constructs that are
  intentionally small by construction: `.peitho-footnotes` (entries block) and
  `sup.peitho-footnote-ref` (body reference markers). No other exclusions —
  code blocks are checked (unreadable code is exactly what the check should
  catch).
- **Threshold**: fixed 24pt, not configurable (zero-config policy). Frontmatter
  can grow a key later if a real need appears.
- **Granularity**: one warning per offending slide, reporting the smallest
  font size found and a short excerpt of that text.

## Threshold semantics

CSS `24pt` = **32px** computed. The peitho canvas height is always 720px
(16:9 → 1280×720, 4:3 → 960×720), and 32/720 matches the classic
"24pt on a 7.5-inch-tall PowerPoint slide" ratio (24/540), so the px
threshold is stable and meaningful for both aspect ratios. The comparison
happens in computed px (`minFontSizePx < 32.0`, after rounding to 0.01px to
absorb float noise); the report displays pt (px × 0.75, one decimal max)
because that is the unit presenters think in.

### Amendment (2026-08-01, author-approved)

The warning decision is now made on the same pt value shown in the report.
Computed px is converted and rounded once to 0.1pt
(`round-to-0.1pt(px × 0.75)`), and a warning is emitted if and only if that
display value is below 24.0pt. This gives an effective computed-px threshold
of approximately 31.933px.

This replaces the 0.01px-rounded comparison against 32px above. The former
decision and display roundings always left a self-contradictory window in
which the report could say that text at "24pt" was below the recommended
24pt; deciding on the displayed value removes that window.

## Measurement (lint_measure.js)

For each `section.peitho-slide`, walk text nodes (not elements — font size is
a property of the text's parent at the point of rendering):

1. Skip a text node when its trimmed content is empty.
2. Skip it when `parent.closest(".peitho-footnotes, sup.peitho-footnote-ref")`
   matches inside the slide.
3. Skip it when the parent is not rendered: zero-size bounding rect (covers
   `display:none` subtrees) or `visibility: hidden`/`collapse` computed style.
4. Otherwise read the parent's computed `font-size` in px.

Per slide, record the minimum over surviving nodes and a sample of the text
at that minimum:

- `minFontSizePx: number | null` — `null` when the slide has no measurable
  text (image-only slides produce no font warning).
- `minFontSample: string | null` — the winning text node's content,
  whitespace-collapsed and truncated to 40 characters with a trailing `…`.

Both fields join the existing per-slide payload object (same base64 chunk
channel; the payload is ephemeral per-run, script and parser always ship
together, so no compatibility concern).

## Reporting (lint.rs)

`SlideOverflow` is renamed to `SlideMeasurement` — it now carries more than
overflow. A second collector produces font warnings alongside overflow
warnings:

```
warning: slide 3 has text at 18pt, below the recommended 24pt: "Some long caption that was shrunk to f…"
  help: raise the font size in the layout CSS, or move content to another slide instead of shrinking it
```

- pt values format with at most one decimal (`18pt`, `17.3pt`).
- The summary line unifies both kinds:
  `checked N slide(s): M warning(s)` (and `no warnings` when clean) —
  the current `no overflow` / `overflow warning(s)` wording changes.
- Exit code stays: any warning (either kind) → 1, clean → 0. Measurement
  transport failures remain hard errors, as today.

## Edge cases and constraints

- **Reveal decks**: the lint document renders final state (all reveal content
  visible by construction), so all text is measured.
- **Skipped/draft slides**: draft slides never reach Rendered; `skip` slides
  are real slides and are checked (they remain in PDF/publish output).
- **Text inside code_images SVGs** (mermaid/math/graphviz): rendered as
  `<img>`, not DOM text — unmeasurable and out of scope. Documented
  limitation; the math KaTeX HTML path (`FragmentKind::Math`) *is* DOM text
  and is measured.
- **Slide count validation** is unchanged: one measurement object per slide,
  count mismatch is a hard error.
- **No new Chrome run**: the check rides the single existing lint invocation;
  `--virtual-time-budget` and the font/image readiness waits already in
  `lint_measure.js` apply to font-size measurement too (fonts must settle
  before computed sizes are trusted; the existing `document.fonts.ready`
  bounded wait covers this).

## Files touched

- `crates/peitho-core/src/lint_measure.js` — font-size walk + payload fields
- `crates/peitho-core/src/render.rs` — lint-document tests asserting the new
  script markers
- `crates/peitho/src/lint.rs` — `SlideMeasurement` rename, font warning
  collector, report/summary wording, tests
- `docs/` — this design record and the implementation plan
