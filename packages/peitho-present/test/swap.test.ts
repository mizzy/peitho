import { afterEach, expect, it } from "vitest";
import { installSwapShortcut, swapRoute } from "../src/index";

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
});

it("looks up swapped state and counterpart from one route table", () => {
  expect(swapRoute("/present.html")).toEqual({
    swapped: false,
    counterpart: "presenter-swapped"
  });
  expect(swapRoute("/")).toEqual({
    swapped: false,
    counterpart: "presenter-swapped"
  });
  expect(swapRoute("/presenter")).toEqual({
    swapped: false,
    counterpart: "present-swapped"
  });
  expect(swapRoute("/presenter.html")).toEqual({
    swapped: false,
    counterpart: "present-swapped"
  });
  expect(swapRoute("/present-swapped")).toEqual({
    swapped: true,
    counterpart: "presenter"
  });
  expect(swapRoute("/presenter-swapped")).toEqual({
    swapped: true,
    counterpart: "present.html"
  });
  expect(swapRoute("/slides/000-intro.html")).toBeNull();
  expect(swapRoute("/unknown")).toBeNull();
});

it("dispatches swaprequest on s keydown, ignores repeats, and cleans up", () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:swaprequest", (event) => requests.push((event as CustomEvent).detail));
  const cleanup = installSwapShortcut(window, bus);
  cleanups.push(cleanup);

  const swap = new KeyboardEvent("keydown", { key: "s", cancelable: true });
  const shiftedSwap = new KeyboardEvent("keydown", { key: "S", shiftKey: true, cancelable: true });
  const chordSwap = new KeyboardEvent("keydown", { key: "s", metaKey: true, cancelable: true });
  const repeatedSwap = new KeyboardEvent("keydown", { key: "s", repeat: true, cancelable: true });
  const other = new KeyboardEvent("keydown", { key: "x", cancelable: true });
  window.dispatchEvent(swap);
  window.dispatchEvent(shiftedSwap);
  window.dispatchEvent(chordSwap);
  window.dispatchEvent(repeatedSwap);
  window.dispatchEvent(other);

  expect(swap.defaultPrevented).toBe(true);
  expect(shiftedSwap.defaultPrevented).toBe(true);
  expect(chordSwap.defaultPrevented).toBe(false);
  expect(repeatedSwap.defaultPrevented).toBe(false);
  expect(other.defaultPrevented).toBe(false);
  expect(requests).toEqual([null, null]);

  cleanup();
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "s", cancelable: true }));
  expect(requests).toEqual([null, null]);
});
