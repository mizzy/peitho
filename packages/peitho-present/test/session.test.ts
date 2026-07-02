import { afterEach, expect, it, vi } from "vitest";
import { mountPresentShell } from "../src/index";
import type { PresentShell } from "../src/index";

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function okText(value: string): Response {
  return { ok: true, status: 200, text: async () => value } as Response;
}

const manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 2,
  slides: [
    { index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false },
    { index: 1, key: "details", src: "slides/001-details.html", hasNotes: false }
  ]
};

function standardFetch(): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-details.html")
      return okText("<section><h1>Details</h1></section>");
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
}

const shells: PresentShell[] = [];

afterEach(() => {
  while (shells.length > 0) shells.pop()?.destroy();
});

it("isolates navigate events by shell bus", async () => {
  const rootA = document.createElement("main");
  const rootB = document.createElement("main");
  const busA = new EventTarget();
  const busB = new EventTarget();
  const shellA = await mountPresentShell({
    root: rootA,
    fetcher: standardFetch(),
    window,
    bus: busA
  });
  const shellB = await mountPresentShell({
    root: rootB,
    fetcher: standardFetch(),
    window,
    bus: busB
  });
  shells.push(shellA, shellB);

  busA.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(shellA.currentIndex).toBe(1);
  expect(shellB.currentIndex).toBe(0);
});

it("dispatches presentationstart after mount with total and startedAt", async () => {
  const now = 1000;
  const starts: unknown[] = [];
  const bus = new EventTarget();
  bus.addEventListener("peitho:presentationstart", (event) =>
    starts.push((event as CustomEvent).detail)
  );

  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  shells.push(shell);

  expect(starts).toEqual([{ total: 2, startedAt: 1000 }]);
  expect(shell.startedAt()).toBe(1000);
});

it("pauses resumes and resets elapsed time from timercontrol events", async () => {
  let now = 1000;
  const bus = new EventTarget();
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  shells.push(shell);

  now = 1500;
  expect(shell.elapsedMs()).toBe(500);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  expect(shell.isPaused()).toBe(true);
  now = 2500;
  expect(shell.elapsedMs()).toBe(500);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "resume" } }));
  expect(shell.isPaused()).toBe(false);
  now = 3000;
  expect(shell.elapsedMs()).toBe(1000);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } }));
  expect(shell.startedAt()).toBe(3000);
  expect(shell.elapsedMs()).toBe(0);
});

it("dispatches presentationend once for pagehide and destroy", async () => {
  let now = 1000;
  const bus = new EventTarget();
  const ends: unknown[] = [];
  bus.addEventListener("peitho:presentationend", (event) =>
    ends.push((event as CustomEvent).detail)
  );
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });

  now = 1750;
  window.dispatchEvent(new Event("pagehide"));
  shell.destroy();

  expect(ends).toEqual([{ endedAt: 1750, elapsedMs: 750 }]);
});
