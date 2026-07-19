import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installSectionActuals } from "../src/sectionActuals";
import type { PresentShell, SlideChangeDetail, TimerControlDetail } from "../src/index";

const cleanups: Array<() => void> = [];

function shell(overrides: Partial<PresentShell> = {}): Pick<
  PresentShell,
  "currentIndex" | "elapsedMs" | "startedAt"
> {
  return {
    currentIndex: 0,
    elapsedMs: () => 0,
    startedAt: () => 100,
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
});

it("accumulates elapsed deltas by current section and keeps revisit actuals", () => {
  let elapsed = 0;
  let currentIndex = 0;
  const bus = new EventTarget();
  const actuals = installSectionActuals({
    shell: {
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsed,
      startedAt: () => 100
    },
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 1, plannedDurationMs: 1_000 },
      { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 1_000 }
    ],
    bus,
    window
  });
  cleanups.push(actuals.destroy);

  elapsed = 1_000;
  vi.advanceTimersByTime(250);
  currentIndex = 2;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "demo", index: 2, total: 3, previousIndex: 0 }
    })
  );
  elapsed = 3_000;
  vi.advanceTimersByTime(250);
  currentIndex = 0;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "setup", index: 0, total: 3, previousIndex: 2 }
    })
  );

  expect(actuals.actualMs()).toEqual([1_000, 2_000]);
});

it("clears actuals immediately on timer reset", () => {
  let elapsed = 0;
  const bus = new EventTarget();
  const actuals = installSectionActuals({
    shell: shell({ elapsedMs: () => elapsed }),
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 1_000 }],
    bus,
    window
  });
  cleanups.push(actuals.destroy);

  elapsed = 1_500;
  vi.advanceTimersByTime(250);
  bus.dispatchEvent(
    new CustomEvent<TimerControlDetail>("peitho:timercontrol", {
      detail: { action: "reset" }
    })
  );

  expect(actuals.actualMs()).toEqual([0]);
});

it("flushes pending elapsed to the current section on demand", () => {
  let elapsed = 0;
  const bus = new EventTarget();
  const actuals = installSectionActuals({
    shell: shell({ elapsedMs: () => elapsed }),
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 10_000 }],
    bus,
    window
  });
  cleanups.push(actuals.destroy);

  elapsed = 1_750;
  actuals.flush();

  expect(actuals.actualMs()).toEqual([1_750]);
});

it("attributes pending elapsed before timer adopt rebases", () => {
  const bus = new EventTarget();
  const actuals = installSectionActuals({
    shell: shell({ currentIndex: 0, startedAt: () => 100 }),
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 10_000 }],
    bus,
    window
  });
  cleanups.push(actuals.destroy);

  for (let i = 1; i <= 10; i += 1) {
    bus.dispatchEvent(
      new CustomEvent("peitho:timeradopt", {
        detail: {
          running: true,
          previousElapsedMs: i * 210 - 10,
          elapsedMs: i * 210
        }
      })
    );
  }

  expect(actuals.actualMs()).toEqual([2_000]);
});

it("removes interval and listeners on cleanup", () => {
  let elapsed = 0;
  const bus = new EventTarget();
  const actuals = installSectionActuals({
    shell: shell({ elapsedMs: () => elapsed }),
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(actuals.destroy);

  expect(vi.getTimerCount()).toBe(1);
  actuals.destroy();
  cleanups.pop();
  elapsed = 60_000;
  vi.advanceTimersByTime(250);
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "only", index: 0, total: 1, previousIndex: null }
    })
  );

  expect(vi.getTimerCount()).toBe(0);
  expect(actuals.actualMs()).toEqual([0]);
});
