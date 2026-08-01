# lint: detect slot-level overflow

Issue: #385
Date: 2026-08-01
Branch: `issue-385-slot-overflow`

## Problem

`peitho lint` reports "no overflow" for slides that are visibly clipping content. Slot containers carry `overflow: hidden`, so `getBoundingClientRect()` of a clipped descendant is clamped to what is painted and never expands the slide bounds. `contentBounds` only ever compares against the slide box, so the check fires only once content escapes the *slide* — which happens at extreme volumes (the existing test uses 80 paragraphs), not when a slot quietly hides its own content.

Measured on the built output of a 12-bullet deck (default theme and layout): `.body` clientHeight 469px, scrollHeight 486px, **17px hidden**, one whole bullet invisible, slide box unchanged at 823px. Lint reports no overflow.

## Author decisions (2026-08-01)

| Question | Decision |
| --- | --- |
| Excluding intentionally-hidden elements | Generic visual-hiding predicate, not a `katex-mathml` class carve-out |
| Severity | Warning, alongside the existing overflow and font-size warnings — not an error |
| Scope of measurement | Every clipping element under the slide, not only `.slot-*` |

### Why a generic predicate rather than a class name

KaTeX hides its accessibility MathML copy with `clip: rect(1px,1px,1px,1px)` on a 1×1px box, so it is clipped *by design*: all three math example slides report 98-107px of "overflow" that no one can see. Excluding it by class name would work, and the existing font-size check does exactly that for `.peitho-footnotes` — but that selector names a structure **peitho itself generates**, whereas `katex-mathml` is a third-party library's internal markup. It would break silently if KaTeX renames it, and the next library using the same accessibility pattern (`sr-only` and friends) would need another carve-out.

The thing being excluded is not "KaTeX's MathML"; it is "elements that are visually hidden". Testing that property directly covers KaTeX, `sr-only`, and anything future without lint knowing any library's names.

### Why not restrict measurement to `.slot-*`

Restricting to slot elements would also dodge the KaTeX false positive, and the one true positive found so far (`peitho-tour` slide 11) is a `slot-code`. But it would miss an intermediate container between the slot and the content — and a missed clip is precisely the bug being fixed here.

### Why warning rather than error

Pillar ③ forbids the *build* silently dropping content: a parser swallowing unknown structure, a slot contract violation ignored. Those are peitho's own decisions and are determinate. Slot overflow is a **browser rendering outcome** — it varies with fonts, theme CSS, and environment, and can be deliberate (a long log shown boxed). It belongs with the existing overflow and font-size warnings, which are the same kind of judgement.

Practically: `peitho-tour` slide 11 already clips 14px of a code block. Making this an error would mean "fixing the linter broke an existing deck", forcing the deck fix into the same PR. As a warning, detection lands first and deck fixes stay a separate judgement. (This PR fixes that slide anyway — the detection finding it is the feature working.)

## Design

### 1. Measure clipping per element (`lint_measure.js`)

For every element under the slide, compare `scrollHeight`/`scrollWidth` against `clientHeight`/`clientWidth`, but only when the computed `overflow` on that axis actually clips (`hidden`, `clip`, `auto`, `scroll` — `visible` does not clip, and reports rounding noise, see tolerance below).

**`clip` was added during review**: it is a real CSS value in the same family as `hidden` and was missing from the first list, so a `.body { overflow: clip }` holding twelve bullets clipped them all while lint reported nothing.

**`auto`/`scroll` stay in scope but get their own help text.** Two shipped examples declare scrollable regions deliberately (`examples/code-images` and `examples/two-column`), and telling their author to "shrink or split the slide content" would be wrong advice for a choice they made on purpose. But the content is still lost on a slide — nobody scrolls during a talk, and in PDF it is simply gone — so the finding is real. The message became: *a scrollable region cannot be scrolled in a printed or projected deck, so content past the edge will not be seen*.

**The size guard is per axis, not per element.** The first version skipped the whole element when *either* dimension was ≤1px, which discarded the other axis's genuine overflow. Measured: `.body { width: 0; height: 200px; overflow: hidden }` clipped twelve bullets vertically and produced no warning at all — and a slot squeezed to zero width by an over-wide sibling is the most common two-column failure, which the base theme's `min-width: 0` on `.body`/`.code`/`.footnotes` exists to permit. Gating each axis on its own dimension fixes it while keeping KaTeX excluded (its box is 1×1, so both axes are skipped).

Skip elements that are visually hidden:

```js
function isVisuallyHidden(el, style) {
  // Accessibility copies such as KaTeX's MathML are collapsed to a 1x1px box.
  return el.clientWidth <= 1 || el.clientHeight <= 1
      || style.visibility === "hidden" || style.visibility === "collapse";
}
```

**Revised during review**: the first version also tested `style.clip !== "auto"` and `style.clipPath !== "none"`. Both were removed after measurement showed they created the very blind spot this issue exists to fix. A deck reporting `overflows the body slot vertically by 17px` reported **nothing at all** once its `.body` carried `clip-path: inset(0 round 12px)` — ordinary rounded corners — because the element was skipped as "hidden". `clip-path` is a mainstream decorative tool on fully visible elements.

Measuring what the predicate actually needs for KaTeX settled it: its MathML box is `clientWidth: 1, clientHeight: 1, clipPath: "none"`. The **size test alone** excludes it, and the `clipPath` branch protected nothing while costing real detection. Removing both also makes the predicate honest about the argument above — with `clip !== "auto"` in it, this was KaTeX's specific technique wearing a generic name. Size-plus-visibility is genuinely general. A regression test pins the `clip-path` case.

Report the worst offender per slide per axis, carrying the slot name for the message. The slot name comes from the nearest ancestor-or-self with a `slot-*` class (the rendered slot wrapper); elements outside any slot report no name and the message omits it.

New optional payload fields on `SlideMeasurement`, so an older payload still deserializes:
`slotOverflowAxis`, `slotOverflowPx`, `slotName`.

**Revised during implementation**: these ride as an array (`slotOverflows`, serde-defaulted to empty) rather than three flat fields, because the worst offender is tracked *per axis* — a slide clipping both horizontally and vertically produces two entries. Three flat fields could only carry one of them, which would have silently dropped half the finding: the same failure shape this issue exists to fix.

**Slot-name resolution is self → ancestors → descendants**, not "nearest ancestor-or-self" as first written here. The default layout renders `<div class="body">` (which carries `overflow: hidden` and therefore clips) wrapping `<div class="slot-body">` (which carries the name), so an ancestor-only walk finds nothing on the most common layout of all. Measured: before the fix the warning read "a container"; after, it reads ``the `body` slot``. Note this makes the descendant leg the **primary** path for shipped themes, not a rare fallback — `themes/base.css` puts `overflow: hidden` on `.body` and `.code` while the slot elements are their children.

**The descendant leg names a slot only when exactly one is found.** First-descendant-wins was rejected: `examples/two-column` has `<div class="columns">` wrapping both `.slot-left` and `.slot-right`, so a theme putting `overflow: hidden` on a grid or flex row wrapper — an ordinary thing to do — would report a clip in `right` as belonging to `left`. A confidently wrong pointer is worse than none, and the message already has a correct unnamed form. Verified by forcing that clip: the warning reads "a container", not a guessed slot.

### 2. Collect and report (`lint.rs`)

A third warning kind next to `OverflowWarning` and `FontSizeWarning`:

```rust
struct SlotOverflowWarning {
    slide: usize,
    axis: OverflowAxis,
    overflow_px: i64,
    slot: Option<String>,
}
```

`OverflowAxis` and its `adverb()` are reused as-is. The summary line (`checked N slide(s): M warning(s)`, unified in #377) needs no change — the count simply includes the new kind.

Message:

```
warning: slide 11 content overflows the `code` slot vertically by 14px
  help: shrink or split the slide content, or adjust the layout CSS
```

Without a slot name: `... overflows a container vertically by 14px`.

`OVERFLOW_HELP` is reused — the remedy is identical.

### Known limitation: the reported number includes trailing margin

`scrollHeight - clientHeight` measures the *scrollable overflow area*, which in Chrome includes the bottom margin of the last in-flow child. That margin paints nothing, so the reported pixel count overstates how much text is actually hidden.

Measured on a realistic 8-paragraph deck against the unmodified default theme: lint reports **47px** while only **24px** of painted text sits below the visible edge — the remaining 23px is the `.slot-body p { margin: 0 0 24px }` the theme ships on exactly the element `.body` clips.

The detection itself is correct in that case (content genuinely *is* clipped), so this is a magnitude error, not a false positive. But the same mechanism *can* produce a pure false positive when the last child fits and only its margin crosses the edge — constructed and confirmed: a 50px paragraph in a 60px box with `margin-bottom: 40px` reports 30px with nothing hidden.

Accepted for now, because measuring painted content instead would mean comparing the last in-flow descendant's `getBoundingClientRect()` against the container's padding box — materially more machinery, and the 18-deck sweep shows no real deck currently hits the pure-false-positive shape. Recorded so a future "lint says 47px but only one line is missing" report is recognized as this, not a new bug.

### Tolerance

`OVERFLOW_TOLERANCE_PX` is 1px today. Sub-pixel layout rounding produces small `scrollHeight` excesses on perfectly healthy elements (an `h1` measured a 3px excess with `overflow: visible`). Gating on "the computed overflow actually clips" removes most of that, since `visible` elements are the noisy ones. The same 1px tolerance then applies to what remains.

Measured across 14 example decks: with clipping-only gating plus the hidden-element predicate, exactly one element remains flagged — `peitho-tour` slide 11 at 14px, which is a genuine defect. Zero false positives.

## Tasks

1. **`lint_measure.js`**: per-element clipping measurement with the visually-hidden predicate and slot-name resolution; emit the new optional fields.
2. **`lint.rs`**: deserialize the new fields; `SlotOverflowWarning` + `collect_slot_overflow_warnings`; render in `write_lint_report`; count into the summary.
3. **Tests**: unit tests for the collector (present/absent, both axes, with and without slot name, tolerance boundary); an integration test that a clipping deck warns and a healthy deck does not.
4. **Fix `examples/peitho-tour` slide 11** — the deck this feature found.

   Measured before choosing the fix: `pre.slot-code` has clientHeight 286 / scrollHeight 300 (**14px hidden**), padding 18px top and bottom, and the last line `# The main thing` has `lastLineVisible: true` while sitting 10px past the padding box. **No code text is lost** — all 11 lines render; what is clipped is the block's own bottom padding, which is why the last line appears to touch the border.

   So the fix is *not* to trim deck content (that would remove a line the reader can currently see). The seam is the keyed override that already exists for this slide in `examples/peitho-tour/css/overrides.css`, whose comment reads "shrink it to fit" — the author already attempted this and landed 14px short. Adjust that override (font-size, line-height, or vertical padding) so the block fits with its padding intact, leaving `deck.md` unchanged.

   Success criterion is measured, not eyeballed: `pre.scrollHeight <= pre.clientHeight` for that slide, and zero *overflow* warnings from `peitho lint`. The deck's 19 font-size warnings are pre-existing (default theme body text at 22.5pt) and out of scope.
5. **E2E**: run `peitho lint` over every example deck and confirm only intended warnings appear (math decks must stay clean).

## Verification

Beyond the workspace gates, this needs a real browser: `lint_measure.js` runs in Chrome and jsdom cannot reproduce the clipping geometry that is the entire subject of the change.

**Baseline captured before implementation** (all 18 example decks, warnings split by kind): **zero overflow warnings everywhere**; every existing warning is a font-size warning from #386/#388 (the default theme renders body text at 22.5pt against a 24pt threshold — `peitho-tour` alone accounts for 19 of them).

That baseline is what makes the check meaningful: warning *counts* cannot be the signal, because most decks already warn. After implementation, re-run all 18 decks and split by kind again. The expected delta is exactly one new overflow warning, on `peitho-tour` slide 11, which Task 4 then drives to zero. Any other new overflow warning is a false positive and must be investigated — in particular the math decks must stay clean, since their KaTeX MathML is clipped by design.
