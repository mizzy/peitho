import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  installRehearsalReporter,
  type RehearsalReportDetail
} from "../src/rehearsalReporter";
import type { RehearsalSnapshot } from "../../../bindings/RehearsalSnapshot";
import type { BeforeCloseDetail } from "../src/sync";

const cleanups: Array<() => void> = [];
const emptyTimeline = { entries: () => [] };

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
    reports.push((event as CustomEvent<RehearsalReportDetail>).detail.snapshot);
  });
  return reports;
}

function collectReportDetails(bus: EventTarget): RehearsalReportDetail[] {
  const reports: RehearsalReportDetail[] = [];
  bus.addEventListener("peitho:rehearsalreport", (event) => {
    reports.push((event as CustomEvent<RehearsalReportDetail>).detail);
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
    timeline: emptyTimeline,
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
      version: 2,
      elapsedMs: 1_250,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 1_250 }],
      timeline: []
    }
  ]);

  paused = true;
  elapsed = 2_000;
  vi.advanceTimersByTime(5_000);
  expect(reports).toHaveLength(1);
});

it("reports immediately on local start but not ordinary resume", () => {
  const bus = new EventTarget();
  const reports = collectReportDetails(bus);
  let startedAt: number | null = null;
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [0], flush: vi.fn() },
    timeline: {
      entries: () => [{ key: "intro", index: 0, atMs: 0 }]
    },
    shell: {
      elapsedMs: () => 0,
      startedAt: () => startedAt,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  startedAt = 100;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "resume" } }));

  expect(reports).toEqual([
    {
      final: false,
      snapshot: {
        version: 2,
        elapsedMs: 0,
        sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 0 }],
        timeline: [{ key: "intro", index: 0, atMs: 0 }]
      }
    }
  ]);
});

it("reports immediately on the first positive running timer adoption", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  let startedAt: number | null = null;
  let elapsedMs = 0;
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [0], flush: vi.fn() },
    timeline: {
      entries: () => [{ key: "details", index: 1, atMs: 7_000 }]
    },
    shell: {
      elapsedMs: () => elapsedMs,
      startedAt: () => startedAt,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 1, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  startedAt = 100;
  elapsedMs = 7_000;
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: true, previousElapsedMs: 0, elapsedMs: 7_000 }
    })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: true, previousElapsedMs: 7_000, elapsedMs: 7_000 }
    })
  );

  expect(reports).toEqual([
    {
      version: 2,
      elapsedMs: 7_000,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 0 }],
      timeline: [{ key: "details", index: 1, atMs: 7_000 }]
    }
  ]);
});

it("reports immediately when a running timer is adopted at zero", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  let startedAt: number | null = null;
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [0], flush: vi.fn() },
    timeline: {
      entries: () => [{ key: "intro", index: 0, atMs: 0 }]
    },
    shell: {
      elapsedMs: () => 0,
      startedAt: () => startedAt,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  startedAt = 100;
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: true, previousElapsedMs: 0, elapsedMs: 0 }
    })
  );

  expect(reports).toEqual([
    {
      version: 2,
      elapsedMs: 0,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 0 }],
      timeline: [{ key: "intro", index: 0, atMs: 0 }]
    }
  ]);
});

it("reports once when adoption transitions a running rehearsal to paused", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  let elapsedMs = 3_000;
  let paused = false;
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [elapsedMs], flush: vi.fn() },
    timeline: {
      entries: () => [{ key: "intro", index: 0, atMs: 0 }]
    },
    shell: {
      elapsedMs: () => elapsedMs,
      startedAt: () => 100,
      isPaused: () => paused
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  function adopt(running: boolean, nextElapsedMs: number): void {
    const previousElapsedMs = elapsedMs;
    elapsedMs = nextElapsedMs;
    paused = !running;
    bus.dispatchEvent(
      new CustomEvent("peitho:timeradopt", {
        detail: { running, previousElapsedMs, elapsedMs }
      })
    );
  }

  adopt(true, 3_100);
  adopt(false, 3_900);
  adopt(false, 3_900);

  expect(reports).toEqual([
    {
      version: 2,
      elapsedMs: 3_900,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 3_900 }],
      timeline: [{ key: "intro", index: 0, atMs: 0 }]
    }
  ]);

  adopt(true, 3_900);
  adopt(false, 4_200);
  expect(reports.at(-1)?.elapsedMs).toBe(4_200);
  expect(reports).toHaveLength(2);
});

it("reports immediately on slidechange pause and reset after first start", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  let startedAt: number | null = 100;
  let elapsed = 1_000;
  let actual = 1_000;
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [actual], flush: vi.fn() },
    timeline: emptyTimeline,
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
      version: 2,
      elapsedMs: 1_000,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 1_000 }],
      timeline: []
    },
    {
      version: 2,
      elapsedMs: 2_000,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 2_000 }],
      timeline: []
    },
    {
      version: 2,
      elapsedMs: 0,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 0 }],
      timeline: []
    }
  ]);
});

it("rounds fractional elapsed and section actuals before reporting", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [432.4, 802.5], flush: vi.fn() },
    timeline: {
      entries: () => [{ key: "intro", index: 0, atMs: 0 }]
    },
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
      version: 2,
      elapsedMs: 1_235,
      sections: [
        { name: "Setup", plannedDurationMs: 60_000, actualMs: 432 },
        { name: "Demo", plannedDurationMs: 60_000, actualMs: 803 }
      ],
      timeline: [{ key: "intro", index: 0, atMs: 0 }]
    }
  ]);
});

it("does not report a zero adopt before the timer has ever started", () => {
  const bus = new EventTarget();
  const reports = collectReports(bus);
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [0], flush: vi.fn() },
    timeline: emptyTimeline,
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
    timeline: emptyTimeline,
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
      version: 2,
      elapsedMs: 0,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 0 }],
      timeline: []
    }
  ]);
});

it("reports through beforeclose and forwards its waitUntil registration", () => {
  const bus = new EventTarget();
  const reports = collectReportDetails(bus);
  const waitUntil = vi.fn();
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [2_500], flush: vi.fn() },
    timeline: emptyTimeline,
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

  bus.dispatchEvent(
    new CustomEvent<BeforeCloseDetail>("peitho:beforeclose", {
      detail: { waitUntil }
    })
  );

  expect(reports).toHaveLength(1);
  expect(reports[0]).toEqual({
    final: true,
    snapshot: {
      version: 2,
      elapsedMs: 2_500,
      sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 2_500 }],
      timeline: []
    },
    waitUntil
  });
});

it("reports on pagehide and removes the listener during cleanup", () => {
  const bus = new EventTarget();
  const reports = collectReportDetails(bus);
  let elapsedMs = 3_900;
  const cleanup = installRehearsalReporter({
    actuals: { actualMs: () => [elapsedMs], flush: vi.fn() },
    timeline: {
      entries: () => [{ key: "intro", index: 0, atMs: 0 }]
    },
    shell: {
      elapsedMs: () => elapsedMs,
      startedAt: () => 100,
      isPaused: () => true
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(cleanup);

  window.dispatchEvent(new Event("pagehide"));

  expect(reports).toEqual([
    {
      final: true,
      keepalive: true,
      snapshot: {
        version: 2,
        elapsedMs: 3_900,
        sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 3_900 }],
        timeline: [{ key: "intro", index: 0, atMs: 0 }]
      }
    }
  ]);

  cleanup();
  cleanups.pop();
  elapsedMs = 4_200;
  window.dispatchEvent(new Event("pagehide"));
  expect(reports).toHaveLength(1);
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
    timeline: emptyTimeline,
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
    timeline: emptyTimeline,
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
