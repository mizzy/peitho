import { clampStep, resolveStepTarget, revealStepCount } from "./stepnav";

export type ViewerSlide = { revealSteps?: unknown };
export type ViewerPosition = { index: number; step: number };
export type ViewerNavigator = {
  readonly walkRevealSteps: boolean;
  direct(index: number, requestedStep?: number): ViewerPosition;
  move(position: ViewerPosition, direction: 1 | -1): ViewerPosition;
};

export function createViewerNavigator(options: {
  slides: ReadonlyArray<ViewerSlide>;
  walkRevealSteps: boolean;
}): ViewerNavigator {
  const slides = options.slides.map(({ revealSteps }) => ({ revealSteps }));

  function normalizedIndex(index: number): number {
    if (slides.length === 0) return 0;
    const integer = Number.isFinite(index) ? Math.floor(index) : 0;
    return Math.max(0, Math.min(integer, slides.length - 1));
  }

  function direct(index: number, requestedStep?: number): ViewerPosition {
    const nextIndex = normalizedIndex(index);
    const stepCount = revealStepCount(slides[nextIndex]);
    if (!options.walkRevealSteps) return { index: nextIndex, step: stepCount };
    const validStep =
      requestedStep !== undefined &&
      Number.isSafeInteger(requestedStep) &&
      requestedStep >= 0;
    return {
      index: nextIndex,
      step: validStep ? clampStep(requestedStep, stepCount) : stepCount
    };
  }

  function move(position: ViewerPosition, direction: 1 | -1): ViewerPosition {
    const current = direct(position.index, position.step);
    if (slides.length === 0) return current;
    if (!options.walkRevealSteps) {
      const nextIndex = current.index + direction;
      if (nextIndex < 0 || nextIndex >= slides.length) return current;
      return direct(nextIndex);
    }
    return resolveStepTarget(slides, current, direction) ?? current;
  }

  return { walkRevealSteps: options.walkRevealSteps, direct, move };
}

function parseUrlInteger(raw: string | null): number | undefined {
  if (raw === null || !/^\d+$/.test(raw)) return undefined;
  const value = Number(raw);
  return Number.isSafeInteger(value) ? value : undefined;
}

function slideFromUrl(url: URL): number {
  const querySlide = url.searchParams.get("slide");
  if (querySlide !== null) return parseUrlInteger(querySlide) ?? 1;
  if (url.hash.startsWith("#slide=")) {
    return parseUrlInteger(url.hash.slice("#slide=".length).split("&")[0] ?? null) ?? 1;
  }
  if (url.hash.startsWith("#") && url.hash.length > 1) {
    return parseUrlInteger(url.hash.slice(1).split("&")[0] ?? null) ?? 1;
  }
  return 1;
}

export function readViewerPosition(url: URL, navigator: ViewerNavigator): ViewerPosition {
  const oneBasedSlide = slideFromUrl(url);
  const requestedStep = parseUrlInteger(url.searchParams.get("step"));
  return navigator.direct(oneBasedSlide - 1, requestedStep);
}

export function writeViewerPosition(
  url: URL,
  position: ViewerPosition,
  navigator: ViewerNavigator
): URL {
  const next = new URL(url.href);
  const normalized = navigator.direct(position.index, position.step);
  next.searchParams.set("slide", String(normalized.index + 1));
  if (navigator.walkRevealSteps) {
    next.searchParams.set("step", String(normalized.step));
  } else {
    next.searchParams.delete("step");
  }
  return next;
}

export {
  announceParsedSlides,
  announceShadowMounted,
  dropDisconnectedShadowMounted,
  executeInlineScripts,
  shadowMountedBacklog
} from "./scripts";
export { keyBelongsToTarget, pointerBelongsToTarget } from "./interactiveTarget";
export { expandRevealStepPages } from "./pdfStepPages";
export { applyStepState, createStepStateStyle } from "./stepState";
