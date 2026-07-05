import { afterEach, expect, it, vi } from "vitest";
import { installKeyboardNavigation, installPresenterKeyboard, mountPresentShell } from "../src/index";
import type { PresentShell } from "../src/index";

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
const cssText = ".slot-title { color: rebeccapurple; }";

const mountedShells: PresentShell[] = [];
const windowListenerCleanups: Array<() => void> = [];

afterEach(() => {
  while (mountedShells.length > 0) {
    mountedShells.pop()?.destroy();
  }
  while (windowListenerCleanups.length > 0) {
    windowListenerCleanups.pop()?.();
  }
  document.documentElement.style.removeProperty("--peitho-canvas-width");
  document.documentElement.style.removeProperty("--peitho-canvas-height");
  document.documentElement.style.removeProperty("--peitho-canvas-aspect");
});

async function mountForTest(options: Parameters<typeof mountPresentShell>[0]): Promise<PresentShell> {
  const shell = await mountPresentShell(options);
  mountedShells.push(shell);
  return shell;
}

function listenWindow(type: string, listener: EventListener): void {
  window.addEventListener(type, listener);
  windowListenerCleanups.push(() => window.removeEventListener(type, listener));
}

function standardFetch(responseManifest = manifest): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(responseManifest);
    if (url === "peitho.css") return okText(cssText);
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-arch-1.html") return okText("<section><pre>code</pre></section>");
    return {
      ok: false,
      status: 404,
      text: async () => "not found",
      json: async () => ({})
    } as Response;
  }) as unknown as typeof fetch;
}

it("loads manifest and fragments into shadow roots", async () => {
  const root = document.createElement("main");
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(cssText);
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-arch-1.html") return okText("<section><pre>code</pre></section>");
    throw new Error(`unexpected ${url}`);
  });

  await mountForTest({ root, fetcher: fetcher as unknown as typeof fetch });

  const hosts = root.querySelectorAll<HTMLElement>(".peitho-slide");
  expect(hosts).toHaveLength(2);
  expect(hosts[0].shadowRoot?.innerHTML).toContain("<h1>Intro</h1>");
  expect(hosts[1].shadowRoot?.innerHTML).toContain("<pre>code</pre>");
});

it("uses manifest aspect ratio for host scaling and root canvas variables", async () => {
  const root = document.createElement("main");
  const fourByThree = { ...manifest, aspectRatio: "4:3", canvasWidth: 960, canvasHeight: 720 };

  await mountForTest({
    root,
    fetcher: standardFetch(fourByThree),
    window,
    viewport: () => ({ width: 1200, height: 900 })
  });

  const host = root.querySelector<HTMLElement>(".peitho-slide");
  expect(host?.style.width).toBe("960px");
  expect(host?.style.height).toBe("720px");
  expect(host?.style.transform).toBe("translate(0px, 0px) scale(1.25)");
  expect(root.style.getPropertyValue("--peitho-canvas-width")).toBe("960px");
  expect(root.style.getPropertyValue("--peitho-canvas-height")).toBe("720px");
  expect(root.style.getPropertyValue("--peitho-canvas-aspect")).toBe("4 / 3");
  expect(document.documentElement.style.getPropertyValue("--peitho-canvas-width")).toBe("");
  expect(document.documentElement.style.getPropertyValue("--peitho-canvas-height")).toBe("");
  expect(document.documentElement.style.getPropertyValue("--peitho-canvas-aspect")).toBe("");
});

it("keeps canvas variables scoped when sibling shells share a document", async () => {
  const firstRoot = document.createElement("main");
  const secondRoot = document.createElement("main");
  const first = await mountForTest({
    root: firstRoot,
    fetcher: standardFetch(),
    window
  });
  await mountForTest({
    root: secondRoot,
    fetcher: standardFetch({
      ...manifest,
      aspectRatio: "4:3",
      canvasWidth: 960,
      canvasHeight: 720
    }),
    window
  });

  first.destroy();
  const firstIndex = mountedShells.indexOf(first);
  if (firstIndex >= 0) mountedShells.splice(firstIndex, 1);

  expect(firstRoot.style.getPropertyValue("--peitho-canvas-width")).toBe("");
  expect(firstRoot.style.getPropertyValue("--peitho-canvas-height")).toBe("");
  expect(firstRoot.style.getPropertyValue("--peitho-canvas-aspect")).toBe("");
  expect(secondRoot.style.getPropertyValue("--peitho-canvas-width")).toBe("960px");
  expect(secondRoot.style.getPropertyValue("--peitho-canvas-height")).toBe("720px");
  expect(secondRoot.style.getPropertyValue("--peitho-canvas-aspect")).toBe("4 / 3");
});

it("injects peitho css into each shadow root before fragment html", async () => {
  const root = document.createElement("main");
  await mountForTest({ root, fetcher: standardFetch() });

  const hosts = [...root.querySelectorAll<HTMLElement>(".peitho-slide")];
  for (const host of hosts) {
    const firstChild = host.shadowRoot?.firstChild;
    const style = host.shadowRoot?.querySelector("style");

    expect(firstChild).toBe(style);
    expect(style?.textContent).toContain(cssText);
  }
});

it("puts shell handles on hosts and hides non-current slides", async () => {
  const root = document.createElement("main");
  await mountForTest({ root, fetcher: standardFetch() });

  const hosts = [...root.querySelectorAll<HTMLElement>(".peitho-slide")];
  expect(hosts[0].dataset.slideKey).toBe("intro");
  expect(hosts[0].dataset.slideIndex).toBe("0");
  expect(hosts[0].hidden).toBe(false);
  expect(hosts[1].hidden).toBe(true);
});

it("scales shell slide hosts as fixed canvases and cleans up resize listeners", async () => {
  let viewport = { width: 1920, height: 1080 };
  const root = document.createElement("main");
  const shell = await mountForTest({
    root,
    fetcher: standardFetch(),
    window,
    viewport: () => viewport
  });

  const host = root.querySelector<HTMLElement>(".peitho-slide");
  expect(root.classList.contains("peitho-shell-viewport")).toBe(true);
  expect(host?.dataset.peithoCanvas).toBe("slide");
  expect(host?.style.width).toBe("1280px");
  expect(host?.style.height).toBe("720px");
  expect(host?.style.transform).toBe("translate(0px, 0px) scale(1.5)");

  viewport = { width: 1000, height: 1000 };
  window.dispatchEvent(new Event("resize"));
  expect(host?.style.transform).toBe("translate(0px, 218.75px) scale(0.78125)");

  shell.destroy();
  viewport = { width: 1280, height: 720 };
  window.dispatchEvent(new Event("resize"));
  expect(host?.style.transform).toBe("translate(0px, 218.75px) scale(0.78125)");
});

it("preserves a positioned shell root provided by the page", async () => {
  const root = document.createElement("main");
  root.style.position = "fixed";

  await mountForTest({ root, fetcher: standardFetch(), window });

  expect(root.style.position).toBe("fixed");
});

it("sets a static shell root to relative for absolute slide hosts", async () => {
  const root = document.createElement("main");

  await mountForTest({ root, fetcher: standardFetch(), window });

  expect(root.style.position).toBe("relative");
});

it("navigates from DOM events for next prev first and last", async () => {
  const root = document.createElement("main");
  const shell = await mountForTest({ root, fetcher: standardFetch(), window });
  const changes: Array<{ index: number }> = [];
  listenWindow("peitho:slidechange", (event) => {
    changes.push((event as CustomEvent).detail);
  });

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(shell.currentIndex).toBe(1);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
  expect(shell.currentIndex).toBe(0);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "last" } }));
  expect(shell.currentIndex).toBe(1);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "first" } }));
  expect(shell.currentIndex).toBe(0);
  expect(changes.map((change) => change.index)).toEqual([1, 0, 1, 0]);
});

it("navigates by key or index and reports invalid targets", async () => {
  const root = document.createElement("main");
  const error = vi.fn();
  const shell = await mountForTest({
    root,
    fetcher: standardFetch(),
    console: { error },
    window
  });

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { key: "arch-1" } } }));
  expect(shell.currentIndex).toBe(1);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 0 } } }));
  expect(shell.currentIndex).toBe(0);
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { key: "missing" } } }));
  expect(shell.currentIndex).toBe(0);
  expect(error).toHaveBeenCalledWith("Unknown slide key: missing");
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 99 } } }));
  expect(shell.currentIndex).toBe(0);
  expect(error).toHaveBeenCalledWith("Unknown slide index: 99");
});

it("emits slidechange with previousIndex after navigation", async () => {
  const root = document.createElement("main");
  const changes: unknown[] = [];
  listenWindow("peitho:slidechange", (event) => {
    changes.push((event as CustomEvent).detail);
  });

  await mountForTest({ root, fetcher: standardFetch(), window });
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(changes).toEqual([
    { key: "intro", index: 0, total: 2, previousIndex: null },
    { key: "arch-1", index: 1, total: 2, previousIndex: 0 }
  ]);
});

it("does not emit slidechange when next at the last slide is a no-op", async () => {
  const root = document.createElement("main");
  const changes: Array<{ index: number }> = [];
  listenWindow("peitho:slidechange", (event) => {
    changes.push((event as CustomEvent).detail);
  });

  const shell = await mountForTest({ root, fetcher: standardFetch(), window });
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "last" } }));
  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(shell.currentIndex).toBe(1);
  expect(changes.map((change) => change.index)).toEqual([0, 1]);
});

it("keyboard emits navigate events instead of calling shell directly", () => {
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });

  const teardown = installKeyboardNavigation(window);
  windowListenerCleanups.push(teardown);
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: " " }));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft" }));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Home" }));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "End" }));

  expect(requests).toEqual([
    { to: "next" },
    { to: "next" },
    { to: "prev" },
    { to: "first" },
    { to: "last" }
  ]);
});

it("keyboard navigation ignores chord-modified navigation keys", () => {
  const requests: unknown[] = [];
  listenWindow("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });

  const teardown = installKeyboardNavigation(window);
  windowListenerCleanups.push(teardown);
  const event = new KeyboardEvent("keydown", {
    key: "ArrowRight",
    metaKey: true,
    cancelable: true
  });
  window.dispatchEvent(event);

  expect(event.defaultPrevented).toBe(false);
  expect(requests).toEqual([]);
});

it("keyboard emits navigate events to an injected bus", () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });

  const teardown = installKeyboardNavigation(window, bus);
  windowListenerCleanups.push(teardown);
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));

  expect(requests).toEqual([{ to: "next" }]);
});

it("presenter keyboard maps Space to playpause and navigation keys to navigate", () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  const onPlaypause = vi.fn();
  bus.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });

  const teardown = installPresenterKeyboard(window, bus, onPlaypause);
  windowListenerCleanups.push(teardown);
  const space = new KeyboardEvent("keydown", { key: " ", cancelable: true });
  const repeatedSpace = new KeyboardEvent("keydown", {
    key: " ",
    repeat: true,
    cancelable: true
  });
  const arrowRight = new KeyboardEvent("keydown", { key: "ArrowRight", cancelable: true });
  const arrowLeft = new KeyboardEvent("keydown", { key: "ArrowLeft", cancelable: true });
  const home = new KeyboardEvent("keydown", { key: "Home", cancelable: true });
  const end = new KeyboardEvent("keydown", { key: "End", cancelable: true });

  window.dispatchEvent(space);
  window.dispatchEvent(repeatedSpace);
  window.dispatchEvent(arrowRight);
  window.dispatchEvent(arrowLeft);
  window.dispatchEvent(home);
  window.dispatchEvent(end);

  expect(space.defaultPrevented).toBe(true);
  expect(repeatedSpace.defaultPrevented).toBe(true);
  expect(arrowRight.defaultPrevented).toBe(true);
  expect(arrowLeft.defaultPrevented).toBe(true);
  expect(home.defaultPrevented).toBe(true);
  expect(end.defaultPrevented).toBe(true);
  expect(onPlaypause).toHaveBeenCalledTimes(1);
  expect(requests).toEqual([
    { to: "next" },
    { to: "prev" },
    { to: "first" },
    { to: "last" }
  ]);
});

it("presenter keyboard ignores chord-modified shortcuts", () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  const onPlaypause = vi.fn();
  bus.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });

  const teardown = installPresenterKeyboard(window, bus, onPlaypause);
  windowListenerCleanups.push(teardown);
  const space = new KeyboardEvent("keydown", {
    key: " ",
    metaKey: true,
    cancelable: true
  });
  const arrowRight = new KeyboardEvent("keydown", {
    key: "ArrowRight",
    metaKey: true,
    cancelable: true
  });

  window.dispatchEvent(space);
  window.dispatchEvent(arrowRight);

  expect(space.defaultPrevented).toBe(false);
  expect(arrowRight.defaultPrevented).toBe(false);
  expect(onPlaypause).not.toHaveBeenCalled();
  expect(requests).toEqual([]);
});

it("shows a visible error when a fragment fetch fails", async () => {
  const root = document.createElement("main");
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(cssText);
    if (url === "slides/000-intro.html") return okText("<section>Intro</section>");
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  });

  const shell = await mountForTest({ root, fetcher: fetcher as unknown as typeof fetch });

  expect(shell.manifest).toBeNull();
  expect(root.textContent).toContain("Failed to load slides/001-arch-1.html: 404");
  expect(root.querySelectorAll(".peitho-slide")).toHaveLength(0);
});

it("shows a visible error when peitho css fetch fails", async () => {
  const root = document.createElement("main");
  const fetcher = vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return { ok: false, status: 503, text: async () => "" } as Response;
    if (url === "slides/000-intro.html") return okText("<section>Intro</section>");
    if (url === "slides/001-arch-1.html") return okText("<section>Code</section>");
    throw new Error(`unexpected ${url}`);
  });

  const shell = await mountForTest({ root, fetcher: fetcher as unknown as typeof fetch });

  expect(shell.manifest).toBeNull();
  expect(root.textContent).toContain("Failed to load peitho.css: 503");
  expect(root.querySelectorAll(".peitho-slide")).toHaveLength(0);
});

it("shows a visible error when manifest fetch fails", async () => {
  const root = document.createElement("main");
  const fetcher = vi.fn(
    async () => ({ ok: false, status: 500, json: async () => ({}) }) as Response
  );

  const shell = await mountForTest({ root, fetcher: fetcher as unknown as typeof fetch });

  expect(shell.manifest).toBeNull();
  expect(root.textContent).toContain("Failed to load manifest.json: 500");
  expect(root.querySelectorAll(".peitho-slide")).toHaveLength(0);
});
