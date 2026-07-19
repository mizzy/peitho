import { afterEach, expect, it, vi } from "vitest";
import { installRehearsalBridge } from "../src/rehearsalBridge";
import type { RehearsalSnapshot } from "../../../bindings/RehearsalSnapshot";

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.restoreAllMocks();
});

const snapshot: RehearsalSnapshot = {
  version: 1,
  elapsedMs: 1_250,
  sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 1_250 }]
};

it("posts rehearsal report events to the server", async () => {
  const bus = new EventTarget();
  const fetcher = vi.fn(async () => ({ ok: true, status: 200 }) as Response);
  const cleanup = installRehearsalBridge(window, bus, fetcher);
  cleanups.push(cleanup);

  bus.dispatchEvent(new CustomEvent("peitho:rehearsalreport", { detail: snapshot }));
  await vi.waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1));

  expect(fetcher).toHaveBeenCalledWith("/rehearsal", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    keepalive: true,
    body: JSON.stringify(snapshot)
  });
});

it("logs fetch failures", async () => {
  const bus = new EventTarget();
  const fetcher = vi.fn(async () => ({ ok: false, status: 500 }) as Response);
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const cleanup = installRehearsalBridge(window, bus, fetcher);
  cleanups.push(cleanup);

  bus.dispatchEvent(new CustomEvent("peitho:rehearsalreport", { detail: snapshot }));
  await vi.waitFor(() => expect(error).toHaveBeenCalled());

  expect(error.mock.calls[0]?.[0]).toContain("failed to POST rehearsal snapshot");
});

it("removes the report listener on cleanup", async () => {
  const bus = new EventTarget();
  const fetcher = vi.fn(async () => ({ ok: true, status: 200 }) as Response);
  const cleanup = installRehearsalBridge(window, bus, fetcher);

  cleanup();
  bus.dispatchEvent(new CustomEvent("peitho:rehearsalreport", { detail: snapshot }));

  await Promise.resolve();
  expect(fetcher).not.toHaveBeenCalled();
});
