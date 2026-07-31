import { nextNonSkippedIndex } from "./skipnav";

export type StepNavigationSlide = { skip?: boolean; revealSteps?: unknown };
export type StepPosition = { index: number; step: number };

export function revealStepCount(slide: StepNavigationSlide | undefined): number {
  const value = slide?.revealSteps;
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) return 0;
  return Math.floor(value);
}

export function clampStep(step: number, stepCount: number): number {
  if (!Number.isFinite(step)) return 0;
  return Math.max(0, Math.min(Math.floor(step), stepCount));
}

export function resolveStepTarget(
  slides: ReadonlyArray<StepNavigationSlide>,
  position: StepPosition,
  direction: 1 | -1
): StepPosition | null {
  if (direction === 1) {
    const stepCount = revealStepCount(slides[position.index]);
    if (position.step < stepCount) {
      return { index: position.index, step: position.step + 1 };
    }
    const index = nextNonSkippedIndex(slides, position.index, 1);
    return index === null ? null : { index, step: 0 };
  }
  if (position.step > 0) {
    return { index: position.index, step: position.step - 1 };
  }
  const index = nextNonSkippedIndex(slides, position.index, -1);
  return index === null ? null : { index, step: revealStepCount(slides[index]) };
}
