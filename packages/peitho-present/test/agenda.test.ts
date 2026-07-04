import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  installAgenda,
  type PresentShell,
  type SlideChangeDetail,
  type TimerControlDetail
} from "../src/index";

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

it("accumulates elapsed deltas into the current section and resumes when returning", () => {
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
  expect(times).toEqual(["0:02 / 0:01", "— / 0:01"]);

  rows = Array.from(root.querySelectorAll<HTMLElement>("[data-peitho-agenda-row]"));
  expect(rows[0].dataset.peithoAgendaState).toBe("current");
  expect(rows[0].hasAttribute("data-peitho-agenda-outcome")).toBe(false);
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

  startedAt = null;
  elapsed = 0;
  vi.advanceTimersByTime(250);
  expect(root.querySelector("[data-peitho-agenda-time]")?.textContent).toBe("0:00 / 0:01");
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
