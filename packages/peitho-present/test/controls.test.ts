import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  installCanvasClickNavigation,
  installFullscreenShortcut,
  installPresentationControls
} from "../src/index";

const cleanups: Array<() => void> = [];

function listenWindow(type: string, listener: EventListener): void {
  window.addEventListener(type, listener);
  cleanups.push(() => window.removeEventListener(type, listener));
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

it("shows controls on mousemove and hides them after the idle timeout", () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    idleMs: 3000
  });
  cleanups.push(cleanup);

  const bar = root.querySelector<HTMLElement>('[data-peitho-control-bar="true"]');
  expect(bar?.hidden).toBe(true);

  window.dispatchEvent(new MouseEvent("mousemove"));
  expect(bar?.hidden).toBe(false);
  vi.advanceTimersByTime(2999);
  expect(bar?.hidden).toBe(false);
  vi.advanceTimersByTime(1);
  expect(bar?.hidden).toBe(true);
});

it("dispatches navigate requests and updates the counter from slidechange", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installPresentationControls({ root, window, document, bus: window });
  cleanups.push(cleanup);

  expect(root.querySelector('[data-peitho-control="counter"]')?.textContent).toBe("– / –");
  root.querySelector<HTMLButtonElement>('[data-peitho-action="prev"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="next"]')?.click();
  window.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "arch-1", index: 2, total: 12, previousIndex: 1 }
    })
  );

  expect(requests).toEqual([{ to: "prev" }, { to: "next" }]);
  expect(root.querySelector('[data-peitho-control="counter"]')?.textContent).toBe("3 / 12");
});

it("opens presenter.html from the presenter button", () => {
  const root = document.createElement("main");
  const openPresenter = vi.fn();
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    openPresenter
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="presenter"]')?.click();

  expect(openPresenter).toHaveBeenCalledTimes(1);
});

it("clicks in the left viewport quarter request prev and other canvas clicks request next", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  vi.spyOn(window, "innerWidth", "get").mockReturnValue(1000);
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installCanvasClickNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 100 }));
  root.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 250 }));
  root.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 900 }));

  expect(requests).toEqual([{ to: "prev" }, { to: "next" }, { to: "next" }]);
});

it("does not navigate when a click starts inside the control bar", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanupA = installCanvasClickNavigation({ root, window, bus: window });
  const cleanupB = installPresentationControls({ root, window, document, bus: window });
  cleanups.push(cleanupA, cleanupB);

  root
    .querySelector<HTMLElement>('[data-peitho-control-bar="true"]')
    ?.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 900 }));

  expect(requests).toEqual([]);
});

it("toggles fullscreen from the f key through browser APIs", () => {
  const requestFullscreen = vi.fn();
  const exitFullscreen = vi.fn();
  Object.defineProperty(document.documentElement, "requestFullscreen", {
    value: requestFullscreen,
    configurable: true
  });
  Object.defineProperty(document, "exitFullscreen", {
    value: exitFullscreen,
    configurable: true
  });
  Object.defineProperty(document, "fullscreenElement", {
    value: null,
    configurable: true
  });
  const cleanup = installFullscreenShortcut({ window, document });
  cleanups.push(cleanup);

  window.dispatchEvent(new KeyboardEvent("keydown", { key: "f" }));

  expect(requestFullscreen).toHaveBeenCalledTimes(1);
});
