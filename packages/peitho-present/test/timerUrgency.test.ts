import { afterEach, expect, it, vi } from "vitest";
import {
  installUrgencyTracker,
  urgencyFor,
  type UrgencyChangeDetail,
  type UrgencyTrackerShell
} from "../src/timerUrgency";

it.each([
  ["planned duration is unset", 120_000, null, "normal"],
  ["remaining time is above three minutes", 119_999, 300_000, "normal"],
  ["remaining time is exactly three minutes", 120_000, 300_000, "warning"],
  ["remaining time is one minute and one second", 239_000, 300_000, "warning"],
  ["remaining time is exactly one minute", 240_000, 300_000, "urgent"],
  ["remaining time is one second", 299_000, 300_000, "urgent"],
  ["remaining time is zero", 300_000, 300_000, "urgent"],
  ["elapsed time exceeds the planned duration", 300_001, 300_000, "overrun"],
  ["two minute plans start at warning", 0, 120_000, "warning"],
  ["thirty second plans start at urgent", 0, 30_000, "urgent"]
] as const)("returns %s", (_label, elapsedMs, plannedDurationMs, expected) => {
  expect(urgencyFor(elapsedMs, plannedDurationMs)).toBe(expected);
});

const trackers: Array<{ destroy(): void }> = [];

afterEach(() => {
  while (trackers.length > 0) trackers.pop()?.destroy();
  vi.useRealTimers();
});

function fakeShell(initialElapsedMs = 0): UrgencyTrackerShell & {
  setElapsedMs(value: number): void;
  setStartedAt(value: number | null): void;
} {
  let elapsed = initialElapsedMs;
  let startedAt: number | null = 1;
  return {
    elapsedMs: () => elapsed,
    startedAt: () => startedAt,
    setElapsedMs(value: number) {
      elapsed = value;
    },
    setStartedAt(value: number | null) {
      startedAt = value;
    }
  };
}

function installTrackerForTest(options: {
  plannedDurationMs: number | null;
  elapsedMs?: number;
}) {
  vi.useFakeTimers();
  const bus = new EventTarget();
  const events: UrgencyChangeDetail[] = [];
  bus.addEventListener("peitho:urgencychange", (event) => {
    events.push((event as CustomEvent<UrgencyChangeDetail>).detail);
  });
  const shell = fakeShell(options.elapsedMs);
  trackers.push(
    installUrgencyTracker({
      shell,
      plannedDurationMs: options.plannedDurationMs,
      bus,
      window
    })
  );
  return { bus, events, shell };
}

it("urgency tracker emits nothing when planned duration is unset", () => {
  const { events, shell } = installTrackerForTest({ plannedDurationMs: null });

  shell.setElapsedMs(10 * 60_000);
  vi.advanceTimersByTime(1_000);

  expect(events).toEqual([]);
});

it("urgency tracker emits no initial event for the current urgency", () => {
  const { events } = installTrackerForTest({
    plannedDurationMs: 300_000,
    elapsedMs: 240_000
  });

  vi.advanceTimersByTime(250);

  expect(events).toEqual([]);
});

it("urgency tracker emits normal to warning at the boundary", () => {
  const { events, shell } = installTrackerForTest({ plannedDurationMs: 300_000 });

  vi.advanceTimersByTime(250);
  shell.setElapsedMs(120_000);
  vi.advanceTimersByTime(250);

  expect(events).toEqual([{ from: "normal", to: "warning" }]);
});

it("urgency tracker emits every intermediate transition on a multi-band jump", () => {
  const { events, shell } = installTrackerForTest({ plannedDurationMs: 300_000 });

  vi.advanceTimersByTime(250);
  shell.setElapsedMs(300_001);
  vi.advanceTimersByTime(250);

  expect(events).toEqual([
    { from: "normal", to: "warning" },
    { from: "warning", to: "urgent" },
    { from: "urgent", to: "overrun" }
  ]);
});

it("urgency tracker emits nothing on rewind to a less urgent band", () => {
  const { events, shell } = installTrackerForTest({ plannedDurationMs: 300_000 });

  vi.advanceTimersByTime(250);
  shell.setElapsedMs(240_000);
  vi.advanceTimersByTime(250);
  events.length = 0;
  shell.setElapsedMs(0);
  vi.advanceTimersByTime(250);

  expect(events).toEqual([]);
});

it("urgency tracker emits nothing on repeated ticks with the same urgency", () => {
  const { events, shell } = installTrackerForTest({ plannedDurationMs: 300_000 });

  vi.advanceTimersByTime(250);
  shell.setElapsedMs(120_000);
  vi.advanceTimersByTime(250);
  events.length = 0;
  vi.advanceTimersByTime(1_000);

  expect(events).toEqual([]);
});

it("urgency tracker snaps back to normal silently on timer reset", () => {
  const { events, shell } = installTrackerForTest({ plannedDurationMs: 300_000 });

  vi.advanceTimersByTime(250);
  shell.setElapsedMs(240_000);
  vi.advanceTimersByTime(250);
  events.length = 0;
  shell.setStartedAt(null);
  shell.setElapsedMs(0);
  vi.advanceTimersByTime(250);
  expect(events).toEqual([]);

  shell.setStartedAt(2);
  shell.setElapsedMs(0);
  vi.advanceTimersByTime(250);
  shell.setElapsedMs(120_000);
  vi.advanceTimersByTime(250);

  expect(events).toEqual([{ from: "normal", to: "warning" }]);
});

it("urgency tracker destroy stops ticking", () => {
  const { events, shell } = installTrackerForTest({ plannedDurationMs: 300_000 });

  trackers.pop()?.destroy();
  shell.setElapsedMs(300_001);
  vi.advanceTimersByTime(1_000);

  expect(events).toEqual([]);
});
