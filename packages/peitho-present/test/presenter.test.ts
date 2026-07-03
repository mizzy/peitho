import { afterEach, expect, it, vi } from "vitest";
import { mountPresenterView } from "../src/index";
import type { PresenterView, SyncChannel, SyncChannelFactory } from "../src/index";
import type { Notes } from "../../../bindings/Notes";

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
  plannedDurationMs: null,
  slides: [
    { index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false },
    { index: 1, key: "details", src: "slides/001-details.html", hasNotes: false }
  ]
};

const notes: Notes = { version: 1, notes: { intro: "Opening note" } };

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
});

it("renders current slide preview next slide note and starts timer from Start button", async () => {
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
  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("00:00");
  root.querySelector<HTMLButtonElement>('[data-peitho-action="start"]')?.click();
  now = 65000;
  view.tick();
  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("01:04");
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

it("buttons emit navigate and timercontrol requests only", async () => {
  const root = document.createElement("main");
  const { channel, factory } = mockSyncChannelFactory();
  const closeWindow = vi.fn();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory,
    closeWindow
  });
  views.push(view);
  const events: unknown[] = [];
  const onNavigate = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  const onTimerControl = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  window.addEventListener("peitho:navigate", onNavigate);
  window.addEventListener("peitho:timercontrol", onTimerControl);
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));

  root.querySelector<HTMLButtonElement>('[data-peitho-action="next"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="start"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="pause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="reset"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="close"]')?.click();

  expect(events).toEqual([
    { to: "next" },
    { action: "start" },
    { action: "pause" },
    { action: "reset" }
  ]);
  expect(channel.sent).toEqual([{ index: 1 }]);
  expect(closeWindow).toHaveBeenCalledTimes(1);
});
