import { afterEach, expect, it, vi } from "vitest";
import { installRehearsalBridge } from "../src/rehearsalBridge";
import type { RehearsalSnapshot } from "../../../bindings/RehearsalSnapshot";

type Deferred<T> = {
  promise: Promise<T>;
  resolve(value: T): void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.restoreAllMocks();
});

const snapshot: RehearsalSnapshot = {
  version: 2,
  elapsedMs: 1_250,
  sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 1_250 }],
  timeline: [{ key: "intro", index: 0, atMs: 0 }]
};

it("posts non-final rehearsal reports without keepalive", async () => {
  const bus = new EventTarget();
  const fetcher = vi.fn(async () => ({ ok: true, status: 200 }) as Response);
  const cleanup = installRehearsalBridge(window, bus, fetcher);
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:rehearsalreport", {
      detail: { snapshot, final: false }
    })
  );
  await vi.waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1));

  expect(fetcher).toHaveBeenCalledWith("/rehearsal", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(snapshot)
  });
});

it("uses keepalive for pagehide fallback reports without changing the body", async () => {
  const bus = new EventTarget();
  const fetcher = vi.fn(async () => ({ ok: true, status: 200 }) as Response);
  const cleanup = installRehearsalBridge(window, bus, fetcher);
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:rehearsalreport", {
      detail: { snapshot, final: true, keepalive: true }
    })
  );
  await vi.waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1));

  expect(fetcher).toHaveBeenCalledWith("/rehearsal", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    keepalive: true,
    body: JSON.stringify(snapshot)
  });
});

it("registers the final snapshot POST with beforeclose waitUntil", async () => {
  const bus = new EventTarget();
  const request = deferred<Response>();
  const fetcher = vi.fn(() => request.promise) as unknown as typeof fetch;
  const waitUntil = vi.fn();
  const cleanup = installRehearsalBridge(window, bus, fetcher);
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:rehearsalreport", {
      detail: { snapshot, final: true, waitUntil }
    })
  );

  expect(waitUntil).toHaveBeenCalledTimes(1);
  expect(fetcher).toHaveBeenCalledWith("/rehearsal", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(snapshot)
  });
  const registered = waitUntil.mock.calls[0]?.[0] as Promise<unknown>;
  let settled = false;
  void registered.then(() => {
    settled = true;
  });
  await Promise.resolve();
  expect(settled).toBe(false);

  request.resolve({ ok: true, status: 200 } as Response);
  await registered;
  expect(settled).toBe(true);
});

it("logs fetch failures", async () => {
  const bus = new EventTarget();
  const fetcher = vi.fn(async () => ({ ok: false, status: 500 }) as Response);
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const cleanup = installRehearsalBridge(window, bus, fetcher);
  cleanups.push(cleanup);

  bus.dispatchEvent(
    new CustomEvent("peitho:rehearsalreport", {
      detail: { snapshot, final: false }
    })
  );
  await vi.waitFor(() => expect(error).toHaveBeenCalled());

  expect(error.mock.calls[0]?.[0]).toContain("failed to POST rehearsal snapshot");
});

it("removes the report listener on cleanup", async () => {
  const bus = new EventTarget();
  const fetcher = vi.fn(async () => ({ ok: true, status: 200 }) as Response);
  const cleanup = installRehearsalBridge(window, bus, fetcher);

  cleanup();
  bus.dispatchEvent(
    new CustomEvent("peitho:rehearsalreport", {
      detail: { snapshot, final: false }
    })
  );

  await Promise.resolve();
  expect(fetcher).not.toHaveBeenCalled();
});
