import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  formatMinuteSeconds,
  installTimeTracker,
  isOverrun,
  isValidDurationMs,
  type TimeTrackerShell
} from "../src/index";

const cleanups: Array<() => void> = [];

function shell(overrides: Partial<TimeTrackerShell> = {}): TimeTrackerShell {
  return {
    manifest: { slideCount: 3 },
    currentIndex: 0,
    elapsedMs: () => 0,
    ...overrides
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

it("formats durations as tracker-style m:ss", () => {
  expect(formatMinuteSeconds(0)).toBe("0:00");
  expect(formatMinuteSeconds(15_000)).toBe("0:15");
  expect(formatMinuteSeconds(90_000)).toBe("1:30");
  expect(formatMinuteSeconds(3_600_000)).toBe("60:00");
});

it("validates positive safe integer durations", () => {
  expect(isValidDurationMs(1)).toBe(true);
  expect(isValidDurationMs(60_000)).toBe(true);
  expect(isValidDurationMs(0)).toBe(false);
  expect(isValidDurationMs(-1)).toBe(false);
  expect(isValidDurationMs(1.5)).toBe(false);
  expect(isValidDurationMs(Number.NaN)).toBe(false);
  expect(isValidDurationMs(Number.POSITIVE_INFINITY)).toBe(false);
  expect(isValidDurationMs(Number.MAX_SAFE_INTEGER + 1)).toBe(false);
});

it("moves rabbit by slide progress and turtle by elapsed progress", () => {
  let elapsed = 30_000;
  const root = document.createElement("main");
  const bus = new EventTarget();
  const cleanup = installTimeTracker({
    root,
    shell: shell({ elapsedMs: () => elapsed }),
    plannedDurationMs: 60_000,
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 1, total: 3, previousIndex: 0, key: "middle" }
    })
  );

  expect(root.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')?.style.left).toBe("50%");
  expect(root.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')?.style.left).toBe("50%");
});

it("keeps the present variant DOM unchanged", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell(),
      plannedDurationMs: 60_000,
      bus,
      window,
      document,
      variant: "present"
    })
  );

  expect(root.innerHTML).toBe(
    '<div class="peitho-time-tracker" data-peitho-time-tracker="present"><span data-peitho-marker="rabbit" aria-label="slide progress" style="left: 0%; transform: translateX(0%);">🐰</span><span data-peitho-marker="turtle" aria-label="time progress" style="left: 0%; transform: translateX(0%);">🐢</span></div>'
  );
});

it("renders presenter variant with legend fill track and five-point time scale", () => {
  let elapsed = 30_000;
  const root = document.createElement("main");
  const bus = new EventTarget();
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell({ currentIndex: 1, elapsedMs: () => elapsed }),
      plannedDurationMs: 60_000,
      bus,
      window,
      document,
      variant: "presenter"
    })
  );

  const tracker = root.querySelector<HTMLElement>('[data-peitho-time-tracker="presenter"]')!;
  expect(tracker.querySelector(".tracker-legend")?.textContent).toBe("Slide progressTime");
  expect(tracker.querySelector(".tracker")).not.toBeNull();
  expect(tracker.querySelector<HTMLElement>(".tracker-fill")?.style.width).toBe("50%");
  expect(tracker.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')?.className).toBe("");
  expect(tracker.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')?.className).toBe("");
  expect(
    Array.from(tracker.querySelectorAll(".tracker-scale span"), (span) => span.textContent)
  ).toEqual(["0:00", "0:15", "0:30", "0:45", "1:00"]);

  elapsed = 45_000;
  vi.advanceTimersByTime(250);
  expect(tracker.querySelector<HTMLElement>(".tracker-fill")?.style.width).toBe("75%");
});

it("clamps markers to the track edges so the glyph never overflows", () => {
  let elapsed = 0;
  const root = document.createElement("main");
  const bus = new EventTarget();
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell({ manifest: { slideCount: 3 }, elapsedMs: () => elapsed }),
      plannedDurationMs: 60_000,
      bus,
      window,
      document
    })
  );

  const rabbit = root.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')!;
  const turtle = root.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')!;

  expect(rabbit.style.left).toBe("0%");
  expect(rabbit.style.transform).toBe("translateX(0%)");
  expect(turtle.style.left).toBe("0%");
  expect(turtle.style.transform).toBe("translateX(0%)");

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 1, total: 3, previousIndex: 0, key: "middle" }
    })
  );
  elapsed = 30_000;
  vi.advanceTimersByTime(250);
  expect(rabbit.style.transform).toBe("translateX(-50%)");
  expect(turtle.style.transform).toBe("translateX(-50%)");

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 2, total: 3, previousIndex: 1, key: "last" }
    })
  );
  elapsed = 60_000;
  vi.advanceTimersByTime(250);
  expect(rabbit.style.left).toBe("100%");
  expect(rabbit.style.transform).toBe("translateX(-100%)");
  expect(turtle.style.left).toBe("100%");
  expect(turtle.style.transform).toBe("translateX(-100%)");
});

it("rejects non-positive and non-finite planned durations", () => {
  for (const plannedDurationMs of [0, -1, Number.NaN]) {
    const root = document.createElement("main");
    const bus = new EventTarget();
    let cleanup: (() => void) | undefined;

    try {
      expect(() => {
        cleanup = installTimeTracker({
          root,
          shell: shell(),
          plannedDurationMs,
          bus,
          window,
          document
        });
      }).toThrow("plannedDurationMs must be a positive finite number");
    } finally {
      cleanup?.();
    }
  }
});

it("dispatches timer start once on the first forward slidechange", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const starts: unknown[] = [];
  bus.addEventListener("peitho:timercontrol", (event) => starts.push((event as CustomEvent).detail));
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell({ manifest: { slideCount: 2 } }),
      plannedDurationMs: 60_000,
      bus,
      window,
      document
    })
  );

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 1, total: 2, previousIndex: 0, key: "two" }
    })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 0, total: 2, previousIndex: 1, key: "one" }
    })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 1, total: 2, previousIndex: 0, key: "two" }
    })
  );

  expect(starts).toEqual([{ action: "start" }]);
});

it("does not auto-start when the first slidechange has a null previous index", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const starts: unknown[] = [];
  bus.addEventListener("peitho:timercontrol", (event) => starts.push((event as CustomEvent).detail));
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell(),
      plannedDurationMs: 60_000,
      bus,
      window,
      document
    })
  );

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 1, total: 3, previousIndex: null, key: "middle" }
    })
  );

  expect(starts).toEqual([]);
});

it("does not auto-start on backward slidechange and does not dispatch twice", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const starts: unknown[] = [];
  bus.addEventListener("peitho:timercontrol", (event) => starts.push((event as CustomEvent).detail));
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell({ currentIndex: 2 }),
      plannedDurationMs: 60_000,
      bus,
      window,
      document
    })
  );

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 1, total: 3, previousIndex: 2, key: "middle" }
    })
  );
  expect(starts).toEqual([]);

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 2, total: 3, previousIndex: 1, key: "last" }
    })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 1, total: 3, previousIndex: 2, key: "middle" }
    })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 2, total: 3, previousIndex: 1, key: "last" }
    })
  );

  expect(starts).toEqual([{ action: "start" }]);
});

it("logs and ignores malformed slidechange detail", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const log = { error: vi.fn() };
  const starts: unknown[] = [];
  bus.addEventListener("peitho:timercontrol", (event) => starts.push((event as CustomEvent).detail));
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell(),
      plannedDurationMs: 60_000,
      bus,
      window,
      document,
      console: log
    })
  );

  const rabbit = root.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')!;
  expect(() => bus.dispatchEvent(new CustomEvent("peitho:slidechange"))).not.toThrow();
  expect(log.error).toHaveBeenCalledWith("Invalid peitho:slidechange event");
  expect(rabbit.style.left).toBe("0%");
  expect(starts).toEqual([]);

  log.error.mockClear();
  expect(() =>
    bus.dispatchEvent(
      new CustomEvent("peitho:slidechange", {
        detail: { index: "later", total: 3, previousIndex: 0, key: "bad" }
      })
    )
  ).not.toThrow();

  expect(log.error).toHaveBeenCalledWith("Invalid peitho:slidechange event");
  expect(rabbit.style.left).toBe("0%");
  expect(starts).toEqual([]);
});

it("logs and ignores slidechange detail with a non-positive total", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const log = { error: vi.fn() };
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell({ currentIndex: 1 }),
      plannedDurationMs: 60_000,
      bus,
      window,
      document,
      console: log
    })
  );

  const rabbit = root.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')!;
  expect(rabbit.style.left).toBe("50%");

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 2, total: 0, previousIndex: 1, key: "bad" }
    })
  );

  expect(log.error).toHaveBeenCalledWith("Invalid peitho:slidechange event");
  expect(rabbit.style.left).toBe("50%");
});

it("keeps rabbit at zero percent for a one-slide deck", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell({ manifest: { slideCount: 1 } }),
      plannedDurationMs: 60_000,
      bus,
      window,
      document
    })
  );

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 0, total: 1, previousIndex: null, key: "only" }
    })
  );

  expect(root.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')?.style.left).toBe("0%");
});

it("pins turtle at one hundred percent and marks overrun after planned time", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell({ elapsedMs: () => 60_001 }),
      plannedDurationMs: 60_000,
      bus,
      window,
      document
    })
  );

  expect(root.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')?.style.left).toBe(
    "100%"
  );
  expect(
    root.querySelector<HTMLElement>("[data-peitho-time-tracker]")?.hasAttribute(
      "data-peitho-overrun"
    )
  ).toBe(true);
});

it("detects overrun with millisecond precision", () => {
  expect(isOverrun(60_000, 60_000)).toBe(false);
  expect(isOverrun(60_001, 60_000)).toBe(true);
});

it("updates turtle on a 250ms interval", () => {
  let elapsed = 0;
  const root = document.createElement("main");
  const bus = new EventTarget();
  cleanups.push(
    installTimeTracker({
      root,
      shell: shell({ elapsedMs: () => elapsed }),
      plannedDurationMs: 60_000,
      bus,
      window,
      document
    })
  );

  const turtle = root.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')!;
  expect(turtle.style.left).toBe("0%");

  elapsed = 30_000;
  vi.advanceTimersByTime(249);
  expect(turtle.style.left).toBe("0%");
  vi.advanceTimersByTime(1);
  expect(turtle.style.left).toBe("50%");
});

it("removes interval and slidechange listener on cleanup", () => {
  let elapsed = 0;
  const root = document.createElement("main");
  const bus = new EventTarget();
  const cleanup = installTimeTracker({
    root,
    shell: shell({ elapsedMs: () => elapsed }),
    plannedDurationMs: 60_000,
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  const rabbit = root.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')!;
  const turtle = root.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')!;
  expect(vi.getTimerCount()).toBe(1);
  expect(root.querySelector("[data-peitho-time-tracker]")).not.toBeNull();

  cleanup();
  cleanups.pop();
  elapsed = 30_000;
  vi.advanceTimersByTime(250);
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { index: 2, total: 3, previousIndex: 1, key: "last" }
    })
  );

  expect(vi.getTimerCount()).toBe(0);
  expect(root.querySelector("[data-peitho-time-tracker]")).toBeNull();
  expect(rabbit.style.left).toBe("0%");
  expect(turtle.style.left).toBe("0%");
});
