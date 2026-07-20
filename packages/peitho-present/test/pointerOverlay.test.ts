import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installPointerOverlay } from "../src/shell";

type DeferredPoll = {
  url: string;
  resolve(response: Response): void;
};

type MockCanvasContext = CanvasRenderingContext2D & {
  clearRect: ReturnType<typeof vi.fn>;
  save: ReturnType<typeof vi.fn>;
  restore: ReturnType<typeof vi.fn>;
  beginPath: ReturnType<typeof vi.fn>;
  arc: ReturnType<typeof vi.fn>;
  fill: ReturnType<typeof vi.fn>;
};

const cleanups: Array<() => void> = [];

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    (() => null) as HTMLCanvasElement["getContext"]
  );
});

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

async function flushPromises(): Promise<void> {
  for (let i = 0; i < 8; i += 1) {
    await Promise.resolve();
  }
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

function canvasFixture(): {
  canvas: HTMLCanvasElement;
  context: MockCanvasContext;
  alphas: number[];
} {
  const canvas = document.createElement("canvas");
  const alphas: number[] = [];
  const context = {
    globalAlpha: 1,
    globalCompositeOperation: "source-over",
    fillStyle: "",
    clearRect: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    beginPath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(() => {
      alphas.push(context.globalAlpha);
    })
  } as unknown as MockCanvasContext;
  vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue({
    left: 0,
    top: 0,
    right: 200,
    bottom: 100,
    x: 0,
    y: 0,
    width: 200,
    height: 100,
    toJSON: () => ({})
  });
  vi.spyOn(canvas, "getContext").mockReturnValue(context);
  document.body.append(canvas);
  return { canvas, context, alphas };
}

function pointerFetch(): { fetcher: typeof fetch; polls: DeferredPoll[] } {
  const polls: DeferredPoll[] = [];
  const fetcher = vi.fn((url: string) => {
    if (url === "/pointer") {
      return Promise.resolve(okJson({ seq: 0, session: "session-a" }));
    }
    return new Promise<Response>((resolve) => {
      polls.push({ url, resolve });
    });
  }) as typeof fetch;
  return { fetcher, polls };
}

async function setupOverlay(now: () => number = () => 0): Promise<{
  bus: EventTarget;
  frames: FrameRequestCallback[];
  polls: DeferredPoll[];
  context: MockCanvasContext;
  alphas: number[];
}> {
  const frames = installFrameMock();
  const { canvas, context, alphas } = canvasFixture();
  const { fetcher, polls } = pointerFetch();
  const bus = new EventTarget();
  cleanups.push(installPointerOverlay({ canvas, fetcher, bus, window, now }));
  await flushPromises();
  expect(polls[0]?.url).toBe("/pointer?seq=0");
  return { bus, frames, polls, context, alphas };
}

it("pointer overlay renders a dot on move messages", async () => {
  const { frames, polls, context } = await setupOverlay();

  polls.shift()?.resolve(
    okJson({ seq: 1, event: { move: { x: 0.25, y: 0.5 } }, session: "session-a" })
  );
  await flushPromises();
  frames.shift()?.(0);

  expect(context.arc).toHaveBeenCalledWith(50, 50, 1.2, 0, Math.PI * 2);
  expect(context.globalCompositeOperation).toBe("multiply");
  expect(context.fillStyle).toBe("#ff2a2a");
});

it("pointer overlay fades after up messages", async () => {
  let now = 0;
  const { frames, polls, alphas } = await setupOverlay(() => now);

  polls.shift()?.resolve(
    okJson({ seq: 1, event: { move: { x: 0.25, y: 0.5 } }, session: "session-a" })
  );
  await flushPromises();
  frames.shift()?.(0);
  alphas.length = 0;

  now = 1000;
  polls.shift()?.resolve(okJson({ seq: 2, event: { up: true }, session: "session-a" }));
  await flushPromises();
  frames.shift()?.(1000);
  now = 1075;
  frames.shift()?.(1075);

  expect(alphas[0]).toBe(1);
  expect(alphas[1]).toBeCloseTo(0.5);
});

it("pointer overlay clears immediately on navigation", async () => {
  const { bus, frames, polls, context } = await setupOverlay();
  polls.shift()?.resolve(
    okJson({ seq: 1, event: { move: { x: 0.25, y: 0.5 } }, session: "session-a" })
  );
  await flushPromises();
  frames.shift()?.(0);
  context.clearRect.mockClear();

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(context.clearRect).toHaveBeenCalledWith(0, 0, 200, 100);
});

it("pointer overlay resets state on a different session id", async () => {
  const { frames, polls, context } = await setupOverlay();
  polls.shift()?.resolve(
    okJson({ seq: 1, event: { move: { x: 0.25, y: 0.5 } }, session: "session-a" })
  );
  await flushPromises();
  frames.shift()?.(0);
  context.arc.mockClear();
  context.clearRect.mockClear();

  polls.shift()?.resolve(okJson({ seq: 2, event: { up: true }, session: "session-b" }));
  await flushPromises();
  while (frames.length > 0) {
    frames.shift()?.(0);
  }

  expect(context.clearRect).toHaveBeenCalled();
  expect(context.arc).not.toHaveBeenCalled();
});
