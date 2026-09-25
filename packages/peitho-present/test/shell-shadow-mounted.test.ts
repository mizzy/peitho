import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SHADOW_MOUNTED_EVENT, mountPresentShell } from "../src/index";
import type { PresentShell, ShadowMountedDetail } from "../src/index";

type WindowWithShadowMountedBacklog = Window & {
  __peithoShadowRoots?: unknown;
};

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
      skip: false,
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    },
    {
      index: 1,
      key: "details",
      src: "slides/001-details.html",
      hasNotes: false,
      skip: false,
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    }
  ]
};

const deckCss = ".slot-title { color: rebeccapurple; }";
const slideHtml = new Map([
  ["slides/000-intro.html", "<section><h1>Intro</h1></section>"],
  ["slides/001-details.html", "<section><h1>Details</h1></section>"]
]);
const mountedShells: PresentShell[] = [];
const roots: HTMLElement[] = [];
const documentListeners: EventListener[] = [];
const windowErrorListeners: EventListener[] = [];
const testWindow = window as WindowWithShadowMountedBacklog;

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    (() => null) as HTMLCanvasElement["getContext"]
  );
});

afterEach(() => {
  for (const listener of documentListeners.splice(0)) {
    document.removeEventListener(SHADOW_MOUNTED_EVENT, listener);
  }
  for (const listener of windowErrorListeners.splice(0)) {
    window.removeEventListener("error", listener);
  }
  while (mountedShells.length > 0) {
    mountedShells.pop()?.destroy();
  }
  for (const root of roots.splice(0)) {
    root.remove();
  }
  delete testWindow.__peithoShadowRoots;
  vi.restoreAllMocks();
});

function currentBacklog(): ShadowMountedDetail[] {
  const backlog = testWindow.__peithoShadowRoots;
  if (!Array.isArray(backlog)) throw new TypeError("expected a shadow-mounted backlog");
  return backlog as ShadowMountedDetail[];
}

function fetcherForDeck(): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(deckCss);
    if (url === "fontscope.css") return okText("");
    const html = slideHtml.get(url);
    if (html !== undefined) return okText(html);
    throw new Error(`unexpected ${url}`);
  }) as unknown as typeof fetch;
}

function createRoot(connected: boolean): HTMLElement {
  const root = document.createElement("main");
  roots.push(root);
  if (connected) document.body.appendChild(root);
  return root;
}

async function mountForTest(
  root: HTMLElement,
  options: { inertSlides?: boolean } = {}
): Promise<PresentShell> {
  const shell = await mountPresentShell({
    root,
    fetcher: fetcherForDeck(),
    window,
    document,
    inertSlides: options.inertSlides
  });
  mountedShells.push(shell);
  return shell;
}

it("announces every connected host to document in slide order and stores the same details", async () => {
  const events: CustomEvent<ShadowMountedDetail>[] = [];
  let backlogWasReadyAtDispatch = true;
  const listener: EventListener = (event) => {
    const mountedEvent = event as CustomEvent<ShadowMountedDetail>;
    events.push(mountedEvent);
    const backlog = testWindow.__peithoShadowRoots;
    if (!Array.isArray(backlog) || backlog.at(-1) !== mountedEvent.detail) {
      backlogWasReadyAtDispatch = false;
    }
  };
  document.addEventListener(SHADOW_MOUNTED_EVENT, listener);
  documentListeners.push(listener);
  const root = createRoot(true);

  await mountForTest(root, { inertSlides: true });

  const hosts = Array.from(root.querySelectorAll<HTMLElement>("[data-slide-key]"));
  expect(hosts).toHaveLength(2);
  expect(events).toHaveLength(hosts.length);
  expect(events.map((event) => event.detail.key)).toEqual(["intro", "details"]);
  expect(backlogWasReadyAtDispatch).toBe(true);

  for (const [index, host] of hosts.entries()) {
    const event = events[index];
    expect(host.hasAttribute("inert")).toBe(true);
    expect(event.target).toBe(host);
    expect(event.bubbles).toBe(true);
    // Present hosts are light-DOM children, so bubbling alone reaches document here.
    // `composed: true` matters for tasks whose dispatch target is inside a shadow tree.
    expect(event.composed).toBe(true);
    expect(event.detail.root).toBe(host.shadowRoot);
    expect(event.detail.key).toBe(host.dataset.slideKey);
    expect(event.detail.index).toBe(Number(host.dataset.slideIndex));
  }

  const backlog = currentBacklog();
  expect(backlog).toHaveLength(events.length);
  for (const [index, event] of events.entries()) {
    expect(backlog[index]).toBe(event.detail);
  }
});

it("creates an empty backlog before the first slide host is appended", async () => {
  const root = createRoot(true);
  const appendChild = root.appendChild.bind(root);
  let backlogAtFirstHostAppend: unknown;
  let backlogLengthAtFirstHostAppend: number | null = null;
  vi.spyOn(root, "appendChild").mockImplementation(<T extends Node>(node: T): T => {
    if (
      backlogLengthAtFirstHostAppend === null &&
      node instanceof HTMLElement &&
      node.dataset.slideKey !== undefined
    ) {
      backlogAtFirstHostAppend = testWindow.__peithoShadowRoots;
      backlogLengthAtFirstHostAppend = Array.isArray(backlogAtFirstHostAppend)
        ? backlogAtFirstHostAppend.length
        : -1;
    }
    return appendChild(node) as T;
  });

  await mountForTest(root);

  expect(Array.isArray(backlogAtFirstHostAppend)).toBe(true);
  expect(backlogLengthAtFirstHostAppend).toBe(0);
  expect(currentBacklog()).toBe(backlogAtFirstHostAppend);
});

it("announces only after the full deck is shown and tolerates synchronous navigation", async () => {
  const root = createRoot(true);
  let firstVisibility: boolean[] | null = null;
  let visibilityAfterLastNavigation: boolean[] | null = null;
  let announcementCount = 0;
  const listener: EventListener = () => {
    if (announcementCount === 0) {
      const hosts = Array.from(root.querySelectorAll<HTMLElement>("[data-slide-key]"));
      firstVisibility = hosts.map((host) => Boolean(host.hidden));
      window.dispatchEvent(
        new CustomEvent("peitho:navigate", { detail: { to: "last" } })
      );
      visibilityAfterLastNavigation = hosts.map((host) => Boolean(host.hidden));
      window.dispatchEvent(
        new CustomEvent("peitho:navigate", { detail: { to: "first" } })
      );
    }
    announcementCount += 1;
  };
  document.addEventListener(SHADOW_MOUNTED_EVENT, listener);
  documentListeners.push(listener);

  const shell = await mountForTest(root);

  const hosts = Array.from(root.querySelectorAll<HTMLElement>("[data-slide-key]"));
  expect(announcementCount).toBe(2);
  expect(firstVisibility).toEqual([false, true]);
  expect(visibilityAfterLastNavigation).toEqual([true, false]);
  expect(hosts.filter((host) => !host.hidden)).toHaveLength(1);
  expect(hosts[0].hidden).toBe(false);
  expect(hosts[1].hidden).toBe(true);

  shell.navigate("last");
  expect(shell.currentIndex).toBe(1);
  expect(hosts[0].hidden).toBe(true);
  expect(hosts[1].hidden).toBe(false);
});

it("fills the backlog when the shell root is never attached to the document", async () => {
  const root = createRoot(false);

  await expect(mountForTest(root)).resolves.toBeDefined();

  const hosts = Array.from(root.querySelectorAll<HTMLElement>("[data-slide-key]"));
  expect(root.isConnected).toBe(false);
  expect(hosts).toHaveLength(2);
  const backlog = currentBacklog();
  expect(backlog).toHaveLength(2);
  for (const [index, host] of hosts.entries()) {
    expect(backlog[index].root).toBe(host.shadowRoot);
    expect(backlog[index].key).toBe(host.dataset.slideKey);
    expect(backlog[index].index).toBe(Number(host.dataset.slideIndex));
  }
});

it("appends two shells to one shared backlog", async () => {
  const firstRoot = createRoot(true);
  const secondRoot = createRoot(true);

  await mountForTest(firstRoot);
  const backlog = currentBacklog();
  expect(backlog).toHaveLength(2);

  await mountForTest(secondRoot);

  expect(currentBacklog()).toBe(backlog);
  expect(backlog).toHaveLength(4);
  expect(backlog.map(({ key, index }) => ({ key, index }))).toEqual([
    { key: "intro", index: 0 },
    { key: "details", index: 1 },
    { key: "intro", index: 0 },
    { key: "details", index: 1 }
  ]);
  const hosts = [firstRoot, secondRoot].flatMap((root) =>
    Array.from(root.querySelectorAll<HTMLElement>("[data-slide-key]"))
  );
  for (const [index, host] of hosts.entries()) {
    expect(backlog[index].root).toBe(host.shadowRoot);
  }
});

it("continues loading and remains usable when a shadow-mounted listener throws", async () => {
  const root = createRoot(true);
  const listenerError = new Error("listener failed");
  const reportedErrors: unknown[] = [];
  const errorListener: EventListener = (event) => {
    reportedErrors.push((event as ErrorEvent).error);
    event.preventDefault();
  };
  window.addEventListener("error", errorListener);
  windowErrorListeners.push(errorListener);

  const throwingListener: EventListener = () => {
    throw listenerError;
  };
  const announced: ShadowMountedDetail[] = [];
  const collectingListener: EventListener = (event) => {
    announced.push((event as CustomEvent<ShadowMountedDetail>).detail);
  };
  document.addEventListener(SHADOW_MOUNTED_EVENT, throwingListener);
  document.addEventListener(SHADOW_MOUNTED_EVENT, collectingListener);
  documentListeners.push(throwingListener, collectingListener);

  const shell = await mountForTest(root);

  expect(reportedErrors).toHaveLength(2);
  for (const error of reportedErrors) expect(error).toBe(listenerError);
  expect(announced).toHaveLength(2);
  expect(currentBacklog()).toHaveLength(2);

  shell.navigate("last");
  const hosts = Array.from(root.querySelectorAll<HTMLElement>("[data-slide-key]"));
  expect(shell.currentIndex).toBe(1);
  expect(hosts).toHaveLength(2);
  expect(hosts[0].hidden).toBe(true);
  expect(hosts[1].hidden).toBe(false);
});

it("shows a non-array backlog error before appending any slide host", async () => {
  const existingValue = { ownedBy: "layout" };
  testWindow.__peithoShadowRoots = existingValue;
  const root = createRoot(true);
  const appendChild = root.appendChild.bind(root);
  let appendedHostCount = 0;
  vi.spyOn(root, "appendChild").mockImplementation(<T extends Node>(node: T): T => {
    if (node instanceof HTMLElement && node.dataset.slideKey !== undefined) {
      appendedHostCount += 1;
    }
    return appendChild(node) as T;
  });

  await mountForTest(root);

  expect(appendedHostCount).toBe(0);
  expect(root.children).toHaveLength(0);
  expect(root.textContent).toBe("window.__peithoShadowRoots must be an array");
  expect(testWindow.__peithoShadowRoots).toBe(existingValue);
});
