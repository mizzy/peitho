import { afterEach, expect, it, vi } from "vitest";
import { mountPresenterView } from "../src/index";
import type { PresenterView, SyncChannel, SyncChannelFactory } from "../src/index";
import type { Manifest } from "../../../bindings/Manifest";
import type { Notes } from "../../../bindings/Notes";

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
  slides: [
    { index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false },
    { index: 1, key: "details", src: "slides/001-details.html", hasNotes: false }
  ]
};

const notes: Notes = { version: 1, notes: { intro: "Opening note" } };

function standardFetch(overrides: Partial<typeof manifest> = {}): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(Object.assign({}, manifest, overrides));
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-details.html")
      return okText("<section><h1>Details</h1></section>");
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
}

function mockSyncChannelFactory() {
  const channel: SyncChannel & { sent: unknown[]; closed: boolean } = {
    sent: [],
    closed: false,
    onmessage: null,
    postMessage(message: unknown) {
      this.sent.push(message);
    },
    close() {
      this.closed = true;
    }
  };
  const factory: SyncChannelFactory = () => channel;
  return { channel, factory };
}

function sizeElement(element: HTMLElement, width: number, height: number): void {
  Object.defineProperty(element, "clientWidth", { value: width, configurable: true });
  Object.defineProperty(element, "clientHeight", { value: height, configurable: true });
}

const views: PresenterView[] = [];
const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  while (views.length > 0) views.pop()?.destroy();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it("renders the redesigned presenter shell and starts timer from the playpause button", async () => {
  let now = 1000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);

  expect(root.querySelector('[data-peitho-presenter="current"] .peitho-slide')).not.toBeNull();
  expect(
    root.querySelector<HTMLElement>(
      '[data-peitho-presenter="preview"] [data-slide-index="1"]'
    )?.shadowRoot?.textContent
  ).toContain("Details");
  expect(root.querySelector('[data-peitho-presenter="notes"]')?.textContent).toContain(
    "Opening note"
  );
  expect(root.querySelector(".left")).not.toBeNull();
  expect(root.querySelector(".right")).not.toBeNull();
  expect(root.querySelector(".agenda")).toBeNull();
  expect(root.querySelector(".status-line")?.textContent).not.toContain("Section");
  expect(root.querySelector(".kbdbar")?.textContent).toContain("Space");

  const clock = root.querySelector<HTMLElement>('[data-peitho-presenter="clock"]')!;
  const pill = root.querySelector<HTMLElement>('[data-peitho-presenter="state-pill"]')!;
  const play = root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')!;
  expect(root.querySelector('[data-peitho-action="start"]')).toBeNull();
  expect(root.querySelector('[data-peitho-action="pause"]')).toBeNull();
  expect(root.querySelector('[data-peitho-action="resume"]')).toBeNull();
  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("00:00");
  expect(clock.dataset.peithoState).toBe("stopped");
  expect(pill.dataset.peithoState).toBe("stopped");
  expect(pill.textContent).toContain("Stopped");
  expect(play.textContent).toBe("Start");
  expect(play.textContent).not.toContain("Space");

  play.click();
  now = 65000;
  view.tick();
  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("01:04");
  expect(clock.dataset.peithoState).toBe("running");
  expect(pill.dataset.peithoState).toBe("running");
  expect(pill.textContent).toContain("Running");
  expect(play.textContent).toBe("Pause");
});

it("shows planned duration in presenter timer when manifest has time", async () => {
  let now = 1_000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ plannedDurationMs: 60_000 }),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  now = 31_000;
  view.tick();

  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("00:30 / 01:00");
  expect(
    root.querySelector(
      '[data-peitho-presenter="clock"] [data-peitho-presenter="tracker-slot"] > [data-peitho-time-tracker="presenter"]'
    )
  ).not.toBeNull();

  view.destroy();
  views.pop();
  expect(root.querySelector("[data-peitho-time-tracker]")).toBeNull();
});

it("keeps legacy presenter timer text when manifest has no time", async () => {
  let now = 1_000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ plannedDurationMs: null }),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  now = 65_000;
  view.tick();

  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("01:04");
  expect(root.querySelector("[data-peitho-time-tracker]")).toBeNull();
});

it("logs invalid planned duration and keeps presenter mounted without a tracker", async () => {
  let now = 1_000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const log = { error: vi.fn() };
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ plannedDurationMs: 0 }),
    window,
    now: () => now,
    syncChannelFactory: factory,
    console: log
  });
  views.push(view);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  now = 65_000;
  view.tick();

  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("01:04");
  expect(root.querySelector("[data-peitho-time-tracker]")).toBeNull();
  expect(log.error).toHaveBeenCalledWith("Invalid plannedDurationMs in manifest.json");
});

it("marks presenter timer as overrun after the planned duration", async () => {
  let now = 1_000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ plannedDurationMs: 60_000 }),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  now = 61_500;
  view.tick();

  const timer = root.querySelector<HTMLElement>('[data-peitho-presenter="timer"]')!;
  expect(timer.textContent).toBe("01:00 / 01:00 +00:01");
  expect(timer.querySelector(".planned")?.textContent).toBe(" / 01:00");
  expect(timer.querySelector(".overrun")?.textContent).toBe(" +00:01");
  expect(timer.hasAttribute("data-peitho-overrun")).toBe(true);
});

it("updates preview and shows end of deck on the last slide", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(root.querySelector('[data-peitho-presenter="notes"]')?.textContent).toContain(
    "No notes for this slide."
  );
  expect(root.querySelector('[data-peitho-presenter="preview-end"]')?.textContent).toContain(
    "End of deck"
  );
});

it("scales current and next preview shells to their pane sizes", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  const currentPane = root.querySelector<HTMLElement>('[data-peitho-presenter="current"]')!;
  const previewPane = root.querySelector<HTMLElement>('[data-peitho-presenter="preview"]')!;
  sizeElement(currentPane, 640, 360);
  sizeElement(previewPane, 320, 180);
  window.dispatchEvent(new Event("resize"));

  expect(currentPane.querySelector<HTMLElement>(".peitho-slide")?.style.transform).toBe(
    "translate(0px, 0px) scale(0.5)"
  );
  expect(previewPane.querySelector<HTMLElement>(".peitho-slide")?.style.transform).toBe(
    "translate(0px, 0px) scale(0.25)"
  );
});

it("buttons emit navigate timercontrol and close requests", async () => {
  const root = document.createElement("main");
  const { channel, factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);
  const events: unknown[] = [];
  const closeRequests: unknown[] = [];
  const onNavigate = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  const onTimerControl = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  const onCloseRequest = (event: Event): void => {
    closeRequests.push((event as CustomEvent).detail);
  };
  window.addEventListener("peitho:navigate", onNavigate);
  window.addEventListener("peitho:timercontrol", onTimerControl);
  window.addEventListener("peitho:closerequest", onCloseRequest);
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));
  cleanups.push(() => window.removeEventListener("peitho:closerequest", onCloseRequest));

  root.querySelector<HTMLButtonElement>('[data-peitho-action="next"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="reset"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="close"]')?.click();

  expect(events).toEqual([
    { to: "next" },
    { action: "start" },
    { action: "pause" },
    { action: "resume" },
    { action: "reset" }
  ]);
  expect(closeRequests).toEqual([null]);
  expect(channel.sent).toEqual([{ index: 1 }, { close: true }]);
});

it("derives playpause action labels and chrome from shell timer state", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);
  const play = root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')!;
  const reset = root.querySelector<HTMLButtonElement>('[data-peitho-action="reset"]')!;
  const clock = root.querySelector<HTMLElement>('[data-peitho-presenter="clock"]')!;
  const pill = root.querySelector<HTMLElement>('[data-peitho-presenter="state-pill"]')!;

  expect(clock.dataset.peithoState).toBe("stopped");
  expect(pill.textContent).toContain("Stopped");
  expect(play.textContent).toBe("Start");

  play.click();
  expect(clock.dataset.peithoState).toBe("running");
  expect(pill.textContent).toContain("Running");
  expect(play.textContent).toBe("Pause");

  play.click();
  expect(clock.dataset.peithoState).toBe("paused");
  expect(pill.textContent).toContain("Paused");
  expect(play.textContent).toBe("Resume");

  play.click();
  expect(clock.dataset.peithoState).toBe("running");
  expect(play.textContent).toBe("Pause");

  reset.click();
  expect(clock.dataset.peithoState).toBe("stopped");
  expect(pill.textContent).toContain("Stopped");
  expect(play.textContent).toBe("Start");
});

it("adds button ripple feedback and clears pending ripple timeout on destroy", async () => {
  vi.useFakeTimers();
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);
  const play = root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')!;
  vi.spyOn(play, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    right: 100,
    bottom: 50,
    width: 100,
    height: 50,
    toJSON: () => ({})
  });

  play.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, clientX: 25, clientY: 10 }));

  expect(play.style.getPropertyValue("--rx")).toBe("25%");
  expect(play.style.getPropertyValue("--ry")).toBe("20%");
  expect(play.classList.contains("pressed")).toBe(true);

  vi.advanceTimersByTime(550);
  expect(play.classList.contains("pressed")).toBe(false);

  play.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, clientX: 75, clientY: 25 }));
  expect(play.classList.contains("pressed")).toBe(true);

  view.destroy();
  views.pop();
  expect(vi.getTimerCount()).toBe(0);
});
