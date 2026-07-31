import { expect, it } from "vitest";
import { clampStep, resolveStepTarget, revealStepCount } from "../src/stepnav";

it("walks reveal steps before advancing and stops at the deck boundary", () => {
  const slides = [{ revealSteps: 2 }, { revealSteps: 1 }];

  expect(resolveStepTarget(slides, { index: 0, step: 0 }, 1)).toEqual({ index: 0, step: 1 });
  expect(resolveStepTarget(slides, { index: 0, step: 1 }, 1)).toEqual({ index: 0, step: 2 });
  expect(resolveStepTarget(slides, { index: 0, step: 2 }, 1)).toEqual({ index: 1, step: 0 });
  expect(resolveStepTarget(slides, { index: 1, step: 1 }, 1)).toBeNull();
});

it("walks backward through steps and enters previous non-skipped slides fully revealed", () => {
  const slides = [{ revealSteps: 2 }, { skip: true, revealSteps: 4 }, { revealSteps: 1 }];

  expect(resolveStepTarget(slides, { index: 2, step: 1 }, -1)).toEqual({ index: 2, step: 0 });
  expect(resolveStepTarget(slides, { index: 2, step: 0 }, -1)).toEqual({ index: 0, step: 2 });
  expect(resolveStepTarget(slides, { index: 0, step: 2 }, 1)).toEqual({ index: 2, step: 0 });
  expect(resolveStepTarget(slides, { index: 0, step: 0 }, -1)).toBeNull();
});

it("normalizes reveal step counts and clamps explicit steps", () => {
  expect(revealStepCount({ revealSteps: -3 })).toBe(0);
  expect(revealStepCount({ revealSteps: Number.NaN })).toBe(0);
  expect(revealStepCount({ revealSteps: "x" })).toBe(0);
  expect(revealStepCount({ revealSteps: 2.9 })).toBe(2);

  expect(clampStep(Number.NaN, 2)).toBe(0);
  expect(clampStep(-1, 2)).toBe(0);
  expect(clampStep(1.9, 2)).toBe(1);
  expect(clampStep(5, 2)).toBe(2);
});
