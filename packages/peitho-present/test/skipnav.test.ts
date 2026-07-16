import { expect, it } from "vitest";
import { initialSlideIndex, nextNonSkippedIndex } from "../src/skipnav";

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
