import type { Manifest } from "../../../bindings/Manifest";
import type { RehearsalSnapshot } from "../../../bindings/RehearsalSnapshot";
import { afterEach, expect, it, vi } from "vitest";
import {
  installRehearsalReporter,
  type RehearsalReportDetail
} from "../src/rehearsalReporter";
import { installSlideTimeline } from "../src/slideTimeline";

const manifest: Manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 2,
  plannedDurationMs: 60_000,
  aspectRatio: "16:9",
  canvasWidth: 1280,
  canvasHeight: 720,
  sections: [],
  slides: [
    {
      index: 0,
      key: "intro",
      src: "slides/000-intro.html",
      hasNotes: false,
      skip: false,
      revealSteps: 1,
      text: { title: "", body: "", code: "" }
    },
    {
      index: 1,
      key: "details",
      src: "slides/001-details.html",
      hasNotes: false,
      skip: false,
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    }
  ],
  images: []
};

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.restoreAllMocks();
});

it("records start, navigation, and revisits without recording reveal steps", () => {
  const bus = new EventTarget();
  let currentIndex = 0;
  let elapsedMs = 0;
  let startedAt: number | null = null;
  const timeline = installSlideTimeline({
    shell: {
      manifest,
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsedMs,
      startedAt: () => startedAt
    },
    bus
  });
  cleanups.push(timeline.destroy);

  startedAt = 100;
  bus.dispatchEvent(new CustomEvent("peitho:presentationstart"));
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "intro", index: 0, total: 2, previousIndex: null }
    })
  );
  elapsedMs = 7_000;
  currentIndex = 1;
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "details", index: 1, total: 2, previousIndex: 0 }
    })
  );
  elapsedMs = 8_000;
  bus.dispatchEvent(
    new CustomEvent("peitho:stepchange", { detail: { index: 1, step: 1, stepCount: 1 } })
  );
  elapsedMs = 12_000;
  currentIndex = 0;
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "intro", index: 0, total: 2, previousIndex: 1 }
    })
  );

  expect(timeline.entries()).toEqual([
    { key: "intro", index: 0, atMs: 0 },
    { key: "details", index: 1, atMs: 7_000 },
    { key: "intro", index: 0, atMs: 12_000 }
  ]);
});

it("coalesces only an exact consecutive duplicate", () => {
  const bus = new EventTarget();
  let elapsedMs = 0;
  const timeline = installSlideTimeline({
    shell: {
      manifest,
      currentIndex: 0,
      elapsedMs: () => elapsedMs,
      startedAt: () => 100
    },
    bus
  });
  cleanups.push(timeline.destroy);

  bus.dispatchEvent(new CustomEvent("peitho:presentationstart"));
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "intro", index: 0, total: 2, previousIndex: null }
    })
  );
  elapsedMs = 1;
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "intro", index: 0, total: 2, previousIndex: 1 }
    })
  );

  expect(timeline.entries()).toEqual([
    { key: "intro", index: 0, atMs: 0 },
    { key: "intro", index: 0, atMs: 1 }
  ]);
});

it("seeds an empty run at the first positive adopted timer position", () => {
  const bus = new EventTarget();
  const timeline = installSlideTimeline({
    shell: {
      manifest,
      currentIndex: 1,
      elapsedMs: () => 7_000.4,
      startedAt: () => 100
    },
    bus
  });
  cleanups.push(timeline.destroy);

  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: true, previousElapsedMs: 0, elapsedMs: 7_000.4 }
    })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: false, previousElapsedMs: 7_000.4, elapsedMs: 7_000.4 }
    })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: true, previousElapsedMs: 7_000.4, elapsedMs: 7_000.4 }
    })
  );

  expect(timeline.entries()).toEqual([{ key: "details", index: 1, atMs: 7_000 }]);
});

it("seeds and starts tracking when a running timer is adopted at zero", () => {
  const bus = new EventTarget();
  let currentIndex = 0;
  let elapsedMs = 0;
  const timeline = installSlideTimeline({
    shell: {
      manifest,
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsedMs,
      startedAt: () => null
    },
    bus
  });
  cleanups.push(timeline.destroy);

  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: true, previousElapsedMs: 0, elapsedMs: 0 }
    })
  );
  currentIndex = 1;
  elapsedMs = 500;
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "details", index: 1, total: 2, previousIndex: 0 }
    })
  );

  expect(timeline.entries()).toEqual([
    { key: "intro", index: 0, atMs: 0 },
    { key: "details", index: 1, atMs: 500 }
  ]);
});

it("clamps entries when an adopted timer moves backwards", () => {
  const bus = new EventTarget();
  let currentIndex = 0;
  let elapsedMs = 0;
  const timeline = installSlideTimeline({
    shell: {
      manifest,
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsedMs,
      startedAt: () => 100
    },
    bus
  });
  cleanups.push(timeline.destroy);
  const reports: RehearsalSnapshot[] = [];
  const reporterCleanup = installRehearsalReporter({
    actuals: { actualMs: () => [elapsedMs], flush: vi.fn() },
    timeline,
    shell: {
      elapsedMs: () => elapsedMs,
      startedAt: () => 100,
      isPaused: () => false
    },
    sections: [{ name: "Setup", startIndex: 0, endIndex: 1, plannedDurationMs: 60_000 }],
    bus,
    window
  });
  cleanups.push(reporterCleanup);
  bus.addEventListener("peitho:rehearsalreport", (event) => {
    reports.push((event as CustomEvent<RehearsalReportDetail>).detail.snapshot);
  });

  bus.dispatchEvent(new CustomEvent("peitho:presentationstart"));
  currentIndex = 1;
  elapsedMs = 10_020;
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "details", index: 1, total: 2, previousIndex: 0 }
    })
  );

  elapsedMs = 9_980;
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: false, previousElapsedMs: 10_020, elapsedMs: 9_980 }
    })
  );
  expect(timeline.entries()).toEqual([
    { key: "intro", index: 0, atMs: 0 },
    { key: "details", index: 1, atMs: 9_980 }
  ]);

  currentIndex = 0;
  elapsedMs = 9_990;
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "intro", index: 0, total: 2, previousIndex: 1 }
    })
  );

  const report = reports.at(-1)!;
  expect(report.timeline).toEqual([
    { key: "intro", index: 0, atMs: 0 },
    { key: "details", index: 1, atMs: 9_980 },
    { key: "intro", index: 0, atMs: 9_990 }
  ]);
  expect(report.timeline.every((entry) => entry.atMs <= report.elapsedMs)).toBe(true);
  expect(
    report.timeline.every(
      (entry, index) => index === 0 || entry.atMs >= report.timeline[index - 1]!.atMs
    )
  ).toBe(true);
});

it("clears on local and adopted reset", () => {
  const bus = new EventTarget();
  const timeline = installSlideTimeline({
    shell: {
      manifest,
      currentIndex: 0,
      elapsedMs: () => 0,
      startedAt: () => 100
    },
    bus
  });
  cleanups.push(timeline.destroy);

  bus.dispatchEvent(new CustomEvent("peitho:presentationstart"));
  bus.dispatchEvent(
    new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } })
  );
  expect(timeline.entries()).toEqual([]);

  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: true, previousElapsedMs: 0, elapsedMs: 5_000 }
    })
  );
  expect(timeline.entries()).toEqual([{ key: "intro", index: 0, atMs: 5_000 }]);
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: false, previousElapsedMs: 5_000, elapsedMs: 0 }
    })
  );
  expect(timeline.entries()).toEqual([]);
});

it("returns copies, rejects invalid slide events, and removes listeners on destroy", () => {
  const bus = new EventTarget();
  const log = { error: vi.fn() };
  const timeline = installSlideTimeline({
    shell: {
      manifest,
      currentIndex: 0,
      elapsedMs: () => 0,
      startedAt: () => 100
    },
    bus,
    log
  });
  cleanups.push(timeline.destroy);

  bus.dispatchEvent(new CustomEvent("peitho:presentationstart"));
  const copy = timeline.entries();
  copy[0]!.key = "changed";
  copy.push({ key: "details", index: 1, atMs: 1 });
  expect(timeline.entries()).toEqual([{ key: "intro", index: 0, atMs: 0 }]);

  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "wrong", index: 1, total: 2, previousIndex: 0 }
    })
  );
  expect(log.error).toHaveBeenCalledWith("Invalid peitho:slidechange event");

  timeline.destroy();
  cleanups.pop();
  bus.dispatchEvent(
    new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } })
  );
  expect(timeline.entries()).toEqual([{ key: "intro", index: 0, atMs: 0 }]);
});
