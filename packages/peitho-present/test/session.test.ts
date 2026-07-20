import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Manifest } from "../../../bindings/Manifest";
import { mountPresentShell } from "../src/index";
import type { PresentShell } from "../src/index";

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function okText(value: string): Response {
  return { ok: true, status: 200, text: async () => value } as Response;
}

const manifest: Manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 2,
  plannedDurationMs: null,
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
      text: { title: "", body: "", code: "" }
    },
    {
      index: 1,
      key: "details",
      src: "slides/001-details.html",
      hasNotes: false,
      skip: false,
      text: { title: "", body: "", code: "" }
    }
  ],
  images: []
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

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    (() => null) as HTMLCanvasElement["getContext"]
  );
});

afterEach(() => {
  while (shells.length > 0) shells.pop()?.destroy();
  vi.restoreAllMocks();
});

it("does not start the presentation on mount", async () => {
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
    now: () => 1000
  });
  shells.push(shell);

  expect(starts).toEqual([]);
  expect(shell.startedAt()).toBeNull();
  expect(shell.elapsedMs()).toBe(0);
  expect(shell.isPaused()).toBe(false);
});

it("starts once from timercontrol start and ignores duplicate start", async () => {
  let now = 1000;
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

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  now = 1750;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));

  expect(starts).toEqual([{ total: 2, startedAt: 1000 }]);
  expect(shell.startedAt()).toBe(1000);
  expect(shell.elapsedMs()).toBe(750);
});

it("pauses resumes and resets only after a manual start", async () => {
  let now = 1000;
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

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  expect(shell.isPaused()).toBe(false);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  now = 1500;
  expect(shell.elapsedMs()).toBe(500);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  expect(shell.isPaused()).toBe(true);
  now = 2500;
  expect(shell.elapsedMs()).toBe(500);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "resume" } }));
  now = 3000;
  expect(shell.elapsedMs()).toBe(1000);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } }));
  expect(shell.startedAt()).toBeNull();
  expect(shell.elapsedMs()).toBe(0);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  expect(starts).toEqual([
    { total: 2, startedAt: 1000 },
    { total: 2, startedAt: 3000 }
  ]);
});

it("adopts absolute timer state while stopped running and paused", async () => {
  let now = 10_000;
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    now: () => now
  });
  shells.push(shell);

  shell.adoptTimerState({ running: false, elapsedMs: 0 });
  expect(shell.startedAt()).toBeNull();
  expect(shell.isPaused()).toBe(false);
  expect(shell.elapsedMs()).toBe(0);

  shell.adoptTimerState({ running: true, elapsedMs: 2_000 });
  expect(shell.startedAt()).toBe(8_000);
  expect(shell.isPaused()).toBe(false);
  expect(shell.elapsedMs()).toBe(2_000);
  now = 11_500;
  expect(shell.elapsedMs()).toBe(3_500);

  shell.adoptTimerState({ running: false, elapsedMs: 4_000 });
  expect(shell.startedAt()).toBe(7_500);
  expect(shell.isPaused()).toBe(true);
  expect(shell.elapsedMs()).toBe(4_000);
  now = 20_000;
  expect(shell.elapsedMs()).toBe(4_000);

  shell.adoptTimerState({ running: true, elapsedMs: 6_000 });
  expect(shell.startedAt()).toBe(14_000);
  expect(shell.isPaused()).toBe(false);
  expect(shell.elapsedMs()).toBe(6_000);
});

it("emits timeradopt without emitting timerchange when adopting absolute state", async () => {
  const bus = new EventTarget();
  const adopted: unknown[] = [];
  const changes: unknown[] = [];
  let now = 10_000;
  bus.addEventListener("peitho:timeradopt", (event) =>
    adopted.push((event as CustomEvent).detail)
  );
  bus.addEventListener("peitho:timerchange", (event) =>
    changes.push((event as CustomEvent).detail)
  );
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  shells.push(shell);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  now = 12_500;
  adopted.length = 0;
  changes.length = 0;
  shell.adoptTimerState({ running: true, elapsedMs: 4_000 });

  expect(adopted).toEqual([{ running: true, elapsedMs: 4_000, previousElapsedMs: 2_500 }]);
  expect(changes).toEqual([]);
});

it("uses an injected manifest without fetching manifest.json", async () => {
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") return { ok: false, status: 500, text: async () => "" } as Response;
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-details.html")
      return okText("<section><h1>Details</h1></section>");
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    manifest,
    fetcher,
    window
  });
  shells.push(shell);

  expect(shell.manifest?.title).toBe("Demo");
  expect(fetcher).not.toHaveBeenCalledWith("manifest.json");
});

it("emits presentationend only once and only after start", async () => {
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

  shell.destroy();
  expect(ends).toEqual([]);

  const startedShell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  now = 1750;
  window.dispatchEvent(new Event("pagehide"));
  startedShell.destroy();

  expect(ends).toEqual([{ endedAt: 1750, elapsedMs: 750 }]);
});
