import { afterEach, expect, it, vi } from "vitest";
import { installSyncBridge, mountPresentShell, serverSyncChannelFactory } from "../src/index";
import type { PresentShell } from "../src/index";
import type { SyncChannel } from "../src/sync";

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
    { index: 1, key: "arch-1", src: "slides/001-arch-1.html", hasNotes: false }
  ]
};

function standardFetch(): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-arch-1.html") return okText("<section><h1>Arch</h1></section>");
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  }) as typeof fetch;
}

function mockChannel() {
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
  return channel;
}

it("server sync channel posts local messages to /sync", async () => {
  const fetcher = vi.fn((url: string, init?: RequestInit) => {
    if (init?.method === "POST") return Promise.resolve({ ok: true, status: 204 } as Response);
    return new Promise<Response>(() => undefined);
  }) as typeof fetch;
  const factory = serverSyncChannelFactory({
    fetcher
  });
  const channel = factory("peitho-sync");

  channel.postMessage({ index: 2 });
  await Promise.resolve();

  expect(fetcher).toHaveBeenCalledWith("/sync", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ index: 2 })
  });
  channel.close();
});

it("server sync channel polls and forwards long-poll messages", async () => {
  const fetcher = vi.fn((url: string, init?: RequestInit) => {
    if (init?.method === "POST") return Promise.resolve({ ok: true, status: 204 } as Response);
    if (url === "/sync?seq=0") {
      return Promise.resolve(okJson({ seq: 1, message: { index: 1 } }));
    }
    return new Promise<Response>(() => undefined);
  }) as typeof fetch;
  const factory = serverSyncChannelFactory({
    fetcher
  });
  const channel = factory("peitho-sync");
  const received: unknown[] = [];
  channel.onmessage = (event) => received.push(event.data);

  await vi.waitFor(() => expect(received).toEqual([{ index: 1 }]));

  expect(fetcher).toHaveBeenCalledWith("/sync?seq=0", expect.objectContaining({ signal: expect.any(AbortSignal) }));
  expect(fetcher).toHaveBeenCalledWith("/sync?seq=1", expect.objectContaining({ signal: expect.any(AbortSignal) }));
  channel.close();
});

it("server sync channel aborts the active poll on close", () => {
  const captured: { signal?: AbortSignal } = {};
  const fetcher = vi.fn((_url: string, init?: RequestInit) => {
    captured.signal = init?.signal as AbortSignal;
    return new Promise<Response>(() => undefined);
  }) as typeof fetch;
  const channel = serverSyncChannelFactory({
    fetcher
  })("peitho-sync");

  channel.close();

  if (!captured.signal) throw new Error("poll signal was not captured");
  expect(captured.signal.aborted).toBe(true);
});

const shells: PresentShell[] = [];
const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  while (shells.length > 0) shells.pop()?.destroy();
});

it("posts local slidechange index to peitho-sync", async () => {
  const channel = mockChannel();
  const root = document.createElement("main");
  const shell = await mountPresentShell({ root, fetcher: standardFetch(), window });
  shells.push(shell);
  cleanups.push(installSyncBridge(window, () => channel));

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(channel.sent).toEqual([{ index: 1 }]);
});

it("turns remote index messages into navigate requests", () => {
  const channel = mockChannel();
  const requests: unknown[] = [];
  const onNavigate = (event: Event) => requests.push((event as CustomEvent).detail);
  window.addEventListener("peitho:navigate", onNavigate);
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));
  cleanups.push(installSyncBridge(window, () => channel));

  channel.onmessage?.({ data: { index: 1 } });

  expect(requests).toEqual([{ to: { index: 1 } }]);
});

it("dispatches remote sync navigation to the injected bus", () => {
  const channel = mockChannel();
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) =>
    requests.push((event as CustomEvent).detail)
  );

  const cleanup = installSyncBridge(window, () => channel, bus);
  cleanups.push(cleanup);
  channel.onmessage?.({ data: { index: 1 } });

  expect(requests).toEqual([{ to: { index: 1 } }]);
});

it("does not echo forever when remote index equals current slide", async () => {
  const channel = mockChannel();
  const root = document.createElement("main");
  const shell = await mountPresentShell({ root, fetcher: standardFetch(), window });
  shells.push(shell);
  cleanups.push(installSyncBridge(window, () => channel));

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(channel.sent).toEqual([{ index: 1 }]);
  channel.onmessage?.({ data: { index: 1 } });

  expect(shell.currentIndex).toBe(1);
  expect(channel.sent).toEqual([{ index: 1 }]);
});
