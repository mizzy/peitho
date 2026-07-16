import { afterEach, expect, it, vi } from "vitest";
import type { Manifest } from "../../../bindings/Manifest";
import type { Notes } from "../../../bindings/Notes";
import {
  expectedElapsedAtSlide,
  installRemoteControls,
  mountRemoteView,
  plannedProgressAtElapsed,
  remotePaceState,
  type RemoteView
} from "../src/remote";
import type { PresentShell, ShellOptions } from "../src/shell";
import type { SyncChannel } from "../src/sync";

type MockChannel = SyncChannel & {
  sent: unknown[];
  closed: boolean;
  deliver(message: unknown): void;
};

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function fail(status: number): Response {
  return { ok: false, status, json: async () => ({}) } as Response;
}

function manifestWithSlides(
  slides: Array<{ key: string; skip?: boolean; title?: string }>,
  overrides: Partial<Manifest> = {}
): Manifest {
  return {
    version: 1,
    peithoVersion: "0.1.0",
    title: "Remote Demo",
    slideCount: slides.length,
    plannedDurationMs: null,
    aspectRatio: "16:9",
    canvasWidth: 1280,
    canvasHeight: 720,
    sections: [],
    slides: slides.map((slide, index) => ({
      index,
      key: slide.key,
      src: `slides/${String(index).padStart(3, "0")}-${slide.key}.html`,
      hasNotes: false,
      skip: slide.skip ?? false,
      text: { title: slide.title ?? "", body: "", code: "" }
    })),
    images: [],
    ...overrides
  };
}

function fetchManifest(manifest: Manifest, notes: Notes = { version: 1, notes: {} }): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "notes.json") return okJson(notes);
    return fail(404);
  }) as typeof fetch;
}

function mockChannel(): MockChannel {
  const channel: MockChannel = {
    sent: [],
    closed: false,
    onmessage: null,
    postMessage(message: unknown) {
      this.sent.push(message);
    },
    close() {
      this.closed = true;
    },
    deliver(message: unknown) {
      this.onmessage?.({ data: message });
    }
  };
  return channel;
}

const cleanups: Array<() => void> = [];
const views: RemoteView[] = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  while (views.length > 0) views.pop()?.destroy();
  document.body.replaceChildren();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function button(root: HTMLElement, action: "prev" | "next"): HTMLButtonElement {
  const el = root.querySelector<HTMLButtonElement>(`[data-peitho-action="${action}"]`);
  if (el == null) throw new Error(`missing ${action} button`);
  return el;
}

async function mountRemoteForTest(
  manifest: Manifest,
  channel = mockChannel(),
  options: {
    notes?: Notes;
    mountPresentShell?: (options: ShellOptions) => Promise<PresentShell>;
    autoSync?: boolean;
  } = {}
): Promise<{ root: HTMLElement; channel: MockChannel; view: RemoteView }> {
  const root = document.createElement("main");
  document.body.appendChild(root);
  const view = await mountRemoteView({
    root,
    manifestUrl: "manifest.json",
    notesUrl: "notes.json",
    fetcher: fetchManifest(manifest, options.notes),
    channelFactory: () => channel,
    syncChannelFactory: () => channel,
    mountPresentShell: options.mountPresentShell ?? mockMountPresentShell(),
    window,
    document,
    bus: window
  });
  views.push(view);
  if (options.autoSync !== false) {
    channel.deliver({ synced: true });
  }
  return { root, channel, view };
}

function mockMountPresentShell(navigations: unknown[] = []): (options: ShellOptions) => Promise<PresentShell> {
  return vi.fn(async (options: ShellOptions) => {
    const marker = document.createElement("div");
    marker.dataset.previewShell = "mounted";
    options.root.append(marker);
    options.bus?.addEventListener("peitho:navigate", (event) => {
      navigations.push((event as CustomEvent).detail);
    });
    return {
      manifest: null,
      currentIndex: 0,
      navigate: vi.fn(),
      elapsedMs: () => 0,
      isPaused: () => false,
      startedAt: () => null,
      adoptTimerState: vi.fn(),
      destroy: vi.fn()
    };
  });
}

it("remote buttons dispatch navigate request events only", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  cleanups.push(installRemoteControls({ root, document, bus }));

  button(root, "prev").disabled = false;
  button(root, "next").disabled = false;
  button(root, "prev").click();
  button(root, "next").click();

  expect(requests).toEqual([{ to: "prev" }, { to: "next" }]);
});

it("remote controls mount disabled before the synced event arrives", () => {
  const root = document.createElement("main");
  cleanups.push(installRemoteControls({ root, document, bus: new EventTarget() }));

  expect(button(root, "prev").disabled).toBe(true);
  expect(button(root, "next").disabled).toBe(true);
  expect(root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]')?.disabled).toBe(
    true
  );
});

it("remote controller resolves next and prev across skipped slides and posts absolute indexes", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro" }, { key: "skip", skip: true }, { key: "end" }])
  );

  button(root, "next").click();
  channel.deliver({ index: 2 });
  button(root, "prev").click();

  expect(channel.sent).toEqual([{ index: 2 }, { index: 0 }]);
});

it("remote controls are disabled and silent before the initial sync snapshot is applied", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro" }, { key: "end" }]),
    mockChannel(),
    { autoSync: false }
  );

  expect(button(root, "prev").disabled).toBe(true);
  expect(button(root, "next").disabled).toBe(true);
  expect(root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]')?.disabled).toBe(
    true
  );

  button(root, "next").click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]')?.click();
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  window.dispatchEvent(
    new CustomEvent("peitho:timercontrol", { detail: { action: "start" } })
  );

  expect(channel.sent).toEqual([]);
});

it("remote controls enable after synced replay is reported", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro" }, { key: "end" }]),
    mockChannel(),
    { autoSync: false }
  );

  channel.deliver({ synced: true });

  expect(button(root, "prev").disabled).toBe(true);
  expect(button(root, "next").disabled).toBe(false);
  expect(root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]')?.disabled).toBe(
    false
  );
  button(root, "next").click();
  expect(channel.sent).toEqual([{ index: 1 }]);
});

it("remote controller advances optimistically across rapid taps", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro" }, { key: "middle" }, { key: "end" }])
  );

  button(root, "next").click();
  button(root, "next").click();

  expect(channel.sent).toEqual([{ index: 1 }, { index: 2 }]);
});

it("remote controller treats a null replay index as the first non-skipped slide", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "skip", skip: true }, { key: "intro" }, { key: "end" }])
  );

  expect(root.querySelector('[data-peitho-remote="counter"]')?.textContent).toBe("2 / 3");
  button(root, "next").click();

  expect(channel.sent).toEqual([{ index: 2 }]);
});

it("remote renders preview title progress section notes and planned pace chrome", async () => {
  const previewNavigations: unknown[] = [];
  const manifest = manifestWithSlides(
    [
      { key: "intro", title: "Opening" },
      { key: "arch", title: "Architecture" },
      { key: "end", title: "Summary" }
    ],
    {
      plannedDurationMs: 90_000,
      sections: [
        { name: "Intro", startIndex: 0, endIndex: 0, plannedDurationMs: 30_000 },
        { name: "Architecture", startIndex: 1, endIndex: 2, plannedDurationMs: 60_000 }
      ]
    }
  );
  const { root, channel } = await mountRemoteForTest(manifest, mockChannel(), {
    notes: { version: 1, notes: { arch: "Use the typed shell.\nNo shortcuts." } },
    mountPresentShell: mockMountPresentShell(previewNavigations)
  });

  channel.deliver({ index: 1 });
  channel.deliver({
    timer: { running: true, elapsedMs: 20_000, atMs: 1_000_000 },
    nowMs: 1_005_000
  });

  expect(root.querySelector('[data-preview-shell="mounted"]')).not.toBeNull();
  expect(previewNavigations.at(-1)).toEqual({ to: { index: 1 } });
  expect(root.querySelector('[data-peitho-remote="title"]')?.textContent).toBe("Architecture");
  expect(root.querySelector('[data-peitho-remote="counter"]')?.textContent).toBe("2 / 3");
  expect(root.querySelector<HTMLElement>('[data-peitho-remote="progress-fill"]')?.style.width).toBe(
    "50%"
  );
  expect(root.querySelector('[data-peitho-remote="plan-tick"]')).not.toBeNull();
  expect(root.querySelector('[data-peitho-remote="section"]')?.textContent).toBe(
    "Architecture · slide 1 / 2 in section"
  );
  expect(root.querySelector('[data-peitho-remote="notes"]')?.textContent).toBe(
    "Use the typed shell.\nNo shortcuts."
  );
  expect(root.querySelector('[data-peitho-remote="elapsed"]')?.textContent).toBe("0:25");
  expect(root.querySelector('[data-peitho-remote="planned"]')?.textContent).toBe("1:30");
  expect(root.querySelector('[data-peitho-remote="pace-chip"]')?.textContent).toContain("ahead");
});

it("remote mounts with empty notes placeholders when notes fetch fails", async () => {
  const manifest = manifestWithSlides([
    { key: "intro", title: "Intro" },
    { key: "details", title: "Details" }
  ]);
  const channel = mockChannel();
  const error = vi.fn();
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "notes.json") throw new Error("offline");
    return fail(404);
  }) as typeof fetch;
  const root = document.createElement("main");
  document.body.appendChild(root);
  const view = await mountRemoteView({
    root,
    manifestUrl: "manifest.json",
    notesUrl: "notes.json",
    fetcher,
    channelFactory: () => channel,
    syncChannelFactory: () => channel,
    mountPresentShell: mockMountPresentShell(),
    window,
    document,
    bus: window,
    console: { error }
  });
  views.push(view);
  channel.deliver({ synced: true });

  expect(root.querySelector(".peitho-remote")).not.toBeNull();
  expect(root.querySelector('[data-preview-shell="mounted"]')).not.toBeNull();
  expect(root.querySelector('[data-peitho-remote="notes"]')?.textContent).toBe(
    "No notes for this slide"
  );
  expect(root.querySelector<HTMLElement>('[data-peitho-remote="notes"]')?.dataset.peithoEmpty).toBe(
    "true"
  );

  channel.deliver({ index: 1 });

  expect(root.querySelector('[data-peitho-remote="title"]')?.textContent).toBe("Details");
  expect(root.querySelector('[data-peitho-remote="notes"]')?.textContent).toBe(
    "No notes for this slide"
  );
  expect(error).toHaveBeenCalledTimes(1);
  expect(error).toHaveBeenCalledWith("Failed to load notes.json: offline");
});

it("remote omits planned and section rows when the deck has no time or sections", async () => {
  const { root } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro", title: "Intro" }, { key: "end", title: "End" }])
  );

  expect(root.querySelector<HTMLElement>('[data-peitho-remote="planned"]')?.hidden).toBe(true);
  expect(root.querySelector<HTMLElement>('[data-peitho-remote="pace-chip"]')?.hidden).toBe(true);
  expect(root.querySelector<HTMLElement>('[data-peitho-remote="plan-tick"]')?.hidden).toBe(true);
  expect(root.querySelector('[data-peitho-remote="section"]')).toBeNull();
  expect(root.querySelector('[data-peitho-remote="elapsed"]')?.textContent).toBe("0:00");
  expect(root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]')).not.toBeNull();
});

it("remote disables previous and next based on first and last non-skipped slides", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([
      { key: "intro" },
      { key: "skip", skip: true },
      { key: "middle" },
      { key: "backup", skip: true }
    ])
  );

  expect(button(root, "prev").disabled).toBe(true);
  expect(button(root, "next").disabled).toBe(false);

  channel.deliver({ index: 2 });
  expect(button(root, "prev").disabled).toBe(false);
  expect(button(root, "next").disabled).toBe(true);
});

it("remote timer button emits requests while the bridge posts absolute timer states", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000);
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides(
      [{ key: "intro", title: "Intro" }, { key: "end", title: "End" }],
      { plannedDurationMs: 60_000 }
    )
  );
  const requests: unknown[] = [];
  const onTimerControl = (event: Event): void => {
    requests.push((event as CustomEvent).detail);
  };
  window.addEventListener("peitho:timercontrol", onTimerControl);
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));
  const timer = root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]')!;

  timer.click();
  vi.setSystemTime(3_500);
  timer.click();
  expect(root.querySelector('[data-peitho-remote="pace-chip"]')?.textContent).toBe("Paused");
  timer.click();

  expect(requests).toEqual([{ action: "start" }, { action: "pause" }, { action: "resume" }]);
  expect(channel.sent).toEqual([
    { timer: { running: true, elapsedMs: 0 } },
    { timer: { running: false, elapsedMs: 2500 } },
    { timer: { running: true, elapsedMs: 2500 } }
  ]);
});

it("remote timer states render stopped running and paused rows", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000);
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides(
      [{ key: "intro", title: "Intro" }, { key: "end", title: "End" }],
      { plannedDurationMs: 60_000 }
    )
  );
  const timer = root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]')!;

  expect(timer.querySelector('[data-peitho-icon="play"]')).not.toBeNull();
  expect(root.querySelector<HTMLElement>('[data-peitho-remote="pace-chip"]')?.hidden).toBe(true);

  channel.deliver({
    timer: { running: true, elapsedMs: 35_000, atMs: 10_000 },
    nowMs: 10_000
  });
  expect(timer.querySelector('[data-peitho-icon="pause"]')).not.toBeNull();
  expect(root.querySelector('[data-peitho-remote="pace-chip"]')?.textContent).toContain("behind");

  channel.deliver({
    timer: { running: false, elapsedMs: 20_000, atMs: 10_000 },
    nowMs: 10_000
  });
  expect(timer.querySelector('[data-peitho-icon="play"]')).not.toBeNull();
  expect(root.querySelector('[data-peitho-remote="pace-chip"]')?.textContent).toBe("Paused");
});

it("remote preview shell reuses the already loaded manifest", async () => {
  const manifest = manifestWithSlides([{ key: "intro", title: "Intro" }, { key: "end" }]);
  let manifestFetches = 0;
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") {
      manifestFetches += 1;
      if (manifestFetches > 1) return fail(500);
      return okJson(manifest);
    }
    if (url === "notes.json") return okJson({ version: 1, notes: {} });
    if (url === "peitho.css") return { ok: true, status: 200, text: async () => "" } as Response;
    const slide = manifest.slides.find((item) => item.src === url);
    if (slide) return { ok: true, status: 200, text: async () => "<section></section>" } as Response;
    return fail(404);
  }) as typeof fetch;
  const root = document.createElement("main");
  document.body.appendChild(root);
  const channel = mockChannel();
  const view = await mountRemoteView({
    root,
    manifestUrl: "manifest.json",
    notesUrl: "notes.json",
    fetcher,
    channelFactory: () => channel,
    syncChannelFactory: () => channel,
    window,
    document,
    bus: window
  });
  views.push(view);

  expect(root.className).not.toContain("peitho-remote-error");
  expect(manifestFetches).toBe(1);
  expect(root.querySelector('[data-peitho-remote="preview"] .peitho-slide')).not.toBeNull();
});

it("remote timer interval updates time chrome without replacing the notes text node", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000);
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides(
      [{ key: "intro", title: "Intro" }, { key: "end" }],
      { plannedDurationMs: 60_000 }
    ),
    mockChannel(),
    { notes: { version: 1, notes: { intro: "Long note" } } }
  );
  const notes = root.querySelector<HTMLElement>('[data-peitho-remote="notes"]')!;
  const firstTextNode = notes.firstChild;
  channel.deliver({
    timer: { running: true, elapsedMs: 1_000, atMs: 10_000 },
    nowMs: 10_000
  });

  vi.advanceTimersByTime(1000);

  expect(notes.firstChild).toBe(firstTextNode);
  expect(notes.textContent).toBe("Long note");
  expect(root.querySelector('[data-peitho-remote="elapsed"]')?.textContent).toBe("0:02");
});

it("remote controller no-ops at the ends", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro" }, { key: "middle" }, { key: "end" }])
  );

  channel.deliver({ index: 2 });
  button(root, "next").click();
  channel.deliver({ index: 0 });
  button(root, "prev").click();

  expect(channel.sent).toEqual([]);
});

it("remote controller replays and clamps out-of-range indexes into the counter", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro" }, { key: "middle" }, { key: "end" }])
  );

  channel.deliver({ index: 99 });
  expect(root.querySelector('[data-peitho-remote="counter"]')?.textContent).toBe("3 / 3");

  channel.deliver({ index: -10 });
  expect(root.querySelector('[data-peitho-remote="counter"]')?.textContent).toBe("1 / 3");
});

it("remote controller disables buttons and shows ended state on close", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro" }, { key: "end" }])
  );

  channel.deliver({ close: true });

  expect(button(root, "prev").disabled).toBe(true);
  expect(button(root, "next").disabled).toBe(true);
  expect(root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]')?.disabled).toBe(
    true
  );
  expect(root.querySelector('[data-peitho-remote="status"]')?.textContent).toBe("Ended");
  expect(root.querySelector<HTMLElement>(".peitho-remote")?.dataset.peithoEnded).toBe("true");
});

it("remote controller closes the sync channel and ignores clicks after ended", async () => {
  const { root, channel } = await mountRemoteForTest(
    manifestWithSlides([{ key: "intro" }, { key: "end" }])
  );

  channel.deliver({ close: true });
  button(root, "next").click();
  button(root, "prev").click();

  expect(channel.closed).toBe(true);
  expect(channel.sent).toEqual([]);
});

it("pace math uses section piecewise timing", () => {
  const manifest = manifestWithSlides(
    [{ key: "a" }, { key: "b" }, { key: "c" }, { key: "d" }],
    {
      plannedDurationMs: 60_000,
      sections: [
        { name: "One", startIndex: 0, endIndex: 1, plannedDurationMs: 20_000 },
        { name: "Two", startIndex: 2, endIndex: 3, plannedDurationMs: 40_000 }
      ]
    }
  );

  expect(expectedElapsedAtSlide(manifest, 0)).toBe(0);
  expect(expectedElapsedAtSlide(manifest, 1)).toBe(10_000);
  expect(expectedElapsedAtSlide(manifest, 2)).toBe(20_000);
  expect(expectedElapsedAtSlide(manifest, 3)).toBe(40_000);
});

it("pace math falls back to whole-deck timing without sections", () => {
  const manifest = manifestWithSlides([{ key: "a" }, { key: "b" }, { key: "c" }], {
    plannedDurationMs: 60_000
  });

  expect(expectedElapsedAtSlide(manifest, 0)).toBe(0);
  expect(expectedElapsedAtSlide(manifest, 1)).toBe(20_000);
  expect(expectedElapsedAtSlide(manifest, 2)).toBe(40_000);
});

it("pace math clamps plan tick progress", () => {
  const manifest = manifestWithSlides([{ key: "a" }, { key: "b" }, { key: "c" }], {
    plannedDurationMs: 60_000
  });

  expect(plannedProgressAtElapsed(manifest, -10_000)).toBe(0);
  expect(plannedProgressAtElapsed(manifest, 0)).toBe(0);
  expect(plannedProgressAtElapsed(manifest, 30_000)).toBe(0.75);
  expect(plannedProgressAtElapsed(manifest, 90_000)).toBe(1);
});

it("pace chip selection covers ahead behind paused and null planned time", () => {
  const manifest = manifestWithSlides([{ key: "a" }, { key: "b" }, { key: "c" }], {
    plannedDurationMs: 60_000
  });
  const unplanned = manifestWithSlides([{ key: "a" }, { key: "b" }, { key: "c" }]);

  expect(remotePaceState(manifest, 1, 25_000, true)).toEqual({
    kind: "behind",
    label: "0:05 behind",
    emoji: "tortoise"
  });
  expect(remotePaceState(manifest, 1, 15_000, true)).toEqual({
    kind: "ahead",
    label: "0:05 ahead",
    emoji: "hare"
  });
  expect(remotePaceState(manifest, 1, 15_000, false)).toEqual({
    kind: "paused",
    label: "Paused",
    emoji: null
  });
  expect(remotePaceState(unplanned, 1, 15_000, true)).toBeNull();
});

it("remote manifest fetch failure is visible", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);

  const view = await mountRemoteView({
    root,
    manifestUrl: "manifest.json",
    fetcher: vi.fn(async () => fail(500)) as typeof fetch,
    channelFactory: () => mockChannel(),
    window,
    document,
    bus: window
  });
  views.push(view);

  expect(root.textContent).toContain("Failed to load manifest.json: 500");
  expect(root.className).toContain("peitho-remote-error");
});
