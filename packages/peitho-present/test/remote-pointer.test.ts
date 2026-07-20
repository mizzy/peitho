import { afterEach, expect, it, vi } from "vitest";
import { installRemoteControls, installRemotePointerBridge } from "../src/remote";

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

function pointerEvent(
  type: string,
  init: MouseEventInit & { pointerId?: number } = {}
): PointerEvent {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    button: 0,
    ...init
  }) as PointerEvent;
  Object.defineProperty(event, "pointerId", {
    configurable: true,
    value: init.pointerId ?? 1
  });
  return event;
}

function previewRoot(): HTMLElement {
  const preview = document.createElement("div");
  document.body.append(preview);
  vi.spyOn(preview, "getBoundingClientRect").mockReturnValue({
    left: 10,
    top: 20,
    right: 110,
    bottom: 70,
    x: 10,
    y: 20,
    width: 100,
    height: 50,
    toJSON: () => ({})
  });
  return preview;
}

function installFrameMock(): FrameRequestCallback[] {
  const frames: FrameRequestCallback[] = [];
  const originalRequest = window.requestAnimationFrame;
  const originalCancel = window.cancelAnimationFrame;
  window.requestAnimationFrame = vi.fn((callback: FrameRequestCallback): number => {
    frames.push(callback);
    return frames.length;
  });
  window.cancelAnimationFrame = vi.fn();
  cleanups.push(() => {
    window.requestAnimationFrame = originalRequest;
    window.cancelAnimationFrame = originalCancel;
  });
  return frames;
}

function fetchRecorder(): { fetcher: typeof fetch; bodies: unknown[] } {
  const bodies: unknown[] = [];
  const fetcher = vi.fn(async (_url: string, init?: RequestInit) => {
    bodies.push(JSON.parse(String(init?.body)));
    return { ok: true, status: 200 } as Response;
  }) as typeof fetch;
  return { fetcher, bodies };
}

it("remote pointer mode toggle emits mode change requests", () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:pointermodechange", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  cleanups.push(installRemoteControls({ root, document, bus }));

  root.querySelector<HTMLButtonElement>('[data-peitho-pointer-mode="pointer"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-pointer-mode="off"]')?.click();

  expect(requests).toEqual([{ mode: "pointer" }, { mode: "off" }]);
});

it("remote pointer bridge posts move and up bodies", () => {
  const frames = installFrameMock();
  const bus = new EventTarget();
  const preview = previewRoot();
  const { fetcher, bodies } = fetchRecorder();
  cleanups.push(installRemotePointerBridge({ bus, previewRoot: preview, fetcher, window }));

  bus.dispatchEvent(new CustomEvent("peitho:pointermodechange", { detail: { mode: "pointer" } }));
  preview.dispatchEvent(pointerEvent("pointerdown", { clientX: 60, clientY: 45 }));
  frames.shift()?.(0);
  preview.dispatchEvent(pointerEvent("pointerup", { clientX: 60, clientY: 45 }));

  expect(bodies).toEqual([{ move: { x: 0.5, y: 0.5 } }, { up: true }]);
  expect(fetcher).toHaveBeenCalledWith("/pointer", {
    method: "POST",
    body: JSON.stringify({ move: { x: 0.5, y: 0.5 } }),
    keepalive: true,
    headers: { "Content-Type": "application/json" }
  });
});

it("remote pointer bridge removes preview listeners and posts final up on off", () => {
  const frames = installFrameMock();
  const bus = new EventTarget();
  const preview = previewRoot();
  const { fetcher, bodies } = fetchRecorder();
  cleanups.push(installRemotePointerBridge({ bus, previewRoot: preview, fetcher, window }));

  bus.dispatchEvent(new CustomEvent("peitho:pointermodechange", { detail: { mode: "pointer" } }));
  bus.dispatchEvent(new CustomEvent("peitho:pointermodechange", { detail: { mode: "off" } }));
  preview.dispatchEvent(pointerEvent("pointerdown", { clientX: 60, clientY: 45 }));
  frames.shift()?.(0);

  expect(bodies).toEqual([{ up: true }]);
  expect(fetcher).toHaveBeenCalledTimes(1);
});

it("remote pointer bridge coalesces moves to one post per frame", () => {
  const frames = installFrameMock();
  const bus = new EventTarget();
  const preview = previewRoot();
  const { fetcher, bodies } = fetchRecorder();
  cleanups.push(installRemotePointerBridge({ bus, previewRoot: preview, fetcher, window }));

  bus.dispatchEvent(new CustomEvent("peitho:pointermodechange", { detail: { mode: "pointer" } }));
  preview.dispatchEvent(pointerEvent("pointerdown", { clientX: 20, clientY: 25 }));
  preview.dispatchEvent(pointerEvent("pointermove", { clientX: 60, clientY: 45 }));
  preview.dispatchEvent(pointerEvent("pointermove", { clientX: 90, clientY: 60 }));
  expect(frames).toHaveLength(1);

  frames.shift()?.(0);

  expect(bodies).toEqual([{ move: { x: 0.8, y: 0.8 } }]);
});

it("remote pointer bridge clamps preview coordinates to the unit square", () => {
  const frames = installFrameMock();
  const bus = new EventTarget();
  const preview = previewRoot();
  const { fetcher, bodies } = fetchRecorder();
  cleanups.push(installRemotePointerBridge({ bus, previewRoot: preview, fetcher, window }));

  bus.dispatchEvent(new CustomEvent("peitho:pointermodechange", { detail: { mode: "pointer" } }));
  preview.dispatchEvent(pointerEvent("pointerdown", { clientX: 0, clientY: 100 }));
  frames.shift()?.(0);

  expect(bodies).toEqual([{ move: { x: 0, y: 1 } }]);
});
