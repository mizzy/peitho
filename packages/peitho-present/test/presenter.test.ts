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

const notes: Notes = { version: 1, notes: { intro: "Opening note" } };

function standardFetch(overrides: Partial<typeof manifest> = {}): typeof fetch {
  const responseManifest = Object.assign({}, manifest, overrides) as Manifest;
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(responseManifest);
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-details.html")
      return okText("<section><h1>Details</h1></section>");
    const slide = responseManifest.slides.find((item) => item.src === url);
    if (slide) return okText(`<section><h1>${slide.key}</h1></section>`);
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
}

function manifestWithSlides(slides: Array<{ key: string; skip?: boolean }>): Manifest {
  return {
    ...manifest,
    slideCount: slides.length,
    slides: slides.map((slide, index) => ({
      index,
      key: slide.key,
      src: `slides/${String(index).padStart(3, "0")}-${slide.key}.html`,
      hasNotes: false,
      skip: slide.skip ?? false,
      text: { title: "", body: "", code: "" }
    }))
  };
}

function legacyManifestFetch(): typeof fetch {
  const legacyManifest: Omit<Manifest, "images"> = {
    version: 1,
    peithoVersion: "0.1.0",
    title: "Legacy",
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
    ]
  };

  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(legacyManifest);
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
  const notesEl = root.querySelector<HTMLElement>('[data-peitho-presenter="notes"]');
  expect(notesEl?.textContent).toContain("Opening note");
  expect(notesEl?.classList.contains("is-empty")).toBe(false);
  expect(root.querySelector(".left")).not.toBeNull();
  expect(root.querySelector(".right")).not.toBeNull();
  expect(
    Array.from(root.querySelector(".stage")?.children ?? []).map((child) => child.className)
  ).toEqual(["colhead", "slide-frame", "kbdbar", "notes"]);
  expect(root.querySelector(".agenda")).toBeNull();
  expect(root.querySelector(".status-line")?.textContent).not.toContain("Section");
  expect(root.querySelector(".kbdbar")?.textContent).toContain("Space");
  expect(root.querySelector(".kbdbar")?.textContent).toContain("start / pause");
  expect(root.querySelector(".kbdbar")?.textContent?.replace(/\s+/g, " ")).toContain("S swap");

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
  expect(play.textContent).toContain("Start");
  expect(play.textContent).toContain("Space");

  play.click();
  now = 65000;
  view.tick();
  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("01:04");
  expect(clock.dataset.peithoState).toBe("running");
  expect(pill.dataset.peithoState).toBe("running");
  expect(pill.textContent).toContain("Running");
  expect(play.textContent).toContain("Pause");
  expect(play.textContent).toContain("Space");
});

it("loads legacy manifests without images at runtime", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: legacyManifestFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  expect(root.querySelector('[data-peitho-presenter="current"] .peitho-slide')).not.toBeNull();
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

it.each([
  ["normal (no planned)", null, 65_000, "normal"],
  ["normal (planned, plenty of runway)", 600_000, 60_000, "normal"],
  ["warning", 600_000, 481_000, "warning"],
  ["urgent", 600_000, 541_000, "urgent"],
  ["overrun", 600_000, 601_000, "overrun"]
] as const)(
  "updates presenter timer urgency to %s on tick",
  async (_label, plannedDurationMs, elapsedMs, expected) => {
    let now = 1_000;
    const root = document.createElement("main");
    const { factory } = mockSyncChannelFactory();
    const view = await mountPresenterView({
      root,
      notes,
      fetcher: standardFetch({ plannedDurationMs }),
      window,
      now: () => now,
      syncChannelFactory: factory
    });
    views.push(view);

    root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
    now = 1_000 + elapsedMs;
    view.tick();

    expect(
      root.querySelector<HTMLElement>('[data-peitho-presenter="clock"]')?.dataset.peithoUrgency
    ).toBe(expected);
  }
);

it("keeps agenda slot empty when manifest has no sections", async () => {
  const root = document.createElement("main");
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ sections: [] }),
    window,
    now: () => 1000,
    syncChannelFactory: mockSyncChannelFactory().factory
  });
  views.push(view);

  expect(root.querySelector('[data-peitho-presenter="agenda-slot"]')?.childElementCount).toBe(0);
  expect(root.querySelector("[data-peitho-agenda]")).toBeNull();
});

it("mounts agenda between tracker and controls when manifest has sections", async () => {
  const root = document.createElement("main");
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({
      plannedDurationMs: 180_000,
      sections: [
        { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
        { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 120_000 }
      ]
    }),
    window,
    now: () => 1000,
    syncChannelFactory: mockSyncChannelFactory().factory
  });
  views.push(view);

  const clockChildren = Array.from(
    root.querySelector('[data-peitho-presenter="clock"]')?.children ?? [],
    (node) => (node as HTMLElement).dataset.peithoPresenter ?? (node as HTMLElement).className
  );
  expect(clockChildren).toEqual(["clock-row", "tracker-slot", "agenda-slot", "controls"]);
  expect(
    root.querySelector('[data-peitho-presenter="agenda-slot"] [data-peitho-agenda]')
  ).not.toBeNull();
});

it("shows the current section name in the status line and follows slide navigation", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({
      plannedDurationMs: 180_000,
      sections: [
        { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
        { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 120_000 }
      ]
    }),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  const sectionEl = root.querySelector<HTMLElement>('[data-peitho-presenter="section"]')!;
  const sepEl = root.querySelector<HTMLElement>('[data-peitho-presenter="section-sep"]')!;
  expect(sectionEl.hidden).toBe(false);
  expect(sepEl.hidden).toBe(false);
  expect(sectionEl.textContent).toBe("Section — “Setup”");
  expect(root.querySelector(".status-line")?.textContent).not.toContain("Now");

  window.dispatchEvent(
    new CustomEvent("peitho:navigate", { detail: { to: { index: 1 } } })
  );
  await Promise.resolve();
  expect(sectionEl.textContent).toBe("Section — “Demo”");
});

it("hides the section chip in the status line when the manifest has no sections", async () => {
  const root = document.createElement("main");
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ sections: [] }),
    window,
    now: () => 1000,
    syncChannelFactory: mockSyncChannelFactory().factory
  });
  views.push(view);

  const sectionEl = root.querySelector<HTMLElement>('[data-peitho-presenter="section"]')!;
  const sepEl = root.querySelector<HTMLElement>('[data-peitho-presenter="section-sep"]')!;
  expect(sectionEl.hidden).toBe(true);
  expect(sepEl.hidden).toBe(true);
  expect(sectionEl.textContent).toBe("");
  expect(root.querySelector(".status-line")?.textContent).not.toContain("Now");
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

  const notesEl = root.querySelector<HTMLElement>('[data-peitho-presenter="notes"]');
  expect(notesEl?.textContent).toContain("No notes for this slide.");
  expect(notesEl?.classList.contains("is-empty")).toBe(true);
  expect(root.querySelector('[data-peitho-presenter="preview-end"]')?.textContent).toContain(
    "End of deck"
  );
});

it("next preview skips skipped slides while counters keep total slide count", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({
      ...manifestWithSlides([
        { key: "intro" },
        { key: "appendix", skip: true },
        { key: "summary" }
      ])
    }),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  expect(root.querySelector('[data-peitho-presenter="position"]')?.textContent).toBe(
    "Slide 01 of 03"
  );
  expect(root.querySelector('[data-peitho-presenter="position-short"]')?.textContent).toBe(
    "01 / 03"
  );
  expect(root.querySelector('[data-peitho-presenter="next-position"]')?.textContent).toBe(
    "03 / 03"
  );
  expect(
    root.querySelector<HTMLElement>(
      '[data-peitho-presenter="preview"] [data-slide-index="2"]'
    )?.shadowRoot?.textContent
  ).toContain("summary");
});

it("next preview shows end when only skipped slides remain", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({
      ...manifestWithSlides([
        { key: "intro" },
        { key: "appendix", skip: true },
        { key: "backup", skip: true }
      ])
    }),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  expect(root.querySelector<HTMLElement>('[data-peitho-presenter="preview"]')?.hidden).toBe(true);
  expect(root.querySelector<HTMLElement>('[data-peitho-presenter="preview-end"]')?.hidden).toBe(
    false
  );
  expect(root.querySelector('[data-peitho-presenter="next-position"]')?.textContent).toBe("End");
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

it("scales presenter previews from a 4 by 3 manifest canvas", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ aspectRatio: "4:3", canvasWidth: 960, canvasHeight: 720 }),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  const currentPane = root.querySelector<HTMLElement>('[data-peitho-presenter="current"]')!;
  const previewPane = root.querySelector<HTMLElement>('[data-peitho-presenter="preview"]')!;
  sizeElement(currentPane, 320, 240);
  sizeElement(previewPane, 240, 180);
  window.dispatchEvent(new Event("resize"));

  const currentSlide = currentPane.querySelector<HTMLElement>(".peitho-slide");
  const previewSlide = previewPane.querySelector<HTMLElement>(".peitho-slide");
  expect(currentSlide?.style.width).toBe("960px");
  expect(currentSlide?.style.height).toBe("720px");
  expect(currentSlide?.style.transform).toBe("translate(0px, 0px) scale(0.3333333333333333)");
  expect(previewSlide?.style.transform).toBe("translate(0px, 0px) scale(0.25)");
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
  const swapRequests: unknown[] = [];
  const onNavigate = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  const onTimerControl = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  const onCloseRequest = (event: Event): void => {
    closeRequests.push((event as CustomEvent).detail);
  };
  const onSwapRequest = (event: Event): void => {
    swapRequests.push((event as CustomEvent).detail);
  };
  window.addEventListener("peitho:navigate", onNavigate);
  window.addEventListener("peitho:timercontrol", onTimerControl);
  window.addEventListener("peitho:closerequest", onCloseRequest);
  window.addEventListener("peitho:swaprequest", onSwapRequest);
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));
  cleanups.push(() => window.removeEventListener("peitho:closerequest", onCloseRequest));
  cleanups.push(() => window.removeEventListener("peitho:swaprequest", onSwapRequest));
  channel.onmessage?.({ data: { synced: true } });

  root.querySelector<HTMLButtonElement>('[data-peitho-action="next"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="reset"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="swap"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="close"]')?.click();

  expect(events).toEqual([
    { to: "next" },
    { action: "start" },
    { action: "pause" },
    { action: "resume" },
    { action: "reset" }
  ]);
  expect(closeRequests).toEqual([null]);
  expect(swapRequests).toEqual([null]);
  expect(channel.sent).toEqual([
    { index: 1 },
    { timer: { running: true, elapsedMs: 0 } },
    { timer: { running: false, elapsedMs: 0 } },
    { timer: { running: true, elapsedMs: 0 } },
    { timer: { running: false, elapsedMs: 0 } },
    { swapped: true },
    { close: true }
  ]);
});

it("posts presenter timer transitions and adopts replayed timer state", async () => {
  let now = 1_000;
  const root = document.createElement("main");
  const { channel, factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);
  const play = root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')!;
  channel.onmessage?.({ data: { synced: true } });

  play.click();
  now = 2_500;
  play.click();
  now = 3_500;
  play.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="reset"]')?.click();

  expect(channel.sent).toEqual([
    { timer: { running: true, elapsedMs: 0 } },
    { timer: { running: false, elapsedMs: 1500 } },
    { timer: { running: true, elapsedMs: 1500 } },
    { timer: { running: false, elapsedMs: 0 } }
  ]);

  now = 10_000;
  channel.onmessage?.({
    data: { timer: { running: true, elapsedMs: 2_000, atMs: 50_000 }, nowMs: 51_000 }
  });
  view.tick();

  expect(view.mainShell.elapsedMs()).toBe(3_000);
  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("00:03");
});

it("maps presenter Space to timer playpause without navigating", async () => {
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
  const timerRequests: unknown[] = [];
  const navigateRequests: unknown[] = [];
  const onTimerControl = (event: Event): void => {
    timerRequests.push((event as CustomEvent).detail);
  };
  const onNavigate = (event: Event): void => {
    navigateRequests.push((event as CustomEvent).detail);
  };
  window.addEventListener("peitho:timercontrol", onTimerControl);
  window.addEventListener("peitho:navigate", onNavigate);
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));

  const startEvent = new KeyboardEvent("keydown", { key: " ", cancelable: true });
  window.dispatchEvent(startEvent);
  const pauseEvent = new KeyboardEvent("keydown", { key: " ", cancelable: true });
  window.dispatchEvent(pauseEvent);
  const resumeEvent = new KeyboardEvent("keydown", { key: " ", cancelable: true });
  window.dispatchEvent(resumeEvent);

  expect(startEvent.defaultPrevented).toBe(true);
  expect(pauseEvent.defaultPrevented).toBe(true);
  expect(resumeEvent.defaultPrevented).toBe(true);
  expect(timerRequests).toEqual([{ action: "start" }, { action: "pause" }, { action: "resume" }]);
  expect(navigateRequests).toEqual([]);
  expect(view.mainShell.currentIndex).toBe(0);
});

it("ignores repeated presenter Space keydown and keeps arrow navigation", async () => {
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
  const timerRequests: unknown[] = [];
  const navigateRequests: unknown[] = [];
  const onTimerControl = (event: Event): void => {
    timerRequests.push((event as CustomEvent).detail);
  };
  const onNavigate = (event: Event): void => {
    navigateRequests.push((event as CustomEvent).detail);
  };
  window.addEventListener("peitho:timercontrol", onTimerControl);
  window.addEventListener("peitho:navigate", onNavigate);
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));

  const repeatEvent = new KeyboardEvent("keydown", { key: " ", repeat: true, cancelable: true });
  window.dispatchEvent(repeatEvent);
  const nextEvent = new KeyboardEvent("keydown", { key: "ArrowRight", cancelable: true });
  window.dispatchEvent(nextEvent);
  const prevEvent = new KeyboardEvent("keydown", { key: "ArrowLeft", cancelable: true });
  window.dispatchEvent(prevEvent);

  expect(repeatEvent.defaultPrevented).toBe(true);
  expect(nextEvent.defaultPrevented).toBe(true);
  expect(prevEvent.defaultPrevented).toBe(true);
  expect(timerRequests).toEqual([]);
  expect(navigateRequests).toEqual([{ to: "next" }, { to: "prev" }]);
  expect(view.mainShell.currentIndex).toBe(0);
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
  expect(play.textContent).toContain("Start");

  play.click();
  expect(clock.dataset.peithoState).toBe("running");
  expect(pill.textContent).toContain("Running");
  expect(play.textContent).toContain("Pause");

  play.click();
  expect(clock.dataset.peithoState).toBe("paused");
  expect(pill.textContent).toContain("Paused");
  expect(play.textContent).toContain("Resume");

  play.click();
  expect(clock.dataset.peithoState).toBe("running");
  expect(play.textContent).toContain("Pause");

  reset.click();
  expect(clock.dataset.peithoState).toBe("stopped");
  expect(pill.textContent).toContain("Stopped");
  expect(play.textContent).toContain("Start");
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
