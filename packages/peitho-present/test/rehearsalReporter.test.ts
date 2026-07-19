import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installRehearsalReporter } from "../src/rehearsalReporter";
import type { RehearsalSnapshot } from "../../../bindings/RehearsalSnapshot";

const cleanups: Array<() => void> = [];

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function collectReports(bus: EventTarget): RehearsalSnapshot[] {
  const reports: RehearsalSnapshot[] = [];
  bus.addEventListener("peitho:rehearsalreport", (event) => {
    reports.push((event as CustomEvent<RehearsalSnapshot>).detail);
  });
  return reports;
}

it("does not report before the timer has started and then reports every five seconds while running", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  let startedAt: number | null = null;
  let paused = false;
  let elapsed = 0;
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [1_250], flush: vi.fn() },
    shell: {
      elapsedMs: () => elapsed,
      startedAt: () => startedAt,
      isPaused: () => paused
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  vi.advanceTimersByTime(5_000);
  expect(reports).toEqual([]);

  startedAt = 100;
  elapsed = 1_250;
  vi.advanceTimersByTime(4_999);
  expect(reports).toEqual([]);
  vi.advanceTimersByTime(1);
  expect(reports).toEqual([
    {
      version: 1,
      elapsedMs: 1_250,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 1_250 }]
    }
  ]);

  paused = true;
  elapsed = 2_000;
  vi.advanceTimersByTime(5_000);
  expect(reports).toHaveLength(1);
});

it("reports immediately on slidechange pause and reset after first start", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  let startedAt: number | null = 100;
  let elapsed = 1_000;
  let actual = 1_000;
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [actual], flush: vi.fn() },
    shell: {
      elapsedMs: () => elapsed,
      startedAt: () => startedAt,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "setup", index: 0, total: 1, previousIndex: null }
    })
  );
  elapsed = 2_000;
  actual = 2_000;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  startedAt = null;
  elapsed = 0;
  actual = 0;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } }));

  expect(reports).toEqual([
    {
      version: 1,
      elapsedMs: 1_000,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 1_000 }]
    },
    {
      version: 1,
      elapsedMs: 2_000,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 2_000 }]
    },
    {
      version: 1,
      elapsedMs: 0,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 0 }]
    }
  ]);
});

it("rounds fractional elapsed and section actuals before reporting", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [432.4, 802.5], flush: vi.fn() },
    shell: {
      elapsedMs: () => 1_234.6,
      startedAt: () => 100,
      isPaused: () => false
    },
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
      { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 60_000 }
    ],
    bus,
    window
  });
  cleanups.push(cleanup);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));

  expect(reports).toEqual([
    {
      version: 1,
      elapsedMs: 1_235,
      sections: [
        { name: "Setup", plannedDurationMs: 60_000, actualMs: 432 },
        { name: "Demo", plannedDurationMs: 60_000, actualMs: 803 }
      ]
    }
  ]);
});

it("does not report a zero adopt before the timer has ever started", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [0], flush: vi.fn() },
    shell: {
      elapsedMs: () => 0,
      startedAt: () => null,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: false, elapsedMs: 0, previousElapsedMs: 2_000 }
    })
  );

  expect(reports).toEqual([]);
});

it("reports an adopted reset after a started session zeroes actuals", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [0], flush: vi.fn() },
    shell: {
      elapsedMs: () => 0,
      startedAt: () => 100,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: false, elapsedMs: 0, previousElapsedMs: 2_000 }
    })
  );

  expect(reports).toEqual([
    {
      version: 1,
      elapsedMs: 0,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 0 }]
    }
  ]);
});

it("reports on close requests so the final section tail is persisted", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [2_500], flush: vi.fn() },
    shell: {
      elapsedMs: () => 2_500,
      startedAt: () => 100,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  bus.dispatchEvent(new CustomEvent("peitho:closerequest"));

  expect(reports).toEqual([
    {
      version: 1,
      elapsedMs: 2_500,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 2_500 }]
    }
  ]);
});

it("flushes pending actuals before reporting a pause snapshot", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  let actual = 0;
  const flush = vi.fn(() => {
    actual = 2_250;
  });
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [actual], flush },
    shell: {
      elapsedMs: () => 2_250,
      startedAt: () => 100,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));

  expect(flush).toHaveBeenCalledTimes(1);
  expect(reports[0]?.elapsedMs).toBe(2_250);
  expect(reports[0]?.sections.reduce((sum, section) => sum + section.actualMs, 0)).toBe(
    2_250
  );
});

it("is a no-op without sections", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [], flush: vi.fn() },
    shell: {
      elapsedMs: () => 1_000,
      startedAt: () => 100,
      isPaused: () => false
    },
    sections: [],
    bus,
    window
  });
  cleanups.push(cleanup);

  vi.advanceTimersByTime(5_000);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));

  expect(reports).toEqual([]);
  expect(vi.getTimerCount()).toBe(0);
});
