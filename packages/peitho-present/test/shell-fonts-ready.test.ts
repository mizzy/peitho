import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { mountPresentShell } from "../src/index";
import type { PresentShell } from "../src/index";

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function okText(value: string): Response {
  return { ok: true, status: 200, text: async () => value } as Response;
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T | PromiseLike<T>) => void } {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((innerResolve) => {
    resolve = innerResolve;
  });
  return { promise, resolve };
}

async function flushMicrotasks(count = 50): Promise<void> {
  for (let index = 0; index < count; index += 1) {
    await Promise.resolve();
  }
}

const manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 1,
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
      skip: false,
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    }
  ]
};

const mountedShells: PresentShell[] = [];

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    (() => null) as HTMLCanvasElement["getContext"]
  );
});

afterEach(() => {
  while (mountedShells.length > 0) {
    mountedShells.pop()?.destroy();
  }
  Reflect.deleteProperty(document, "fonts");
  vi.restoreAllMocks();
});

it("waits for document fonts before appending slide hosts", async () => {
  const root = document.createElement("main");
  const fontsReady = deferred<void>();
  const load = vi.fn(async (_font: string, _text?: string) => []);
  Object.defineProperty(document, "fonts", {
    configurable: true,
    value: {
      status: "loading",
      ready: fontsReady.promise,
      load,
      forEach: (callback: (face: FontFace) => void) => {
        callback({ family: "Deck", weight: "400", style: "normal" } as unknown as FontFace);
      }
    }
  });
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText('@font-face { font-family: "Deck"; src: url("deck.woff2"); }');
    if (url === "fontscope.css") return okText('@font-face { font-family: "Deck"; src: url("deck.woff2"); font-display:block; }');
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    throw new Error(`unexpected ${url}`);
  });

  const mounting = mountPresentShell({
    root,
    fetcher: fetcher as unknown as typeof fetch,
    window,
    document
  });
  await flushMicrotasks();

  expect(root.children.length).toBe(0);
  // The slide text reached fonts.load(), so only the covering subsets get fetched.
  expect(load).toHaveBeenCalledTimes(1);
  expect(load.mock.calls[0]?.[0]).toBe("normal 400 1em Deck");
  // Deduped characters, not the raw markup: every glyph of "Intro", no tags.
  const text = load.mock.calls[0]?.[1] ?? "";
  for (const char of "Intro") expect(text).toContain(char);
  expect(text).not.toContain("<");

  fontsReady.resolve();
  const shell = await mounting;
  mountedShells.push(shell);

  expect(root.children.length).toBeGreaterThan(0);
});
