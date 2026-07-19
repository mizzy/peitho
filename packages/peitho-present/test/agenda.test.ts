import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  type PresentShell,
  type SlideChangeDetail,
  type TimerControlDetail
} from "../src/index";
import { installAgenda as installAgendaImpl, type AgendaOptions } from "../src/agenda";
import { installSectionActuals } from "../src/sectionActuals";

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

type TestAgendaOptions = Omit<AgendaOptions, "actuals"> & {
  actuals?: AgendaOptions["actuals"];
};

function installAgenda(options: TestAgendaOptions): () => void {
  if (options.actuals) return installAgendaImpl(options as AgendaOptions);
  const actuals = installSectionActuals({
    shell: options.shell,
    sections: options.sections,
    bus: options.bus,
    window: options.window,
    log: options.log
  });
  const cleanupAgenda = installAgendaImpl({ ...options, actuals });
  return () => {
    cleanupAgenda();
    actuals.destroy();
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

it("does not mount agenda when sections are empty", () => {
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 0 }),
    sections: [],
    bus: new EventTarget(),
    window,
    document
  });
  cleanups.push(cleanup);

  expect(root.innerHTML).toBe("");
});

it("does not mount agenda and logs when a section has invalid planned duration", () => {
  const root = document.createElement("div");
  const log = { error: vi.fn() };
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 0 }),
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
      {
        name: "Demo",
        startIndex: 1,
        endIndex: 1,
        plannedDurationMs: Number.MAX_SAFE_INTEGER + 1
      }
    ],
    bus: new EventTarget(),
    window,
    document,
    log
  });
  cleanups.push(cleanup);

  expect(log.error).toHaveBeenCalledWith(
    'Invalid plannedDurationMs for manifest section 2 "Demo" in manifest.json'
  );
  expect(root.innerHTML).toBe("");
});

it("does not mount agenda and logs when sections do not tile slide indexes", () => {
  const cases = [
    {
      sections: [{ name: "Late", startIndex: 1, endIndex: 1, plannedDurationMs: 1_000 }],
      message: 'Invalid manifest section 1 "Late": expected startIndex 0, got 1'
    },
    {
      sections: [
        { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 1_000 },
        { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 1_000 }
      ],
      message: 'Invalid manifest section 2 "Demo": expected startIndex 1, got 2'
    },
    {
      sections: [{ name: "Bad", startIndex: 0, endIndex: 1.5, plannedDurationMs: 1_000 }],
      message:
        'Invalid manifest section 1 "Bad": startIndex and endIndex must be non-negative integers'
    }
  ];

  for (const { sections, message } of cases) {
    const root = document.createElement("div");
    const log = { error: vi.fn() };
    const cleanup = installAgenda({
      root,
      shell: shell({ currentIndex: 0 }),
      sections,
      bus: new EventTarget(),
      window,
      document,
      log
    });
    cleanups.push(cleanup);

    expect(log.error).toHaveBeenCalledWith(message);
    expect(root.innerHTML).toBe("");
  }
});

it("renders agenda header and rows with mock-compatible structure", () => {
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 2 }),
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 1, plannedDurationMs: 60_000 },
      { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 120_000 }
    ],
    bus: new EventTarget(),
    window,
    document
  });
  cleanups.push(cleanup);

  expect(root.querySelector("[data-peitho-agenda-title]")?.textContent).toBe("Agenda");
  expect(root.querySelector("[data-peitho-agenda-hint]")?.textContent).toBe("Actual / Planned");
  const rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows.map((row) => row.dataset.peithoAgendaState)).toEqual(["done", "current"]);
  expect(rows[0].children.length).toBe(4);
  expect(rows[0].querySelector("[data-peitho-agenda-marker]")).not.toBeNull();
  expect(rows[0].querySelector("[data-peitho-agenda-label]")).not.toBeNull();
  expect(rows[0].querySelector("[data-peitho-agenda-name]")?.textContent).toBe("Setup");
  expect(rows[0].querySelector("[data-peitho-agenda-range]")?.textContent).toBe("01–02");
  expect(rows[0].dataset.peithoAgendaOutcome).toBe("under");
  expect(rows[0].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("−1:00");
  expect(rows[1].querySelector("[data-peitho-agenda-range]")?.textContent).toBe("03");
  expect(rows[1].querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:00 / 2:00");
  expect(rows[1].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("·");
  expect(rows[1].hasAttribute("data-peitho-agenda-outcome")).toBe(false);
});

it("renders matching rehearsal baseline actuals per section", () => {
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 0 }),
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 1, plannedDurationMs: 60_000 },
      { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 120_000 }
    ],
    rehearsal: {
      version: 1,
      lastRun: {
        version: 1,
        recordedAtMs: 1_783_000_000_000,
        elapsedMs: 200_000,
        sections: [
          { name: "Setup", plannedDurationMs: 60_000, actualMs: 52_000 },
          { name: "Demo", plannedDurationMs: 120_000, actualMs: 128_000 }
        ]
      }
    },
    bus: new EventTarget(),
    window,
    document
  });
  cleanups.push(cleanup);

  const last = Array.from(
    root.querySelectorAll("[data-peitho-agenda-last]"),
    (node) => node.textContent
  );
  expect(last).toEqual(["(last 0:52)", "(last 2:08)"]);
});

it("omits rehearsal comparison and warns when section names or plans differ", () => {
  const root = document.createElement("div");
  const log = { error: vi.fn(), warn: vi.fn() };
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 0 }),
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    rehearsal: {
      version: 1,
      lastRun: {
        version: 1,
        recordedAtMs: 1_783_000_000_000,
        elapsedMs: 2_000,
        sections: [{ name: "Setup", plannedDurationMs: 61_000, actualMs: 2_000 }]
      }
    },
    bus: new EventTarget(),
    window,
    document,
    log
  });
  cleanups.push(cleanup);

  expect(root.querySelector("[data-peitho-agenda-last]")).toBeNull();
  expect(log.warn).toHaveBeenCalledWith(
    "Last rehearsal does not match the current agenda; deck may have been edited since the rehearsal"
  );
});

it("does not render rehearsal comparison when there is no last run", () => {
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 0 }),
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    rehearsal: { version: 1, lastRun: null },
    bus: new EventTarget(),
    window,
    document
  });
  cleanups.push(cleanup);

  expect(root.querySelector("[data-peitho-agenda-last]")).toBeNull();
});

it("renders never-visited upcoming sections with a dash actual", () => {
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 0 }),
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 1_000 },
      { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 2_000 }
    ],
    bus: new EventTarget(),
    window,
    document
  });
  cleanups.push(cleanup);

  const rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[1].dataset.peithoAgendaState).toBe("upcoming");
  expect(rows[1].querySelector("[data-peitho-agenda-time]")?.textContent).toBe("— / 0:02");
});

it("marks the current agenda row over only after exceeding planned duration", () => {
  let elapsed = 0;
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ elapsedMs: () => elapsed }),
    sections: [{ name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus: new EventTarget(),
    window,
    document
  });
  cleanups.push(cleanup);

  const row = root.querySelector<HTMLElement>("[data-peitho-agenda-row]")!;
  elapsed = 30_000;
  vi.advanceTimersByTime(250);
  expect(row.dataset.peithoAgendaState).toBe("current");
  expect(row.dataset.peithoAgendaOutcome).toBeUndefined();

  elapsed = 70_000;
  vi.advanceTimersByTime(250);
  expect(row.dataset.peithoAgendaState).toBe("current");
  expect(row.dataset.peithoAgendaOutcome).toBe("over");
});

it("keeps done agenda row outcomes for under and over durations", () => {
  let elapsed = 0;
  let currentIndex = 0;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: {
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsed,
      startedAt: () => 100
    },
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
      { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 60_000 },
      { name: "Close", startIndex: 2, endIndex: 2, plannedDurationMs: 60_000 }
    ],
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  elapsed = 30_000;
  vi.advanceTimersByTime(250);
  currentIndex = 1;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "demo", index: 1, total: 3, previousIndex: 0 }
    })
  );
  elapsed = 100_000;
  vi.advanceTimersByTime(250);
  currentIndex = 2;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "close", index: 2, total: 3, previousIndex: 1 }
    })
  );

  const rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows.map((row) => row.dataset.peithoAgendaState)).toEqual([
    "done",
    "done",
    "current"
  ]);
  expect(rows[0].dataset.peithoAgendaOutcome).toBe("under");
  expect(rows[1].dataset.peithoAgendaOutcome).toBe("over");
});

it("accumulates elapsed deltas into the current section and resumes when returning", () => {
  let elapsed = 0;
  let currentIndex = 0;
  let timerStarted = true;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: {
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsed,
      startedAt: () => (timerStarted ? 100 : null)
    },
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 1, plannedDurationMs: 1_000 },
      { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 1_000 }
    ],
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  elapsed = 1_000;
  vi.advanceTimersByTime(250);
  currentIndex = 2;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "demo", index: 2, total: 3, previousIndex: 0 }
    })
  );

  let rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].dataset.peithoAgendaState).toBe("done");
  expect(rows[0].dataset.peithoAgendaOutcome).toBe("under");
  expect(rows[0].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("−0:00");
  expect(rows[1].dataset.peithoAgendaState).toBe("current");
  expect(rows[1].hasAttribute("data-peitho-agenda-outcome")).toBe(false);

  elapsed = 3_000;
  vi.advanceTimersByTime(250);
  currentIndex = 0;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "setup", index: 0, total: 3, previousIndex: 2 }
    })
  );
  elapsed = 4_000;
  vi.advanceTimersByTime(250);

  const times = Array.from(
    root.querySelectorAll("[data-peitho-agenda-time]"),
    (node) => node.textContent
  );
  expect(times).toEqual(["0:02 / 0:01", "0:02 / 0:01"]);

  rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].dataset.peithoAgendaState).toBe("current");
  expect(rows[0].dataset.peithoAgendaOutcome).toBe("over");
  expect(rows[1].dataset.peithoAgendaState).toBe("upcoming");
  expect(rows[1].hasAttribute("data-peitho-agenda-outcome")).toBe(false);

  currentIndex = 2;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "demo", index: 2, total: 3, previousIndex: 0 }
    })
  );
  rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].dataset.peithoAgendaOutcome).toBe("over");
  expect(rows[0].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("+0:01");

  currentIndex = 0;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "setup", index: 0, total: 3, previousIndex: 2 }
    })
  );
  elapsed = 0;
  timerStarted = false;
  bus.dispatchEvent(
    new CustomEvent<TimerControlDetail>("peitho:timercontrol", {
      detail: { action: "reset" }
    })
  );
  vi.advanceTimersByTime(250);
  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:00 / 0:01");
  const resetTimes = Array.from(
    root.querySelectorAll("[data-peitho-agenda-time]"),
    (node) => node.textContent
  );
  expect(resetTimes[1]).toBe("— / 0:01");
});

it("clears actuals immediately when the timer is reset before restarting", () => {
  let elapsed = 0;
  let currentIndex = 0;
  let startedAt: number | null = 100;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: {
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsed,
      startedAt: () => startedAt
    },
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 1_000 },
      { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 1_000 }
    ],
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  elapsed = 1_500;
  vi.advanceTimersByTime(250);
  currentIndex = 1;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "demo", index: 1, total: 2, previousIndex: 0 }
    })
  );

  bus.dispatchEvent(
    new CustomEvent<TimerControlDetail>("peitho:timercontrol", {
      detail: { action: "reset" }
    })
  );
  startedAt = 200;
  elapsed = 400;
  vi.advanceTimersByTime(250);

  const rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:00 / 0:01");
  expect(rows[0].dataset.peithoAgendaOutcome).toBe("under");
  expect(rows[0].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("−0:01");
  expect(rows[1].querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:00 / 0:01");
});

it("keeps actuals stable while paused with a non-null startedAt", () => {
  let elapsed = 0;
  let startedAt: number | null = 100;
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: {
      currentIndex: 0,
      elapsedMs: () => elapsed,
      startedAt: () => startedAt
    },
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 2_000 }],
    bus: new EventTarget(),
    window,
    document
  });
  cleanups.push(cleanup);

  elapsed = 1_000;
  vi.advanceTimersByTime(250);
  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:01 / 0:02");

  // Paused means elapsed is frozen while startedAt stays non-null.
  expect(startedAt).not.toBeNull();
  vi.advanceTimersByTime(750);

  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:01 / 0:02");
});

it("rebases elapsed after timer adoption without attributing the adopted delta", () => {
  let elapsed = 0;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: {
      currentIndex: 0,
      elapsedMs: () => elapsed,
      startedAt: () => 100
    },
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  elapsed = 1_000;
  vi.advanceTimersByTime(250);
  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:01 / 1:00");

  elapsed = 1_201_000;
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: true, previousElapsedMs: 1_000, elapsedMs: 1_201_000 }
    })
  );
  vi.advanceTimersByTime(250);

  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:01 / 1:00");
});

it("clears actuals immediately when a timer reset is adopted", () => {
  let elapsed = 0;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: {
      currentIndex: 0,
      elapsedMs: () => elapsed,
      startedAt: () => 100
    },
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  elapsed = 1_000;
  vi.advanceTimersByTime(250);
  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:01 / 1:00");

  elapsed = 0;
  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: false, previousElapsedMs: 1_000, elapsedMs: 0 }
    })
  );

  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:00 / 1:00");
});

it("attributes pending elapsed time before continuous timer adopt rebases", () => {
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: shell({ currentIndex: 0, startedAt: () => 100 }),
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 10_000 }],
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

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

  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:02 / 0:10");
});

it("flushes pending elapsed time to the previous section on slidechange", () => {
  let elapsed = 0;
  let currentIndex = 0;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: {
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsed,
      startedAt: () => 100
    },
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 1_000 },
      { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 1_000 }
    ],
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  elapsed = 1_000;
  currentIndex = 1;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "demo", index: 1, total: 2, previousIndex: 0 }
    })
  );

  const rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:01 / 0:01");
  expect(rows[0].dataset.peithoAgendaOutcome).toBe("under");
  expect(rows[0].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("−0:00");
  expect(rows[1].querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:00 / 0:01");
});

it("uses rounded seconds as the single source for done outcome and delta text", () => {
  let elapsed = 0;
  let currentIndex = 0;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: {
      get currentIndex() {
        return currentIndex;
      },
      elapsedMs: () => elapsed,
      startedAt: () => 100
    },
    sections: [
      { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 1_000 },
      { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 1_000 }
    ],
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  elapsed = 1_300;
  vi.advanceTimersByTime(250);
  currentIndex = 1;
  bus.dispatchEvent(
    new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
      detail: { key: "demo", index: 1, total: 2, previousIndex: 0 }
    })
  );

  const rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].dataset.peithoAgendaOutcome).toBe("under");
  expect(rows[0].querySelector("[data-peitho-agenda-delta]")?.textContent).toBe("−0:00");
});

it("updates existing row DOM in place during ticks", () => {
  let elapsed = 0;
  const root = document.createElement("div");
  const cleanup = installAgenda({
    root,
    shell: shell({ elapsedMs: () => elapsed }),
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 1_000 }],
    bus: new EventTarget(),
    window,
    document
  });
  cleanups.push(cleanup);

  const row = root.querySelector<HTMLElement>("[data-peitho-agenda-row]");
  const time = root.querySelector<HTMLElement>("[data-peitho-agenda-time]");
  elapsed = 1_000;
  vi.advanceTimersByTime(250);

  expect(root.querySelector("[data-peitho-agenda-row]")).toBe(row);
  expect(root.querySelector("[data-peitho-agenda-time]")).toBe(time);
  expect(time?.textContent).toBe("0:01 / 0:01");
});

it("removes agenda interval listener and DOM on cleanup", () => {
  let elapsed = 0;
  const root = document.createElement("div");
  const bus = new EventTarget();
  const cleanup = installAgenda({
    root,
    shell: shell({ elapsedMs: () => elapsed }),
    sections: [{ name: "Only", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }],
    actuals: { actualMs: () => [0] },
    bus,
    window,
    document
  });
  cleanups.push(cleanup);

  expect(vi.getTimerCount()).toBe(1);
  cleanup();
  cleanups.pop();
  elapsed = 60_000;
  vi.advanceTimersByTime(250);
  bus.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "only", index: 0, total: 1, previousIndex: null }
    })
  );

  expect(vi.getTimerCount()).toBe(0);
  expect(root.querySelector("[data-peitho-agenda]")).toBeNull();
});
