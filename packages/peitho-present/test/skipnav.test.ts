import { expect, it } from "vitest";
import {
  initialSlideIndex,
  nextNonSkippedIndex,
  presentedBefore,
  slideProgress
} from "../src/skipnav";

it("finds the next non-skipped slide in a direction", () => {
  const slides = [{ skip: false }, { skip: true }, { skip: false }];

  expect(nextNonSkippedIndex(slides, 0, 1)).toBe(2);
  expect(nextNonSkippedIndex(slides, 2, -1)).toBe(0);
});

it("returns the first non-skipped slide as the initial slide", () => {
  expect(initialSlideIndex([{ skip: true }, { skip: false }, { skip: false }])).toBe(1);
});

it("falls back to the first slide when every slide is skipped", () => {
  expect(initialSlideIndex([{ skip: true }, { skip: true }])).toBe(0);
});

it("counts only presented slides strictly before an index", () => {
  const slides = [{}, { skip: true }, { skip: false }, {}];

  expect([-1, 0, 1, 2, 3, 4, 99].map((index) => presentedBefore(slides, index))).toEqual([
    0, 0, 1, 1, 2, 3, 3
  ]);
  expect(presentedBefore([], 0)).toBe(0);
  expect(presentedBefore([{ skip: true }, { skip: true }], 2)).toBe(0);
});

it("measures ordinary slide progress and clamps it to the track", () => {
  const slides = [{}, { skip: false }, {}];

  expect([-1, 0, 1, 2, 3].map((index) => slideProgress(slides, index))).toEqual([
    0, 0, 0.5, 1, 1
  ]);
});

it("reaches the goal on the last presented slide before a skipped appendix", () => {
  const slides = [{}, {}, {}, { skip: true }, { skip: true }];

  expect(slideProgress(slides, 2)).toBe(1);
  expect(slideProgress(slides, 3)).toBe(1);
  expect(slideProgress(slides, 4)).toBe(1);
});

it("holds the preceding presented position on a skipped middle slide", () => {
  const slides = [{}, {}, { skip: true }, {}, {}];

  expect(slides.map((_, index) => slideProgress(slides, index))).toEqual([
    0, 1 / 3, 1 / 3, 2 / 3, 1
  ]);
});

it("keeps leading skipped slides at the start of the track", () => {
  const slides = [{ skip: true }, { skip: true }, {}, {}];

  expect(slides.map((_, index) => slideProgress(slides, index))).toEqual([0, 0, 0, 1]);
});

it("returns zero progress for an empty deck", () => {
  expect(slideProgress([], 0)).toBe(0);
});

it("returns zero progress when every slide is skipped", () => {
  const slides = [{ skip: true }, { skip: true }];

  expect(slideProgress(slides, 0)).toBe(0);
  expect(slideProgress(slides, 1)).toBe(0);
});

it("returns the goal when there is exactly one presented slide", () => {
  for (const slides of [
    [{}],
    [{ skip: true }, {}, { skip: true }]
  ]) {
    for (const index of [0, 1, 2]) expect(slideProgress(slides, index)).toBe(1);
  }
});
