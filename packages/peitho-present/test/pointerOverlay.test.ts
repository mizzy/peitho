import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installPointerOverlay, mixToWhite } from "../src/shell";

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
  createRadialGradient: ReturnType<typeof vi.fn>;
};

type MockCanvasGradient = CanvasGradient & {
  stops: Array<[number, string]>;
  addColorStop: ReturnType<typeof vi.fn>;
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
  gradients: MockCanvasGradient[];
} {
  const canvas = document.createElement("canvas");
  const alphas: number[] = [];
  const gradients: MockCanvasGradient[] = [];
  const context = {
    globalAlpha: 1,
    globalCompositeOperation: "source-over",
    fillStyle: "",
    clearRect: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    beginPath: vi.fn(),
    arc: vi.fn(),
    createRadialGradient: vi.fn(() => {
      const gradient = {
        stops: [],
        addColorStop: vi.fn()
      } as unknown as MockCanvasGradient;
      gradient.addColorStop.mockImplementation((offset: number, color: string) => {
        gradient.stops.push([offset, color]);
      });
      gradients.push(gradient);
      return gradient;
    }),
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
  return { canvas, context, alphas, gradients };
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

async function setupOverlay(
  now: () => number = () => 0,
  pointerColor?: string | null
): Promise<{
  bus: EventTarget;
  frames: FrameRequestCallback[];
  polls: DeferredPoll[];
  context: MockCanvasContext;
  alphas: number[];
  gradients: MockCanvasGradient[];
}> {
  const frames = installFrameMock();
  const { canvas, context, alphas, gradients } = canvasFixture();
  const { fetcher, polls } = pointerFetch();
  const bus = new EventTarget();
  cleanups.push(installPointerOverlay({ canvas, fetcher, bus, window, now, pointerColor }));
  await flushPromises();
  expect(polls[0]?.url).toBe("/pointer?seq=0");
  return { bus, frames, polls, context, alphas, gradients };
}

function resolveMove(
  polls: DeferredPoll[],
  seq: number,
  x: number,
  y: number,
  session = "session-a"
): void {
  polls.shift()?.resolve(okJson({ seq, event: { move: { x, y } }, session }));
}

it("pointer overlay renders the default cyan gradient when no pointer color is present", async () => {
  const { frames, polls, context, gradients } = await setupOverlay();

  resolveMove(polls, 1, 0.25, 0.5);
  await flushPromises();
  frames.shift()?.(0);

  expect(context.createRadialGradient).toHaveBeenCalledWith(50, 50, 0, 50, 50, 1.2);
  expect(context.arc).toHaveBeenCalledWith(50, 50, 1.2, 0, Math.PI * 2);
  expect(context.globalCompositeOperation).toBe("source-over");
  expect(gradients[0]?.stops).toEqual([
    [0, "#e0f2fe"],
    [0.25, "#38bdf8"],
    [1, "rgba(56, 189, 248, 0)"]
  ]);
});

it("pointer overlay uses the manifest pointer color when present", async () => {
  const { frames, polls, gradients } = await setupOverlay(() => 0, "#ff2a2a");

  resolveMove(polls, 1, 0.25, 0.5);
  await flushPromises();
  frames.shift()?.(0);

  expect(gradients[0]?.stops).toEqual([
    [0, "#ffb4b4"],
    [0.25, "#ff2a2a"],
    [1, "rgba(255, 42, 42, 0)"]
  ]);
});

it("mixToWhite mixes supported pointer colors in sRGB", () => {
  expect(mixToWhite("#ff2a2a", 0.65)).toBe("#ffb4b4");
  expect(mixToWhite("#f00", 0.65)).toBe("#ffa6a6");
  expect(mixToWhite("cyan", 0.65)).toBe("#a6ffff");
});

it("pointer overlay fades the trail after up messages", async () => {
  let now = 0;
  const { frames, polls, alphas } = await setupOverlay(() => now);

  resolveMove(polls, 1, 0.25, 0.5);
  await flushPromises();

  now = 250;
  polls.shift()?.resolve(okJson({ seq: 2, event: { up: true }, session: "session-a" }));
  await flushPromises();
  frames.shift()?.(250);

  expect(alphas[0]).toBeCloseTo(0.5);
});

it("pointer overlay bounds the trail buffer", async () => {
  let now = 0;
  const { frames, polls, context } = await setupOverlay(() => now);

  for (let index = 0; index < 70; index += 1) {
    now = index;
    resolveMove(polls, index + 1, 0.25, 0.5);
    await flushPromises();
  }
  frames.shift()?.(70);

  expect(context.fill).toHaveBeenCalledTimes(64);
});

it("pointer overlay clears immediately on navigation", async () => {
  const { bus, frames, polls, context } = await setupOverlay();
  resolveMove(polls, 1, 0.25, 0.5);
  await flushPromises();
  frames.shift()?.(0);
  context.clearRect.mockClear();
  context.fill.mockClear();

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  while (frames.length > 0) {
    frames.shift()?.(0);
  }

  expect(context.clearRect).toHaveBeenCalledWith(0, 0, 200, 100);
  expect(context.fill).not.toHaveBeenCalled();
});

it("pointer overlay resets state on a different session id", async () => {
  const { frames, polls, context } = await setupOverlay();
  resolveMove(polls, 1, 0.25, 0.5);
  await flushPromises();
  frames.shift()?.(0);
  context.arc.mockClear();
  context.fill.mockClear();
  context.clearRect.mockClear();

  polls.shift()?.resolve(okJson({ seq: 2, event: { up: true }, session: "session-b" }));
  await flushPromises();
  while (frames.length > 0) {
    frames.shift()?.(0);
  }

  expect(context.clearRect).toHaveBeenCalled();
  expect(context.arc).not.toHaveBeenCalled();
  expect(context.fill).not.toHaveBeenCalled();
});
