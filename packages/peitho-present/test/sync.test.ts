import { afterEach, expect, it, vi } from "vitest";
import {
  installCloseOnEscape,
  installSyncBridge,
  mountPresentShell,
  serverSyncChannelFactory
} from "../src/index";
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
      text: { title: "", body: "", code: "" }
    },
    {
      index: 1,
      key: "arch-1",
      src: "slides/001-arch-1.html",
      hasNotes: false,
      text: { title: "", body: "", code: "" }
    }
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
    if (url === "/sync") return Promise.resolve(okJson({ seq: 0, message: null }));
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
    body: JSON.stringify({ index: 2 }),
    keepalive: true
  });
  channel.close();
});

it("server sync channel polls and forwards long-poll messages", async () => {
  const fetcher = vi.fn((url: string, init?: RequestInit) => {
    if (init?.method === "POST") return Promise.resolve({ ok: true, status: 204 } as Response);
    if (url === "/sync") return Promise.resolve(okJson({ seq: 5, message: null }));
    if (url === "/sync?seq=5") {
      return Promise.resolve(okJson({ seq: 6, message: { index: 1 } }));
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

  expect(fetcher).toHaveBeenCalledWith("/sync");
  expect(fetcher).toHaveBeenCalledWith("/sync?seq=5", expect.objectContaining({ signal: expect.any(AbortSignal) }));
  expect(fetcher).toHaveBeenCalledWith("/sync?seq=6", expect.objectContaining({ signal: expect.any(AbortSignal) }));
  channel.close();
});

it("server sync channel replays poll response state after the poll message", async () => {
  const fetcher = vi.fn((url: string, init?: RequestInit) => {
    if (init?.method === "POST") return Promise.resolve({ ok: true, status: 204 } as Response);
    if (url === "/sync") return Promise.resolve(okJson({ seq: 5, message: null }));
    if (url === "/sync?seq=5") {
      return Promise.resolve(okJson({ seq: 6, message: { index: 1 }, index: 2, swapped: true }));
    }
    return new Promise<Response>(() => undefined);
  }) as typeof fetch;
  const channel = serverSyncChannelFactory({ fetcher })("peitho-sync");
  const received: unknown[] = [];
  channel.onmessage = (event) => received.push(event.data);

  await vi.waitFor(() =>
    expect(received).toEqual([{ index: 1 }, { index: 2 }, { swapped: true }])
  );

  channel.close();
});

it("server sync channel does not replay backlog close messages after handshake", async () => {
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") return Promise.resolve(okJson({ seq: 9, message: null }));
    if (url === "/sync?seq=9") return new Promise<Response>(() => undefined);
    throw new Error(`unexpected poll url: ${url}`);
  }) as typeof fetch;
  const channel = serverSyncChannelFactory({ fetcher })("peitho-sync");
  const received: unknown[] = [];
  channel.onmessage = (event) => received.push(event.data);

  await vi.waitFor(() =>
    expect(fetcher).toHaveBeenCalledWith("/sync?seq=9", expect.objectContaining({ signal: expect.any(AbortSignal) }))
  );

  expect(received).toEqual([]);
  expect(fetcher).not.toHaveBeenCalledWith("/sync?seq=0", expect.anything());
  channel.close();
});

it("server sync channel replays handshake index then swapped through onmessage", async () => {
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") {
      return Promise.resolve(okJson({ seq: 4, message: null, index: 2, swapped: true }));
    }
    if (url === "/sync?seq=4") return new Promise<Response>(() => undefined);
    throw new Error(`unexpected sync url: ${url}`);
  }) as typeof fetch;
  const channel = serverSyncChannelFactory({ fetcher })("peitho-sync");
  const received: unknown[] = [];
  channel.onmessage = (event) => received.push(event.data);

  await vi.waitFor(() => expect(received).toEqual([{ index: 2 }, { swapped: true }]));

  expect(fetcher).toHaveBeenCalledWith("/sync");
  expect(fetcher).toHaveBeenCalledWith(
    "/sync?seq=4",
    expect.objectContaining({ signal: expect.any(AbortSignal) })
  );
  channel.close();
});

it("server sync channel forwards generation replay values", async () => {
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") {
      return Promise.resolve(okJson({ seq: 4, message: null, generation: 8 }));
    }
    if (url === "/sync?seq=4") {
      return Promise.resolve(okJson({ seq: 5, message: null, generation: 9 }));
    }
    if (url === "/sync?seq=5") return new Promise<Response>(() => undefined);
    throw new Error(`unexpected sync url: ${url}`);
  }) as typeof fetch;
  const channel = serverSyncChannelFactory({ fetcher })("peitho-sync");
  const received: unknown[] = [];
  channel.onmessage = (event) => received.push(event.data);

  await vi.waitFor(() => expect(received).toEqual([{ generation: 8 }, { generation: 9 }]));

  channel.close();
});

it("server sync channel re-handshakes after a poll network error", async () => {
  let handshakes = 0;
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") {
      handshakes += 1;
      return Promise.resolve(
        okJson(
          handshakes === 1
            ? { seq: 9, message: null, generation: 1 }
            : { seq: 0, message: null, generation: 2 }
        )
      );
    }
    if (url === "/sync?seq=9") {
      return Promise.reject(new Error("connection reset"));
    }
    if (url === "/sync?seq=0") return new Promise<Response>(() => undefined);
    throw new Error(`unexpected sync url: ${url}`);
  }) as typeof fetch;
  const channel = serverSyncChannelFactory({
    fetcher,
    retryMs: 0,
    setTimeoutFn: ((callback: () => void) => window.setTimeout(callback, 0)) as Window["setTimeout"]
  })("peitho-sync");
  const received: unknown[] = [];
  channel.onmessage = (event) => received.push(event.data);

  await vi.waitFor(() => expect(received).toContainEqual({ generation: 2 }));

  expect(fetcher).toHaveBeenCalledWith("/sync");
  expect(fetcher).toHaveBeenCalledWith("/sync?seq=9", expect.objectContaining({ signal: expect.any(AbortSignal) }));
  expect(fetcher).toHaveBeenCalledWith("/sync?seq=0", expect.objectContaining({ signal: expect.any(AbortSignal) }));
  channel.close();
});

it("server sync channel aborts the active poll on close", async () => {
  const captured: { signal?: AbortSignal } = {};
  const fetcher = vi.fn((url: string, init?: RequestInit) => {
    if (url === "/sync") return Promise.resolve(okJson({ seq: 0, message: null }));
    captured.signal = init?.signal as AbortSignal;
    return new Promise<Response>(() => undefined);
  }) as typeof fetch;
  const channel = serverSyncChannelFactory({
    fetcher
  })("peitho-sync");

  await vi.waitFor(() => expect(captured.signal).toBeInstanceOf(AbortSignal));

  channel.close();
  if (!captured.signal) throw new Error("poll signal was not captured");
  expect(captured.signal.aborted).toBe(true);
});

const shells: PresentShell[] = [];
const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  while (shells.length > 0) shells.pop()?.destroy();
  vi.restoreAllMocks();
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

it("dispatches closerequest from Escape", () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:closerequest", (event) => requests.push((event as CustomEvent).detail));
  const cleanup = installCloseOnEscape(window, bus);
  cleanups.push(cleanup);

  const chordEscape = new KeyboardEvent("keydown", {
    key: "Escape",
    metaKey: true,
    cancelable: true
  });
  const bareEscape = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
  window.dispatchEvent(chordEscape);
  window.dispatchEvent(bareEscape);

  expect(chordEscape.defaultPrevented).toBe(false);
  expect(bareEscape.defaultPrevented).toBe(true);
  expect(requests).toEqual([null]);
});

it("posts close sync messages from closerequest", () => {
  const channel = mockChannel();
  const bus = new EventTarget();
  const cleanup = installSyncBridge(window, () => channel, bus);
  cleanups.push(cleanup);

  bus.dispatchEvent(new CustomEvent("peitho:closerequest"));

  expect(channel.sent).toEqual([{ close: true }]);
});

it("posts absolute swap state from swaprequest based on the current route", () => {
  for (const [path, expected] of [
    ["/present.html", { swapped: true }],
    ["/present-swapped", { swapped: false }]
  ] as const) {
    const channel = mockChannel();
    const bus = new EventTarget();
    const cleanup = installSyncBridge(
      window,
      () => channel,
      bus,
      () => undefined,
      () => path,
      () => undefined
    );

    bus.dispatchEvent(new CustomEvent("peitho:swaprequest"));

    expect(channel.sent).toEqual([expected]);
    cleanup();
  }
});

it("does not post swap state on unknown routes", () => {
  const channel = mockChannel();
  const bus = new EventTarget();
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const cleanup = installSyncBridge(
    window,
    () => channel,
    bus,
    () => undefined,
    () => "/unknown",
    () => undefined
  );
  cleanups.push(cleanup);

  bus.dispatchEvent(new CustomEvent("peitho:swaprequest"));

  expect(channel.sent).toEqual([]);
  expect(error).toHaveBeenCalledWith("peitho: swap unavailable on this route");
});

it("closes the window when a remote close sync message arrives", () => {
  const channel = mockChannel();
  const closeWindow = vi.fn();
  const cleanup = installSyncBridge(window, () => channel, window, closeWindow);
  cleanups.push(cleanup);

  channel.onmessage?.({ data: { close: true } });

  expect(closeWindow).toHaveBeenCalledTimes(1);
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

it("ignores generation sync messages on the present sync bridge", () => {
  const channel = mockChannel();
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const cleanup = installSyncBridge(window, () => channel);
  cleanups.push(cleanup);

  channel.onmessage?.({ data: { generation: 3 } });

  expect(error).not.toHaveBeenCalled();
});

it("navigates to the same-window counterpart when remote swapped state differs", () => {
  const cases: Array<{ path: string; swapped: boolean; counterpart: string }> = [
    { path: "/present.html", swapped: true, counterpart: "presenter-swapped" },
    { path: "/", swapped: true, counterpart: "presenter-swapped" },
    { path: "/presenter", swapped: true, counterpart: "present-swapped" },
    { path: "/presenter.html", swapped: true, counterpart: "present-swapped" },
    { path: "/present-swapped", swapped: false, counterpart: "presenter" },
    { path: "/presenter-swapped", swapped: false, counterpart: "present.html" }
  ];

  for (const testCase of cases) {
    const channel = mockChannel();
    const bus = new EventTarget();
    const navigate = vi.fn();
    const cleanup = installSyncBridge(
      window,
      () => channel,
      bus,
      () => undefined,
      () => testCase.path,
      navigate
    );

    channel.onmessage?.({ data: { swapped: testCase.swapped } });

    expect(navigate).toHaveBeenCalledWith(testCase.counterpart);
    cleanup();
  }
});

it("does not navigate when remote swapped state already matches the route", () => {
  const cases: Array<{ path: string; swapped: boolean }> = [
    { path: "/present.html", swapped: false },
    { path: "/", swapped: false },
    { path: "/presenter", swapped: false },
    { path: "/presenter.html", swapped: false },
    { path: "/present-swapped", swapped: true },
    { path: "/presenter-swapped", swapped: true }
  ];

  for (const testCase of cases) {
    const channel = mockChannel();
    const bus = new EventTarget();
    const navigate = vi.fn();
    const cleanup = installSyncBridge(
      window,
      () => channel,
      bus,
      () => undefined,
      () => testCase.path,
      navigate
    );

    channel.onmessage?.({ data: { swapped: testCase.swapped } });

    expect(navigate).not.toHaveBeenCalled();
    cleanup();
  }
});

it("does not navigate on swapped messages received on unknown routes", () => {
  const channel = mockChannel();
  const bus = new EventTarget();
  const navigate = vi.fn();
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const cleanup = installSyncBridge(
    window,
    () => channel,
    bus,
    () => undefined,
    () => "/unknown",
    navigate
  );
  cleanups.push(cleanup);

  channel.onmessage?.({ data: { swapped: true } });

  expect(navigate).not.toHaveBeenCalled();
  expect(error).toHaveBeenCalledWith("peitho: swap unavailable on this route");
});

it("does not navigate when replayed swapped state equals the current route", async () => {
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") {
      return Promise.resolve(okJson({ seq: 7, message: null, index: 1, swapped: false }));
    }
    if (url === "/sync?seq=7") return new Promise<Response>(() => undefined);
    throw new Error(`unexpected sync url: ${url}`);
  }) as typeof fetch;
  const bus = new EventTarget();
  const navigate = vi.fn();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) =>
    requests.push((event as CustomEvent).detail)
  );
  const cleanup = installSyncBridge(
    window,
    serverSyncChannelFactory({ fetcher }),
    bus,
    () => undefined,
    () => "/present.html",
    navigate
  );
  cleanups.push(cleanup);

  await vi.waitFor(() => expect(requests).toEqual([{ to: { index: 1 } }]));

  expect(navigate).not.toHaveBeenCalled();
});

it("converges from poll response swapped state when the poll message is unrelated", async () => {
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") {
      return Promise.resolve(okJson({ seq: 7, message: null, index: 0, swapped: false }));
    }
    if (url === "/sync?seq=7") {
      return Promise.resolve(okJson({ seq: 8, message: { index: 1 }, index: 1, swapped: true }));
    }
    if (url === "/sync?seq=8") return new Promise<Response>(() => undefined);
    throw new Error(`unexpected sync url: ${url}`);
  }) as typeof fetch;
  const bus = new EventTarget();
  const navigate = vi.fn();
  const cleanup = installSyncBridge(
    window,
    serverSyncChannelFactory({ fetcher }),
    bus,
    () => undefined,
    () => "/present.html",
    navigate
  );
  cleanups.push(cleanup);

  await vi.waitFor(() => expect(navigate).toHaveBeenCalledWith("presenter-swapped"));
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
