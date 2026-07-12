import { afterEach, expect, it, vi } from "vitest";
import {
  installPreviewKeyboard,
  installPreviewReload,
  mountPreviewShell,
  previewGridColumnCount,
  type PreviewShell
} from "../src/preview";
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
  title: "Preview Demo",
  slideCount: 3,
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
      text: { title: "", body: "", code: "" }
    },
    {
      index: 1,
      key: "middle",
      src: "slides/001-middle.html",
      hasNotes: false,
      skip: false,
      text: { title: "", body: "", code: "" }
    },
    {
      index: 2,
      key: "end",
      src: "slides/002-end.html",
      hasNotes: false,
      skip: false,
      text: { title: "", body: "", code: "" }
    }
  ]
};

const cssText = ".slot-title { color: red; }";
const fontCssText = `
@import url("fonts/noto-sans-jp/index.css");
.peitho-preview-slide { color: red; }
@import url("fonts/late.css");
@font-face { font-family: "Noto Sans JP"; src: url("fonts/noto.woff2"); }
`;

function manifestWithSlideCount(slideCount: number): typeof manifest {
  return {
    ...manifest,
    slideCount,
    slides: Array.from({ length: slideCount }, (_, index) => ({
      index,
      key: `slide-${index}`,
      src: `slides/${String(index).padStart(3, "0")}.html`,
      hasNotes: false,
      skip: false,
      text: { title: "", body: "", code: "" }
    }))
  };
}

function manifestWithSlides(slides: Array<{ key: string; skip?: boolean }>): typeof manifest {
  return {
    ...manifest,
    slideCount: slides.length,
    slides: slides.map((slide, index) => ({
      index,
      key: slide.key,
      src: `slides/${String(index).padStart(3, "0")}-${slide.key}.html`,
      hasNotes: false,
      skip: slide.skip ?? false,
      text: { title: "", body: "", code: "" }
    }))
  };
}

function fetchForManifest(deck: typeof manifest, css = cssText): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "/sync") return okJson({ seq: 0, message: null, generation: 0 });
    if (url === "manifest.json") return okJson(deck);
    if (url === "peitho.css") return okText(css);
    if (url.startsWith("slides/")) return okText(`<section><h1>${url}</h1></section>`);
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  }) as typeof fetch;
}

function standardFetch(): typeof fetch {
  return fetchForManifest(manifest);
}

function setRootWidth(root: HTMLElement, width: number): void {
  Object.defineProperty(root, "clientWidth", {
    configurable: true,
    value: width
  });
}

function mockSelection(isCollapsed: boolean): void {
  vi.spyOn(window, "getSelection").mockReturnValue({ isCollapsed } as Selection);
}

const shells: PreviewShell[] = [];
const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  while (shells.length > 0) shells.pop()?.destroy();
  sessionStorage.clear();
  vi.restoreAllMocks();
});

async function mountForTest(root: HTMLElement, bus: EventTarget = window): Promise<PreviewShell> {
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: standardFetch(),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  return shell;
}

function mockChannel() {
  const channel: SyncChannel & { closed: boolean; sent: unknown[] } = {
    closed: false,
    sent: [],
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

it("computes preview grid columns from root width and clamps to one", () => {
  expect(previewGridColumnCount(1044)).toBe(3);
  expect(previewGridColumnCount(367)).toBe(1);
  expect(previewGridColumnCount(0)).toBe(1);
});

it("preview keyboard emits overview requests from o and ignores chord modifiers", () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:overviewrequest", (event) =>
    requests.push((event as CustomEvent).detail)
  );
  cleanups.push(installPreviewKeyboard(window, bus));

  const chord = new KeyboardEvent("keydown", { key: "o", metaKey: true, cancelable: true });
  const bare = new KeyboardEvent("keydown", { key: "o", cancelable: true });
  window.dispatchEvent(chord);
  window.dispatchEvent(bare);

  expect(chord.defaultPrevented).toBe(false);
  expect(bare.defaultPrevented).toBe(true);
  expect(requests).toEqual([{ action: "toggle" }]);
});

it("preview keyboard emits command requests and ignores chord-modified commands", () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  const navigations: unknown[] = [];
  bus.addEventListener("peitho:overviewrequest", (event) =>
    requests.push((event as CustomEvent).detail)
  );
  bus.addEventListener("peitho:navigate", (event) =>
    navigations.push((event as CustomEvent).detail)
  );
  cleanups.push(installPreviewKeyboard(window, bus));

  const bareEscape = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
  const chordEscape = new KeyboardEvent("keydown", {
    key: "Escape",
    metaKey: true,
    cancelable: true
  });
  const bareEnter = new KeyboardEvent("keydown", { key: "Enter", cancelable: true });
  const chordEnter = new KeyboardEvent("keydown", {
    key: "Enter",
    metaKey: true,
    cancelable: true
  });
  const bareArrow = new KeyboardEvent("keydown", { key: "ArrowRight", cancelable: true });
  const chordArrow = new KeyboardEvent("keydown", {
    key: "ArrowRight",
    metaKey: true,
    cancelable: true
  });
  const bareUp = new KeyboardEvent("keydown", { key: "ArrowUp", cancelable: true });
  const chordUp = new KeyboardEvent("keydown", {
    key: "ArrowUp",
    metaKey: true,
    cancelable: true
  });
  const bareDown = new KeyboardEvent("keydown", { key: "ArrowDown", cancelable: true });
  const chordDown = new KeyboardEvent("keydown", {
    key: "ArrowDown",
    metaKey: true,
    cancelable: true
  });

  for (const event of [
    bareEscape,
    chordEscape,
    bareEnter,
    chordEnter,
    bareArrow,
    chordArrow,
    bareUp,
    chordUp,
    bareDown,
    chordDown
  ]) {
    window.dispatchEvent(event);
  }

  expect(requests).toEqual([{ action: "toggle" }, { action: "activate" }]);
  expect(navigations).toEqual([{ to: "next" }, { to: "up" }, { to: "down" }]);
  expect(bareEscape.defaultPrevented).toBe(true);
  expect(chordEscape.defaultPrevented).toBe(false);
  expect(bareEnter.defaultPrevented).toBe(true);
  expect(chordEnter.defaultPrevented).toBe(false);
  expect(bareArrow.defaultPrevented).toBe(true);
  expect(chordArrow.defaultPrevented).toBe(false);
  expect(bareUp.defaultPrevented).toBe(false);
  expect(chordUp.defaultPrevented).toBe(false);
  expect(bareDown.defaultPrevented).toBe(false);
  expect(chordDown.defaultPrevented).toBe(false);
});

it("injects document scoped font css once for preview shells", async () => {
  const firstRoot = document.createElement("main");
  const secondRoot = document.createElement("main");
  const first = await mountPreviewShell({
    root: firstRoot,
    fetcher: fetchForManifest(manifest, fontCssText),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  const second = await mountPreviewShell({
    root: secondRoot,
    fetcher: fetchForManifest(manifest, fontCssText),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(first, second);

  const styles = document.head.querySelectorAll<HTMLStyleElement>(
    "style[data-peitho-font-scope]"
  );
  expect(styles).toHaveLength(1);
  expect(styles[0].textContent).toBe(
    [
      '@import url("fonts/noto-sans-jp/index.css");',
      '@font-face { font-family: "Noto Sans JP"; src: url("fonts/noto.woff2"); }'
    ].join("\n")
  );
});

it("removes document scoped font css when the last preview shell is destroyed", async () => {
  const firstRoot = document.createElement("main");
  const secondRoot = document.createElement("main");
  const first = await mountPreviewShell({
    root: firstRoot,
    fetcher: fetchForManifest(manifest, fontCssText),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  const second = await mountPreviewShell({
    root: secondRoot,
    fetcher: fetchForManifest(manifest, fontCssText),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(first, second);

  expect(document.head.querySelectorAll("style[data-peitho-font-scope]")).toHaveLength(1);
  first.destroy();
  expect(document.head.querySelectorAll("style[data-peitho-font-scope]")).toHaveLength(1);
  second.destroy();
  expect(document.head.querySelectorAll("style[data-peitho-font-scope]")).toHaveLength(0);
});

it("overview requests toggle between single and grid mode", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  expect(shell.mode).toBe("single");
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  expect(shell.mode).toBe("grid");
  expect(root.dataset.peithoPreviewMode).toBe("grid");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  expect(shell.mode).toBe("single");
  expect(root.dataset.peithoPreviewMode).toBe("single");
});

it("exit overview requests exit grid mode and are a no-op in single mode", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  expect(shell.mode).toBe("single");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  expect(shell.mode).toBe("grid");
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  expect(shell.mode).toBe("single");
});

it("Escape toggles between single and grid mode with the current slide selected", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  cleanups.push(installPreviewKeyboard(window, bus));

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 2 } } }));
  const enterGrid = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
  window.dispatchEvent(enterGrid);

  expect(enterGrid.defaultPrevented).toBe(true);
  expect(shell.mode).toBe("grid");
  expect(shell.currentIndex).toBe(2);
  expect(shell.selectedIndex).toBe(2);

  const exitGrid = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
  window.dispatchEvent(exitGrid);

  expect(exitGrid.defaultPrevented).toBe(true);
  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(2);
});

it("Enter activate request enters grid from single mode with the current slide selected", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 2 } } }));
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } }));

  expect(shell.mode).toBe("grid");
  expect(shell.currentIndex).toBe(2);
  expect(shell.selectedIndex).toBe(2);
});

it("grid arrow navigation moves selection and Enter shows the selected slide", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
  expect(shell.mode).toBe("grid");
  expect(shell.selectedIndex).toBe(1);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } }));

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(1);
  const hosts = [...root.querySelectorAll<HTMLElement>(".peitho-preview-slide")];
  expect(hosts.map((host) => host.hidden)).toEqual([true, false, true]);
});

it("grid arrow navigation scrolls the selected tile into view", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  const targetTile = root.querySelectorAll<HTMLElement>(".peitho-preview-tile")[1];
  const scrollIntoView = vi.fn();
  targetTile.scrollIntoView = scrollIntoView;

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(shell.mode).toBe("grid");
  expect(shell.selectedIndex).toBe(1);
  expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest" });
});

it("grid mode sets scroll padding and single mode clears it", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));

  expect(shell.mode).toBe("grid");
  expect(root.style.scrollPaddingTop).toBe("24px");
  expect(root.style.scrollPaddingBottom).toBe("24px");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));

  expect(shell.mode).toBe("single");
  expect(root.style.scrollPaddingTop).toBe("");
  expect(root.style.scrollPaddingBottom).toBe("");
});

it("grid selection styling keeps tile size stable without changing selection classes", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));

  const [selectedTile, unselectedTile] = root.querySelectorAll<HTMLElement>(
    ".peitho-preview-tile"
  );
  expect(shell.mode).toBe("grid");
  expect(selectedTile.classList.contains("is-selected")).toBe(true);
  expect(unselectedTile.classList.contains("is-selected")).toBe(false);
  expect(selectedTile.style.borderWidth).toBe("1px");
  expect(unselectedTile.style.borderWidth).toBe("1px");
  expect(selectedTile.style.outlineWidth).toBe("3px");
  expect(selectedTile.style.outlineStyle).toBe("solid");
  expect(unselectedTile.style.outline).toBe("");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));

  expect(shell.mode).toBe("single");
  expect(selectedTile.style.border).toBe("0px");
  expect(selectedTile.style.outline).toBe("");
});

it("entering grid scrolls the current slide tile into view", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 2 } } }));
  const targetTile = root.querySelectorAll<HTMLElement>(".peitho-preview-tile")[2];
  const scrollIntoView = vi.fn();
  targetTile.scrollIntoView = scrollIntoView;
  expect(scrollIntoView).not.toHaveBeenCalled();

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } }));

  expect(shell.mode).toBe("grid");
  expect(shell.selectedIndex).toBe(2);
  expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest" });
});

it("single mode navigation does not scroll preview tiles into view", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  const targetTile = root.querySelectorAll<HTMLElement>(".peitho-preview-tile")[1];
  const scrollIntoView = vi.fn();
  targetTile.scrollIntoView = scrollIntoView;

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(1);
  expect(scrollIntoView).not.toHaveBeenCalled();
});

it("single mode next skips one or more skipped slides in preview", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(
      manifestWithSlides([
        { key: "intro" },
        { key: "appendix-a", skip: true },
        { key: "appendix-b", skip: true },
        { key: "summary" }
      ])
    ),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(3);
  expect(shell.selectedIndex).toBe(3);
});

it("single mode prev skips one or more skipped slides in preview", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(
      manifestWithSlides([
        { key: "intro" },
        { key: "appendix-a", skip: true },
        { key: "appendix-b", skip: true },
        { key: "summary" }
      ])
    ),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 3 } } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(0);
  expect(shell.selectedIndex).toBe(0);
});

it("single mode next is a no-op when only skipped slides remain in preview", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(
      manifestWithSlides([
        { key: "intro" },
        { key: "appendix-a", skip: true },
        { key: "appendix-b", skip: true }
      ])
    ),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  const event = new CustomEvent("peitho:navigate", {
    cancelable: true,
    detail: { to: "next" }
  });
  bus.dispatchEvent(event);

  expect(shell.currentIndex).toBe(0);
  expect(shell.selectedIndex).toBe(0);
  expect(event.defaultPrevented).toBe(false);
});

it("grid next navigation can select and activate a skipped slide in preview", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(
      manifestWithSlides([
        { key: "intro" },
        { key: "appendix", skip: true },
        { key: "summary" }
      ])
    ),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } }));

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(1);
  expect(shell.selectedIndex).toBe(1);
});

it("grid vertical navigation moves by one computed row and stops at row edges", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  setRootWidth(root, 1044);
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(manifestWithSlideCount(7)),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1044, height: 720 })
  });
  shells.push(shell);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 1 } } }));
  const handledDown = new CustomEvent("peitho:navigate", {
    cancelable: true,
    detail: { to: "down" }
  });
  bus.dispatchEvent(handledDown);
  expect(shell.selectedIndex).toBe(4);
  expect(handledDown.defaultPrevented).toBe(true);

  const handledUp = new CustomEvent("peitho:navigate", {
    cancelable: true,
    detail: { to: "up" }
  });
  bus.dispatchEvent(handledUp);
  expect(shell.selectedIndex).toBe(1);
  expect(handledUp.defaultPrevented).toBe(true);

  const blockedUp = new CustomEvent("peitho:navigate", {
    cancelable: true,
    detail: { to: "up" }
  });
  bus.dispatchEvent(blockedUp);
  expect(shell.selectedIndex).toBe(1);
  expect(blockedUp.defaultPrevented).toBe(false);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 4 } } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "down" } }));
  expect(shell.selectedIndex).toBe(4);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 3 } } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "down" } }));
  expect(shell.selectedIndex).toBe(6);
});

it("single mode ignores preview vertical navigation requests", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  setRootWidth(root, 1044);
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(manifestWithSlideCount(7)),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1044, height: 720 })
  });
  shells.push(shell);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 3 } } }));
  const saved = sessionStorage.getItem("peitho:preview-state");
  const up = new CustomEvent("peitho:navigate", { cancelable: true, detail: { to: "up" } });
  const down = new CustomEvent("peitho:navigate", { cancelable: true, detail: { to: "down" } });
  bus.dispatchEvent(up);
  bus.dispatchEvent(down);

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(3);
  expect(shell.selectedIndex).toBe(3);
  expect(sessionStorage.getItem("peitho:preview-state")).toBe(saved);
  expect(up.defaultPrevented).toBe(false);
  expect(down.defaultPrevented).toBe(false);
});

it("clicking a grid tile shows that slide in single mode", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  mockSelection(true);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  root.querySelectorAll<HTMLElement>(".peitho-preview-tile")[2].click();

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(2);
});

it("dragging across a grid tile does not activate it on the follow-up click", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  mockSelection(true);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  const tile = root.querySelectorAll<HTMLElement>(".peitho-preview-tile")[2];
  tile.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, clientX: 100, clientY: 100 }));
  tile.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientX: 112, clientY: 100 }));
  tile.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, clientX: 112, clientY: 100 }));
  tile.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 112, clientY: 100 }));

  expect(shell.mode).toBe("grid");
  expect(shell.currentIndex).toBe(0);
});

it("clicking a grid tile with non-collapsed selection does not activate it", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  mockSelection(false);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  root
    .querySelectorAll<HTMLElement>(".peitho-preview-tile")[2]
    .dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 900 }));

  expect(shell.mode).toBe("grid");
  expect(shell.currentIndex).toBe(0);
});

it("saves and restores mode and slide index from sessionStorage", async () => {
  const bus = new EventTarget();
  const firstRoot = document.createElement("main");
  const first = await mountForTest(firstRoot, bus);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  first.saveState();
  first.destroy();
  shells.pop();

  const secondRoot = document.createElement("main");
  const second = await mountForTest(secondRoot, new EventTarget());

  expect(second.mode).toBe("grid");
  expect(second.selectedIndex).toBe(1);
  expect(second.currentIndex).toBe(1);
});

it("ignores preview commands while content is still loading without clobbering saved state", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const saved = JSON.stringify({ mode: "grid", index: 2 });
  sessionStorage.setItem("peitho:preview-state", saved);
  let resolveSync: (response: Response) => void = () => {
    throw new Error("sync handshake was not requested");
  };
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") {
      return new Promise<Response>((resolve) => {
        resolveSync = resolve;
      });
    }
    if (url === "manifest.json") return Promise.resolve(okJson(manifest));
    if (url === "peitho.css") return Promise.resolve(okText(cssText));
    if (url === "slides/000-intro.html") return Promise.resolve(okText("<section><h1>Intro</h1></section>"));
    if (url === "slides/001-middle.html") return Promise.resolve(okText("<section><h1>Middle</h1></section>"));
    if (url === "slides/002-end.html") return Promise.resolve(okText("<section><h1>End</h1></section>"));
    return Promise.resolve({ ok: false, status: 404, text: async () => "not found" } as Response);
  }) as typeof fetch;

  const mounted = mountPreviewShell({
    root,
    bus,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));

  expect(sessionStorage.getItem("peitho:preview-state")).toBe(saved);

  resolveSync(okJson({ seq: 0, message: null, generation: 0 }));
  const shell = await mounted;
  shells.push(shell);

  expect(shell.mode).toBe("grid");
  expect(shell.currentIndex).toBe(2);
  expect(shell.selectedIndex).toBe(2);
});

it("ignores corrupt preview state JSON and starts at the first slide", async () => {
  sessionStorage.setItem("peitho:preview-state", "{not json");
  const root = document.createElement("main");

  const shell = await mountForTest(root, new EventTarget());

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(0);
  expect(shell.selectedIndex).toBe(0);
});

it("starts on the first non-skipped slide in preview when there is no restored state", async () => {
  const root = document.createElement("main");
  const shell = await mountPreviewShell({
    root,
    bus: new EventTarget(),
    fetcher: fetchForManifest(
      manifestWithSlides([
        { key: "intro", skip: true },
        { key: "main" },
        { key: "appendix", skip: true }
      ])
    ),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(1);
  expect(shell.selectedIndex).toBe(1);
});

it("restores a skipped slide index exactly in preview", async () => {
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 1 }));
  const root = document.createElement("main");
  const shell = await mountPreviewShell({
    root,
    bus: new EventTarget(),
    fetcher: fetchForManifest(
      manifestWithSlides([
        { key: "intro" },
        { key: "appendix", skip: true },
        { key: "summary" }
      ])
    ),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(1);
  expect(shell.selectedIndex).toBe(1);
});

it("handshakes sync generation before fetching preview content", async () => {
  const root = document.createElement("main");
  const calls: string[] = [];
  const fetcher = vi.fn(async (url: string) => {
    calls.push(url);
    if (url === "/sync") return okJson({ seq: 7, message: null, generation: 4 });
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(cssText);
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-middle.html") return okText("<section><h1>Middle</h1></section>");
    if (url === "slides/002-end.html") return okText("<section><h1>End</h1></section>");
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  }) as typeof fetch;

  const shell = await mountPreviewShell({
    root,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  expect(calls[0]).toBe("/sync");
  expect(calls[1]).toBe("manifest.json");
  expect(shell.generation).toBe(4);
});

it("fetches preview slide fragments in parallel", async () => {
  const root = document.createElement("main");
  const requestedSlides: string[] = [];
  const slideResponses = new Map<string, (response: Response) => void>();
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") return Promise.resolve(okJson({ seq: 0, message: null, generation: 0 }));
    if (url === "manifest.json") return Promise.resolve(okJson(manifest));
    if (url === "peitho.css") return Promise.resolve(okText(cssText));
    if (url.startsWith("slides/")) {
      requestedSlides.push(url);
      return new Promise<Response>((resolve) => {
        slideResponses.set(url, resolve);
      });
    }
    return Promise.resolve({ ok: false, status: 404, text: async () => "not found" } as Response);
  }) as typeof fetch;

  const mounted = mountPreviewShell({
    root,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });

  await vi.waitFor(() =>
    expect(requestedSlides).toEqual([
      "slides/000-intro.html",
      "slides/001-middle.html",
      "slides/002-end.html"
    ])
  );

  slideResponses.get("slides/000-intro.html")?.(okText("<section><h1>Intro</h1></section>"));
  slideResponses.get("slides/001-middle.html")?.(okText("<section><h1>Middle</h1></section>"));
  slideResponses.get("slides/002-end.html")?.(okText("<section><h1>End</h1></section>"));

  const shell = await mounted;
  shells.push(shell);
  expect(root.querySelectorAll(".peitho-preview-slide")).toHaveLength(3);
});

it("generation changes save preview state before reloading", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  const channel = mockChannel();
  const reload = vi.fn();

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  cleanups.push(installPreviewReload(shell, () => channel, reload));

  channel.onmessage?.({ data: { generation: shell.generation } });
  expect(reload).not.toHaveBeenCalled();
  channel.onmessage?.({ data: { generation: shell.generation + 1 } });

  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state") ?? "{}")).toEqual({
    mode: "grid",
    index: 1
  });
  expect(reload).toHaveBeenCalledTimes(1);
});
