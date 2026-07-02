import { afterEach, expect, it, vi } from "vitest";
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  calculateCanvasFit,
  installCanvasScaler
} from "../src/index";

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.restoreAllMocks();
});

it("exports the fixed Peitho canvas size", () => {
  expect(CANVAS_WIDTH).toBe(1280);
  expect(CANVAS_HEIGHT).toBe(720);
});

it("fits a 16 by 9 viewport without letterbox", () => {
  expect(calculateCanvasFit({ width: 1920, height: 1080 })).toEqual({
    scale: 1.5,
    width: 1920,
    height: 1080,
    left: 0,
    top: 0
  });
});

it("letterboxes vertically in a square viewport", () => {
  expect(calculateCanvasFit({ width: 1000, height: 1000 })).toEqual({
    scale: 0.78125,
    width: 1000,
    height: 562.5,
    left: 0,
    top: 218.75
  });
});

it("letterboxes horizontally in a narrow viewport", () => {
  expect(calculateCanvasFit({ width: 500, height: 720 })).toEqual({
    scale: 0.390625,
    width: 500,
    height: 281.25,
    left: 0,
    top: 219.375
  });
});

it("applies fixed canvas dimensions and a centered transform", () => {
  const target = document.createElement("section");
  const cleanup = installCanvasScaler({
    window,
    target,
    viewport: () => ({ width: 1920, height: 1080 })
  });
  cleanups.push(cleanup);

  expect(target.style.width).toBe("1280px");
  expect(target.style.height).toBe("720px");
  expect(target.style.transformOrigin).toBe("top left");
  expect(target.style.transform).toBe("translate(0px, 0px) scale(1.5)");
});

it("updates the transform on resize and stops after cleanup", () => {
  let viewport = { width: 1000, height: 1000 };
  const target = document.createElement("section");
  const cleanup = installCanvasScaler({
    window,
    target,
    viewport: () => viewport
  });

  expect(target.style.transform).toBe("translate(0px, 218.75px) scale(0.78125)");
  viewport = { width: 1280, height: 720 };
  window.dispatchEvent(new Event("resize"));
  expect(target.style.transform).toBe("translate(0px, 0px) scale(1)");

  cleanup();
  viewport = { width: 1920, height: 1080 };
  window.dispatchEvent(new Event("resize"));
  expect(target.style.transform).toBe("translate(0px, 0px) scale(1)");
});

it("uses window inner size by default", () => {
  const target = document.createElement("section");
  vi.spyOn(window, "innerWidth", "get").mockReturnValue(1280);
  vi.spyOn(window, "innerHeight", "get").mockReturnValue(720);

  const cleanup = installCanvasScaler({ window, target });
  cleanups.push(cleanup);

  expect(target.style.transform).toBe("translate(0px, 0px) scale(1)");
});
