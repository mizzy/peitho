import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  installCanvasClickNavigation,
  installFullscreenShortcut,
  installPresentationControls,
  installSwipeNavigation
} from "../src/index";

const cleanups: Array<() => void> = [];

function listenWindow(type: string, listener: EventListener): void {
  window.addEventListener(type, listener);
  cleanups.push(() => window.removeEventListener(type, listener));
}

function touch(clientX: number, clientY: number): Touch {
  return { clientX, clientY } as Touch;
}

function touchEvent(
  type: "touchstart" | "touchend" | "touchcancel",
  options: { touches?: Touch[]; changedTouches?: Touch[] } = {}
): Event {
  const event = new Event(type, { bubbles: true, cancelable: type === "touchend" });
  Object.defineProperty(event, "touches", {
    value: options.touches ?? []
  });
  Object.defineProperty(event, "changedTouches", {
    value: options.changedTouches ?? []
  });
  return event;
}

function mockSelection(isCollapsed: boolean): void {
  vi.spyOn(window, "getSelection").mockReturnValue({ isCollapsed } as Selection);
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

it("opens presenter popup with default display management", async () => {
  const root = document.createElement("main");
  const openWindow = vi.fn();
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    openPresenter: undefined,
    openPresenterWindow: openWindow
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="presenter"]')?.click();
  await Promise.resolve();

  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
});

it("dispatches a close request from the close button", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:closerequest", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="close"]')?.click();

  expect(requests).toEqual([null]);
});

it("keeps the explicit openPresenter injection as the highest priority", () => {
  const root = document.createElement("main");
  const openPresenter = vi.fn();
  const openPresenterWindow = vi.fn();
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    openPresenter,
    openPresenterWindow
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="presenter"]')?.click();

  expect(openPresenter).toHaveBeenCalledTimes(1);
  expect(openPresenterWindow).not.toHaveBeenCalled();
});

it("clicks in the left viewport quarter request prev and other canvas clicks request next", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  vi.spyOn(window, "innerWidth", "get").mockReturnValue(1000);
  mockSelection(true);
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

it("does not navigate from a click that ends a drag gesture", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  mockSelection(true);
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installCanvasClickNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, clientX: 100, clientY: 100 }));
  root.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientX: 112, clientY: 100 }));
  root.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, clientX: 112, clientY: 100 }));
  root.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 112, clientY: 100 }));

  expect(requests).toEqual([]);
});

it("does not navigate from a click while text selection is non-collapsed", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  mockSelection(false);
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installCanvasClickNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 900 }));

  expect(requests).toEqual([]);
});

it("does not navigate when a click starts inside the control bar", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  mockSelection(true);
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

it("swipe left dispatches next", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  const end = touchEvent("touchend", { changedTouches: [touch(100, 305)] });
  root.dispatchEvent(end);

  expect(requests).toEqual([{ to: "next" }]);
  expect(end.defaultPrevented).toBe(true);
});

it("swipe right dispatches prev", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(100, 300)] }));
  root.dispatchEvent(touchEvent("touchend", { changedTouches: [touch(200, 305)] }));

  expect(requests).toEqual([{ to: "prev" }]);
});

it("too-short horizontal swipe does not dispatch", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  root.dispatchEvent(touchEvent("touchend", { changedTouches: [touch(160, 300)] }));

  expect(requests).toEqual([]);
});

it("too-short horizontal drag does not preventDefault touchend", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  const end = touchEvent("touchend", { changedTouches: [touch(210, 300)] });
  root.dispatchEvent(end);

  expect(requests).toEqual([]);
  expect(end.defaultPrevented).toBe(false);
});

it("failed swipe with significant horizontal movement DOES preventDefault touchend to suppress follow-up click", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  const end = touchEvent("touchend", { changedTouches: [touch(240, 500)] });
  root.dispatchEvent(end);

  expect(requests).toEqual([]);
  expect(end.defaultPrevented).toBe(true);
});

it("mostly-vertical swipe does not dispatch", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  root.dispatchEvent(touchEvent("touchend", { changedTouches: [touch(100, 500)] }));

  expect(requests).toEqual([]);
});

it("too-slow swipe does not dispatch", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  vi.spyOn(window.performance, "now").mockReturnValueOnce(0).mockReturnValueOnce(801);
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  root.dispatchEvent(touchEvent("touchend", { changedTouches: [touch(100, 305)] }));

  expect(requests).toEqual([]);
});

it("multi-touch touchstart is ignored", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(
    touchEvent("touchstart", {
      touches: [touch(200, 300), touch(220, 320)]
    })
  );
  root.dispatchEvent(touchEvent("touchend", { changedTouches: [touch(100, 305)] }));

  expect(requests).toEqual([]);
});

it("mid-swipe multi-touch touchstart does not cancel the active gesture", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  root.dispatchEvent(
    touchEvent("touchstart", {
      touches: [touch(200, 300), touch(220, 320)]
    })
  );
  root.dispatchEvent(touchEvent("touchend", { changedTouches: [touch(100, 305)] }));

  expect(requests).toEqual([{ to: "next" }]);
});

it("swipe that starts inside control bar is ignored", () => {
  const root = document.createElement("main");
  const bar = document.createElement("nav");
  const requests: unknown[] = [];
  bar.dataset.peithoControlBar = "true";
  root.appendChild(bar);
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  bar.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  root.dispatchEvent(touchEvent("touchend", { changedTouches: [touch(100, 305)] }));

  expect(requests).toEqual([]);
});

it("cleanup() removes swipe listeners", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installSwipeNavigation({ root, window, bus: window });
  cleanup();

  root.dispatchEvent(touchEvent("touchstart", { touches: [touch(200, 300)] }));
  root.dispatchEvent(touchEvent("touchend", { changedTouches: [touch(100, 305)] }));

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

it("fullscreen shortcut ignores chord-modified f", () => {
  const requestFullscreen = vi.fn();
  Object.defineProperty(document.documentElement, "requestFullscreen", {
    value: requestFullscreen,
    configurable: true
  });
  Object.defineProperty(document, "fullscreenElement", {
    value: null,
    configurable: true
  });
  const cleanup = installFullscreenShortcut({ window, document });
  cleanups.push(cleanup);

  const event = new KeyboardEvent("keydown", { key: "f", metaKey: true, cancelable: true });
  window.dispatchEvent(event);

  expect(event.defaultPrevented).toBe(false);
  expect(requestFullscreen).not.toHaveBeenCalled();
});
