# Pace progress excludes skipped slides (Issue #745)

## Problem

Three consumers compute "how far through the planned talk is this slide" from the
raw slide index over `manifest.slideCount`, so `skip:true` slides count as talk:

- `timeTracker.ts` `updateSlides` (present/presenter rabbit): `index / (total - 1)`
- `remote.ts` `slideFraction` (remote rabbit, and the fill when no time is planned):
  `index / (slideCount - 1)`
- `remote.ts` `expectedElapsedAtSlide` (remote ahead / on pace / behind label):
  `index / slideCount`, per section `(index - start) / sectionSlideCount`

A skipped appendix keeps the rabbit below 100% on the last presented slide, and
a skipped slide in the middle makes the rabbit leap when navigation jumps it.

## Design

One skip-aware primitive in `skipnav.ts` (the module that already owns skip
semantics), used by all three consumers; no consumer reads raw indexes for
progress anymore.

- `presentedBefore(slides, index)`: number of non-skipped slides with `i < index`.
- `slideProgress(slides, index)`: the rabbit fraction.
  `n` = non-skipped count. Position = `presentedBefore(index)`, minus 1 when the
  current slide is skipped (a skipped slide holds the preceding presented
  slide's position), floored at 0. Fraction = `position / (n - 1)`, clamped to
  [0, 1]; `n <= 1` returns 1 (you are on the last presented slide), matching the
  remote's existing behavior. The tracker previously returned 0 there; the two
  copies disagreed, and they now share one answer.
- `expectedElapsedAtSlide` keeps its shape (entry time of slide `index`) but
  counts presented slides: deck-wide `plannedMs * presentedBefore(index) / n`,
  per section `presentedBefore` within the section over the section's presented
  count. A section with no presented slides contributes fraction 0 inside it
  (its planned time is still consumed by the next section's start). A skipped
  slide therefore has a zero-width on-pace window at the preceding slide's end.

`TimeTrackerShell.manifest` changes from `{ slideCount }` to
`{ slides: ReadonlyArray<{ skip?: boolean }> }` so the tracker cannot compute
progress without skip data.

Unchanged: presenter `N / total`, section ranges and planned times, navigation,
published output.
