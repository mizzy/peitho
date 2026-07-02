import { afterEach, expect, it, vi } from "vitest";
import { installSyncBridge, mountPresentShell } from "../src/index";
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
