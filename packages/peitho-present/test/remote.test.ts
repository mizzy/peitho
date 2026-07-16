import { afterEach, expect, it, vi } from "vitest";
import type { Manifest } from "../../../bindings/Manifest";
import { installRemoteControls, mountRemoteView, type RemoteView } from "../src/remote";
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

function manifestWithSlides(slides: Array<{ key: string; skip?: boolean }>): Manifest {
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
      text: { title: "", body: "", code: "" }
    })),
    images: []
  };
}

function fetchManifest(manifest: Manifest): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
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
  vi.restoreAllMocks();
});

function button(root: HTMLElement, action: "prev" | "next"): HTMLButtonElement {
  const el = root.querySelector<HTMLButtonElement>(`[data-peitho-action="${action}"]`);
  if (el == null) throw new Error(`missing ${action} button`);
  return el;
}

async function mountRemoteForTest(
  manifest: Manifest,
  channel = mockChannel()
): Promise<{ root: HTMLElement; channel: MockChannel; view: RemoteView }> {
  const root = document.createElement("main");
  document.body.appendChild(root);
  const view = await mountRemoteView({
    root,
    manifestUrl: "manifest.json",
    fetcher: fetchManifest(manifest),
    channelFactory: () => channel,
    window,
    document,
    bus: window
  });
  views.push(view);
  return { root, channel, view };
}

it("remote buttons dispatch navigate request events only", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  cleanups.push(installRemoteControls({ root, document, bus }));

  button(root, "prev").click();
  button(root, "next").click();

  expect(requests).toEqual([{ to: "prev" }, { to: "next" }]);
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
  expect(root.querySelector('[data-peitho-remote="status"]')?.textContent).toBe("Ended");
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
