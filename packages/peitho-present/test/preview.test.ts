import { afterEach, expect, it, vi } from "vitest";
import {
  installPreviewKeyboard,
  installPreviewReload,
  mountPreviewShell,
  PREVIEW_NOTES_HEIGHT,
  PREVIEW_STRIP_WIDTH,
  previewGridColumnCount,
  type PreviewShell
} from "../src/preview";
import type { Notes } from "../../../bindings/Notes";
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
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    },
    {
      index: 1,
      key: "middle",
      src: "slides/001-middle.html",
      hasNotes: false,
      skip: false,
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    },
    {
      index: 2,
      key: "end",
      src: "slides/002-end.html",
      hasNotes: false,
      skip: false,
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    }
  ]
};

const notes = { version: 1, notes: { middle: "Pause here.\nThen ask." } };
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
      revealSteps: 0,
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
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    }))
  };
}

type PreviewFetchFixture = {
  fetcher: typeof fetch;
  notes: Notes;
  notesPosts(): Array<[string, RequestInit]>;
  resolveNotesPost(response: Response): void;
  rejectNotesPost(error: unknown): void;
};

function previewFetchFixture(
  deck: typeof manifest = manifest,
  css = cssText,
  sourceNotes: Notes = notes
): PreviewFetchFixture {
  // The shell receives this same object from notes.json, so map assertions observe its updates.
  const loadedNotes: Notes = { version: sourceNotes.version, notes: { ...sourceNotes.notes } };
  const posts: Array<[string, RequestInit]> = [];
  const notesPostSettlers: Array<{
    resolve(response: Response): void;
    reject(error: unknown): void;
  }> = [];
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/notes") {
      posts.push([url, init ?? {}]);
      return new Promise<Response>((resolve, reject) =>
        notesPostSettlers.push({ resolve, reject })
      );
    }
    if (url === "/sync") {
      return okJson({ seq: 0, message: null, generation: 0, buildError: null });
    }
    if (url === "manifest.json") return okJson(deck);
    if (url === "notes.json") return okJson(loadedNotes);
    if (url === "peitho.css") return okText(css);
    if (url.startsWith("slides/")) return okText(`<section><h1>${url}</h1></section>`);
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  }) as unknown as typeof fetch;
  return {
    fetcher,
    notes: loadedNotes,
    notesPosts: () => posts,
    resolveNotesPost(response: Response): void {
      const settler = notesPostSettlers.shift();
      if (settler === undefined) throw new Error("No pending /notes request");
      settler.resolve(response);
    },
    rejectNotesPost(error: unknown): void {
      const settler = notesPostSettlers.shift();
      if (settler === undefined) throw new Error("No pending /notes request");
      settler.reject(error);
    }
  };
}

function fetchForManifest(deck: typeof manifest, css = cssText): typeof fetch {
  return previewFetchFixture(deck, css).fetcher;
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

it("sets the document title from the manifest", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());

  await mountForTest(root);

  expect(document.title).toBe("Preview Demo");
});

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

it("preview_keyboard_only_dispatches_page_keys_from_editable_targets", () => {
  const bus = new EventTarget();
  const navigations: unknown[] = [];
  const overviewRequests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) =>
    navigations.push((event as CustomEvent).detail)
  );
  bus.addEventListener("peitho:overviewrequest", (event) =>
    overviewRequests.push((event as CustomEvent).detail)
  );
  cleanups.push(installPreviewKeyboard(window, bus));

  const textarea = document.createElement("textarea");
  const input = document.createElement("input");
  const select = document.createElement("select");
  const contenteditableRoot = document.createElement("div");
  contenteditableRoot.setAttribute("contenteditable", "true");
  const contenteditableChild = document.createElement("span");
  contenteditableRoot.appendChild(contenteditableChild);
  if (!("isContentEditable" in contenteditableChild)) {
    Object.defineProperty(contenteditableChild, "isContentEditable", { value: true });
  }
  const shadowHost = document.createElement("div");
  const shadowTextarea = document.createElement("textarea");
  shadowHost.attachShadow({ mode: "open" }).appendChild(shadowTextarea);
  document.body.append(textarea, input, select, contenteditableRoot, shadowHost);
  cleanups.push(() => {
    textarea.remove();
    input.remove();
    select.remove();
    contenteditableRoot.remove();
    shadowHost.remove();
  });

  const pageDown = new KeyboardEvent("keydown", {
    key: "PageDown",
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(pageDown);
  expect(navigations).toEqual([{ to: "next" }]);
  expect(pageDown.defaultPrevented).toBe(false);

  for (const key of [
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Home",
    "End",
    "o",
    "Enter",
    "Escape"
  ]) {
    const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
    textarea.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  }
  expect(navigations).toEqual([{ to: "next" }]);
  expect(overviewRequests).toEqual([]);

  const pageUp = new KeyboardEvent("keydown", {
    key: "PageUp",
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(pageUp);
  expect(navigations).toEqual([{ to: "next" }, { to: "prev" }]);
  expect(pageUp.defaultPrevented).toBe(false);

  for (const editable of [input, select, contenteditableChild]) {
    const editablePageDown = new KeyboardEvent("keydown", {
      key: "PageDown",
      bubbles: true,
      cancelable: true
    });
    const editableArrowRight = new KeyboardEvent("keydown", {
      key: "ArrowRight",
      bubbles: true,
      cancelable: true
    });
    editable.dispatchEvent(editablePageDown);
    editable.dispatchEvent(editableArrowRight);
    expect(editablePageDown.defaultPrevented).toBe(false);
    expect(editableArrowRight.defaultPrevented).toBe(false);
  }
  expect(navigations).toEqual([
    { to: "next" },
    { to: "prev" },
    { to: "next" },
    { to: "next" },
    { to: "next" }
  ]);

  for (const key of ["Escape", "ArrowRight"]) {
    const event = new KeyboardEvent("keydown", {
      key,
      bubbles: true,
      composed: true,
      cancelable: true
    });
    shadowTextarea.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  }
  expect(navigations).toHaveLength(5);
  expect(overviewRequests).toEqual([]);
  const shadowPageDown = new KeyboardEvent("keydown", {
    key: "PageDown",
    bubbles: true,
    composed: true,
    cancelable: true
  });
  shadowTextarea.dispatchEvent(shadowPageDown);
  expect(shadowPageDown.defaultPrevented).toBe(false);
  expect(navigations).toHaveLength(6);
  expect(navigations.at(-1)).toEqual({ to: "next" });

  for (const key of ["PageUp", "PageDown"]) {
    for (const modifier of [{ metaKey: true }, { shiftKey: true }]) {
      const event = new KeyboardEvent("keydown", {
        key,
        ...modifier,
        bubbles: true,
        cancelable: true
      });
      textarea.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
      expect(navigations).toHaveLength(6);
    }
  }
});

it("preview_keyboard_leaves_enter_on_links_untouched", () => {
  const bus = new EventTarget();
  const overviewRequests: unknown[] = [];
  bus.addEventListener("peitho:overviewrequest", (event) =>
    overviewRequests.push((event as CustomEvent).detail)
  );
  cleanups.push(installPreviewKeyboard(window, bus));
  const link = document.createElement("a");
  link.href = "#x";
  document.body.appendChild(link);
  cleanups.push(() => link.remove());
  link.focus();

  const enter = new KeyboardEvent("keydown", {
    key: "Enter",
    bubbles: true,
    cancelable: true
  });
  link.dispatchEvent(enter);

  expect(overviewRequests).toEqual([]);
  expect(enter.defaultPrevented).toBe(false);
});

it("editable_page_navigation_is_prevented_only_when_accepted", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 2 }));
  const shell = await mountForTest(root, bus);
  cleanups.push(installPreviewKeyboard(window, bus));
  const textarea = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;

  const boundaryPageDown = new KeyboardEvent("keydown", {
    key: "PageDown",
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(boundaryPageDown);
  expect(boundaryPageDown.defaultPrevented).toBe(false);
  expect(shell.currentIndex).toBe(2);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 1 } } }));
  expect(shell.currentIndex).toBe(1);
  const acceptedPageDown = new KeyboardEvent("keydown", {
    key: "PageDown",
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(acceptedPageDown);
  expect(acceptedPageDown.defaultPrevented).toBe(true);
  expect(shell.currentIndex).toBe(2);
});

it("composing_keys_are_ignored_in_the_notes_textarea", async () => {
  const bus = new EventTarget();
  const navigations: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) =>
    navigations.push((event as CustomEvent).detail)
  );
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountForTest(root, bus);
  cleanups.push(installPreviewKeyboard(window, bus));
  const textarea = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;

  textarea.focus();
  expect(document.activeElement).toBe(textarea);
  const composingEscape = new KeyboardEvent("keydown", {
    key: "Escape",
    isComposing: true,
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(composingEscape);
  expect(document.activeElement).toBe(textarea);
  expect(composingEscape.defaultPrevented).toBe(false);

  const safariCompositionEscape = new KeyboardEvent("keydown", {
    key: "Escape",
    isComposing: false,
    bubbles: true,
    cancelable: true
  });
  Object.defineProperty(safariCompositionEscape, "keyCode", { value: 229 });
  textarea.dispatchEvent(safariCompositionEscape);
  expect(document.activeElement).toBe(textarea);
  expect(safariCompositionEscape.defaultPrevented).toBe(false);

  const composingPageDown = new KeyboardEvent("keydown", {
    key: "PageDown",
    isComposing: true,
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(composingPageDown);
  expect(composingPageDown.defaultPrevented).toBe(false);
  expect(navigations).toEqual([]);
  expect(shell.currentIndex).toBe(0);
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

  expect(requests).toEqual([{ action: "enter" }, { action: "activate" }]);
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
      '@font-face { font-family: "Noto Sans JP"; src: url("fonts/noto.woff2"); font-display:block;}'
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

  expect(shell.mode).toBe("grid");
  expect(root.dataset.peithoPreviewMode).toBe("grid");
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  expect(shell.mode).toBe("single");
  expect(root.dataset.peithoPreviewMode).toBe("single");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  expect(shell.mode).toBe("grid");
  expect(root.dataset.peithoPreviewMode).toBe("grid");
});

it("exit overview requests exit grid mode and are a no-op in single mode", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

  expect(shell.mode).toBe("grid");
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  expect(shell.mode).toBe("single");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  expect(shell.mode).toBe("single");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));
  expect(shell.mode).toBe("grid");
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  expect(shell.mode).toBe("single");
});

it("Escape returns to grid mode with the current slide selected", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  cleanups.push(installPreviewKeyboard(window, bus));

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 2 } } }));
  const enterGrid = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
  window.dispatchEvent(enterGrid);

  expect(enterGrid.defaultPrevented).toBe(true);
  expect(shell.mode).toBe("grid");
  expect(shell.currentIndex).toBe(2);
  expect(shell.selectedIndex).toBe(2);
});

it("escape_in_the_notes_textarea_blurs_before_entering_grid", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  cleanups.push(installPreviewKeyboard(window, bus));
  const textarea = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;

  textarea.value = "edited with Escape";
  textarea.focus();
  expect(document.activeElement).toBe(textarea);
  const blurEditor = new KeyboardEvent("keydown", {
    key: "Escape",
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(blurEditor);

  expect(document.activeElement).not.toBe(textarea);
  expect(shell.mode).toBe("single");
  expect(blurEditor.defaultPrevented).toBe(true);
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(JSON.parse(fixture.notesPosts()[0][1].body as string)).toEqual({
    key: "intro",
    text: "edited with Escape"
  });
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notes.notes.intro).toBe("edited with Escape"));

  const repeatedEscape = new KeyboardEvent("keydown", {
    key: "Escape",
    repeat: true,
    cancelable: true
  });
  window.dispatchEvent(repeatedEscape);
  expect(repeatedEscape.defaultPrevented).toBe(false);
  expect(shell.mode).toBe("single");

  const repeatedArrowRight = new KeyboardEvent("keydown", {
    key: "ArrowRight",
    repeat: true,
    cancelable: true
  });
  window.dispatchEvent(repeatedArrowRight);
  expect(repeatedArrowRight.defaultPrevented).toBe(true);
  expect(shell.currentIndex).toBe(1);
  expect(shell.mode).toBe("single");

  const enterGrid = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
  window.dispatchEvent(enterGrid);
  expect(enterGrid.defaultPrevented).toBe(true);
  expect(shell.mode).toBe("grid");
});

it("Escape in grid mode stays in grid mode", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  cleanups.push(installPreviewKeyboard(window, bus));

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: 2 } } }));
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } }));
  expect(shell.mode).toBe("grid");

  const stayInGrid = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
  window.dispatchEvent(stayInGrid);

  expect(stayInGrid.defaultPrevented).toBe(true);
  expect(shell.mode).toBe("grid");
  expect(shell.selectedIndex).toBe(2);
});

it("repeated_enter_in_grid_mode_is_ignored", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  cleanups.push(installPreviewKeyboard(window, bus));

  const repeatedEnter = new KeyboardEvent("keydown", {
    key: "Enter",
    repeat: true,
    cancelable: true
  });
  window.dispatchEvent(repeatedEnter);

  expect(repeatedEnter.defaultPrevented).toBe(false);
  expect(shell.mode).toBe("grid");
});

it("enter_in_single_mode_focuses_the_notes_textarea_at_the_end", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const fixture = previewFetchFixture(manifest, cssText, {
    version: 1,
    notes: { end: "abc" }
  });
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 2 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  cleanups.push(installPreviewKeyboard(window, bus));
  const textarea = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;

  textarea.setSelectionRange(1, 1);
  const focusNotes = new KeyboardEvent("keydown", { key: "Enter", cancelable: true });
  window.dispatchEvent(focusNotes);

  expect(document.activeElement).toBe(textarea);
  expect(shell.currentIndex).toBe(2);
  expect(shell.selectedIndex).toBe(2);
  expect(textarea.selectionStart).toBe(textarea.value.length);
  expect(textarea.selectionEnd).toBe(textarea.value.length);
  expect(focusNotes.defaultPrevented).toBe(true);

  const repeatedFocusingEnter = new KeyboardEvent("keydown", {
    key: "Enter",
    repeat: true,
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(repeatedFocusingEnter);

  expect(repeatedFocusingEnter.defaultPrevented).toBe(true);

  const enterInNotes = new KeyboardEvent("keydown", {
    key: "Enter",
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(enterInNotes);

  expect(document.activeElement).toBe(textarea);
  expect(shell.mode).toBe("single");
  expect(enterInNotes.defaultPrevented).toBe(false);

  const repeatedTypedEnter = new KeyboardEvent("keydown", {
    key: "Enter",
    repeat: true,
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(repeatedTypedEnter);

  expect(repeatedTypedEnter.defaultPrevented).toBe(false);

  const blurNotes = new KeyboardEvent("keydown", {
    key: "Escape",
    bubbles: true,
    cancelable: true
  });
  textarea.dispatchEvent(blurNotes);

  expect(document.activeElement).not.toBe(textarea);
  expect(shell.mode).toBe("single");

  const enterGrid = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
  window.dispatchEvent(enterGrid);
  await vi.waitFor(() => expect(shell.mode).toBe("grid"));

  const activateSlide = new KeyboardEvent("keydown", { key: "Enter", cancelable: true });
  window.dispatchEvent(activateSlide);

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(2);
  expect(shell.selectedIndex).toBe(2);
  expect(document.activeElement).not.toBe(textarea);
});

it("enter_in_single_mode_focuses_an_empty_notes_textarea_at_zero", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  await mountForTest(root, bus);
  cleanups.push(installPreviewKeyboard(window, bus));
  const textarea = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;

  const focusNotes = new KeyboardEvent("keydown", { key: "Enter", cancelable: true });
  window.dispatchEvent(focusNotes);

  expect(textarea.value).toBe("");
  expect(document.activeElement).toBe(textarea);
  expect(textarea.selectionStart).toBe(0);
  expect(textarea.selectionEnd).toBe(0);
});

it("entering_grid_blurs_a_focused_textarea", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const textarea = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;

  textarea.value = "dirty";
  textarea.focus();
  textarea.blur();
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } }));
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } }));
  expect(document.activeElement).toBe(textarea);

  fixture.resolveNotesPost(okJson({ saved: true }));

  await vi.waitFor(() => {
    expect(shell.mode).toBe("grid");
    expect(document.activeElement).not.toBe(textarea);
  });
});

it("grid arrow navigation moves selection and Enter shows the selected slide", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

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

  expect(shell.mode).toBe("grid");
  expect(root.style.scrollPaddingTop).toBe("24px");
  expect(root.style.scrollPaddingBottom).toBe("24px");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));

  expect(shell.mode).toBe("single");
  expect(root.style.scrollPaddingTop).toBe("");
  expect(root.style.scrollPaddingBottom).toBe("");

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } }));

  expect(shell.mode).toBe("grid");
  expect(root.style.scrollPaddingTop).toBe("24px");
  expect(root.style.scrollPaddingBottom).toBe("24px");
});

it("grid selection styling keeps tile size stable without changing selection classes", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);

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

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
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
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
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

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
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

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
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

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
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

it("clicking a grid tile shows that slide in single mode", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  mockSelection(true);

  root.querySelectorAll<HTMLElement>(".peitho-preview-tile")[2].click();

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(2);
});

it("dragging across a grid tile does not activate it on the follow-up click", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  mockSelection(true);

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

  expect(second.mode).toBe("single");
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
    if (url === "notes.json") return Promise.resolve(okJson(notes));
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

  resolveSync(okJson({ seq: 0, message: null, generation: 0, buildError: null }));
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

  expect(shell.mode).toBe("grid");
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

  expect(shell.mode).toBe("grid");
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
    if (url === "/sync") {
      return okJson({ seq: 7, message: null, generation: 4, buildError: null });
    }
    if (url === "manifest.json") return okJson(manifest);
    if (url === "notes.json") return okJson(notes);
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
  expect(calls[2]).toBe("notes.json");
  expect(shell.generation).toBe(4);
});

it("fetches preview slide fragments in parallel", async () => {
  const root = document.createElement("main");
  const requestedSlides: string[] = [];
  const slideResponses = new Map<string, (response: Response) => void>();
  const fetcher = vi.fn((url: string) => {
    if (url === "/sync") {
      return Promise.resolve(
        okJson({ seq: 0, message: null, generation: 0, buildError: null })
      );
    }
    if (url === "manifest.json") return Promise.resolve(okJson(manifest));
    if (url === "notes.json") return Promise.resolve(okJson(notes));
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

it("renders_an_editable_notes_textarea_with_placeholder_and_status", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: standardFetch(),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const panel = root.querySelector<HTMLElement>('[data-peitho-preview="notes"]')!;
  const position = panel.querySelector<HTMLSpanElement>('[data-peitho-preview="position"]')!;
  const status = panel.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const note = panel.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  expect(panel.hidden).toBe(false);
  expect(panel.style.display).toBe("flex");
  expect(panel.style.flexDirection).toBe("column");
  expect(position).toBeInstanceOf(HTMLSpanElement);
  expect(position.textContent).toBe("1 / 3");
  expect(position.style.flexShrink).toBe("0");
  expect(status).toBeInstanceOf(HTMLSpanElement);
  expect(status.textContent).toBe("");
  expect(status.style.marginLeft).toBe("auto");
  expect(status.style.whiteSpace).toBe("pre-wrap");
  expect(status.style.overflowWrap).toBe("anywhere");
  expect(position.parentElement?.style.display).toBe("flex");
  expect(note).toBeInstanceOf(HTMLTextAreaElement);
  expect(note.getAttribute("aria-label")).toBe("Speaker notes");
  expect(note.placeholder).toBe("No notes for this slide.");
  expect(note.value).toBe("");
  expect(note.style.background).toBe("transparent");
  expect(note.style.color).toBe("inherit");
  expect(note.style.font).toBe("inherit");
  expect(note.style.borderStyle).toBe("none");
  expect(note.style.resize).toBe("none");
  expect(note.style.flex).toBe("1 1 0%");
  expect(note.style.minHeight).toBe("0px");
  expect(note.style.width).toBe("100%");
  expect(note.style.padding).toBe("0px");

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(position.textContent).toBe("2 / 3");
  expect(root.querySelector('[data-peitho-preview="note"]')).toBe(note);
  expect(note.value).toBe("Pause here.\nThen ask.");

  // The slide is fitted above the panel: 1280x720 into 1280x(720-160).
  const host = root.querySelector<HTMLElement>('[data-slide-key="middle"] .peitho-preview-slide')!;
  const scale = (720 - PREVIEW_NOTES_HEIGHT) / 720;
  expect(host.style.transform).toContain(`scale(${scale})`);
});

it("flushes_only_dirty_notes_on_blur", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;

  note.dispatchEvent(new Event("blur"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(fixture.notesPosts()).toHaveLength(0);

  note.value = "edited";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(fixture.notesPosts()[0][0]).toBe("/notes");
  expect(fixture.notesPosts()[0][1]).toMatchObject({
    method: "POST",
    headers: { "Content-Type": "application/json" },
    keepalive: false
  });
  expect(JSON.parse(fixture.notesPosts()[0][1].body as string)).toEqual({
    key: "intro",
    text: "edited"
  });
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => {
    expect(fixture.notes.notes.intro).toBe("edited");
    expect(status.textContent).toBe("");
  });
  note.dispatchEvent(new Event("blur"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(fixture.notesPosts()).toHaveLength(1);

  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(note.value).toBe("Pause here.\nThen ask.");
  note.value = "";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  expect(JSON.parse(fixture.notesPosts()[1][1].body as string)).toEqual({
    key: "middle",
    text: ""
  });
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect("middle" in fixture.notes.notes).toBe(false));
  note.dispatchEvent(new Event("blur"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(fixture.notesPosts()).toHaveLength(2);
});

it("keeps_text_and_shows_the_server_error_when_a_save_fails", async () => {
  const root = document.createElement("main");
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const serverError = "Speaker note text cannot contain -->";

  note.value = "bad -->";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  fixture.resolveNotesPost({
    ok: false,
    status: 422,
    text: async () => JSON.stringify({ error: serverError })
  } as Response);
  await vi.waitFor(() => expect(status.textContent).toBe(serverError));
  expect(note.value).toBe("bad -->");
  expect(fixture.notes.notes.intro).toBeUndefined();

  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
});

it("shows_raw_bodies_and_thrown_fetch_errors", async () => {
  const root = document.createElement("main");
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;

  note.value = "Keep this draft.";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  fixture.resolveNotesPost({
    ok: false,
    status: 500,
    text: async () => "boom"
  } as Response);
  await vi.waitFor(() => expect(status.textContent).toBe("boom"));
  expect(note.value).toBe("Keep this draft.");
  const panel = root.querySelector<HTMLElement>('[data-peitho-preview="notes"]')!;
  expect(status.style.background).not.toBe("");
  expect(panel.style.borderTop).toBe("3px solid rgb(239, 68, 68)");

  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  expect(status.textContent).toBe("boom");
  fixture.rejectNotesPost(new TypeError("Failed to fetch"));
  await vi.waitFor(() => expect(status.textContent).toBe("Failed to fetch"));
  expect(note.value).toBe("Keep this draft.");
});

it("serializes_overlapping_flushes", async () => {
  const root = document.createElement("main");
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;

  note.value = "A";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  note.value = "AB";
  note.dispatchEvent(new Event("blur"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(fixture.notesPosts()).toHaveLength(1);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  expect(JSON.parse(fixture.notesPosts()[1][1].body as string)).toEqual({
    key: "intro",
    text: "AB"
  });
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => {
    expect(fixture.notes.notes.intro).toBe("AB");
    expect(status.textContent).toBe("");
  });
});

it("navigation_waits_for_text_typed_behind_an_in_flight_save", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "A";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  note.value = "AB";
  note.dispatchEvent(new Event("blur"));
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  expect(shell.currentIndex).toBe(0);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  expect(JSON.parse(fixture.notesPosts()[1][1].body as string)).toEqual({
    key: "intro",
    text: "AB"
  });
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => {
    expect(fixture.notes.notes.intro).toBe("AB");
    expect(shell.currentIndex).toBe(1);
  });
  expect(fixture.notesPosts()).toHaveLength(2);
});

it("flushes_before_slide_and_grid_index_changes", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const bus = new EventTarget();
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const panel = root.querySelector<HTMLElement>('[data-peitho-preview="notes"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;

  note.value = "thumbnail edit";
  note.focus();
  root.querySelectorAll<HTMLElement>(".peitho-preview-thumb")[1].click();
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));

  note.value = "navigation edit";
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  expect(shell.currentIndex).toBe(1);
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(2));

  note.value = "enter grid edit";
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(3));
  expect(shell.currentIndex).toBe(2);
  expect(shell.mode).toBe("single");

  fixture.resolveNotesPost({
    ok: false,
    status: 409,
    text: async () => JSON.stringify({ error: "deck is temporarily invalid" })
  } as Response);
  await vi.waitFor(() => expect(status.textContent).toBe("deck is temporarily invalid"));
  expect(shell.currentIndex).toBe(2);
  expect(shell.mode).toBe("single");
  expect(panel.hidden).toBe(false);
  expect(note.value).toBe("enter grid edit");
  expect(document.activeElement).toBe(note);

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(4));
  expect(shell.currentIndex).toBe(2);
  expect(shell.mode).toBe("single");
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.mode).toBe("grid"));

  // Synthetic: entering grid is gated on a settled note and the hidden textarea cannot be typed into; this only exercises the exit-to-another-slide gate.
  note.value = "grid edit";
  bus.dispatchEvent(
    new CustomEvent("peitho:navigate", { detail: { to: { index: 0 } } })
  );
  expect(shell.currentIndex).toBe(0);
  expect(shell.mode).toBe("grid");
  expect(fixture.notesPosts()).toHaveLength(4);

  mockSelection(true);
  root.querySelectorAll<HTMLElement>(".peitho-preview-tile")[0].click();
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(5));
  expect(JSON.parse(fixture.notesPosts()[4][1].body as string)).toEqual({
    key: "end",
    text: "grid edit"
  });
  expect(shell.mode).toBe("grid");
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => {
    expect(shell.currentIndex).toBe(0);
    expect(shell.mode).toBe("single");
  });
});

it("reflushes_text_typed_during_a_transition_save", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const bus = new EventTarget();
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "in flight";
  note.focus();
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);

  note.value = "typed during save";
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  expect(JSON.parse(fixture.notesPosts()[1][1].body as string)).toEqual({
    key: "intro",
    text: "typed during save"
  });
  expect(shell.currentIndex).toBe(0);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
  expect(document.activeElement).toBe(note);
});

it("transition_waits_for_every_queued_flush", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "A";
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(JSON.parse(fixture.notesPosts()[0][1].body as string)).toEqual({
    key: "intro",
    text: "A"
  });
  expect(shell.currentIndex).toBe(0);

  note.value = "AB";
  note.dispatchEvent(new Event("blur"));
  note.value = "A";
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  expect(JSON.parse(fixture.notesPosts()[1][1].body as string)).toEqual({
    key: "intro",
    text: "AB"
  });
  expect(shell.currentIndex).toBe(0);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(3));
  expect(JSON.parse(fixture.notesPosts()[2][1].body as string)).toEqual({
    key: "intro",
    text: "A"
  });
  expect(shell.currentIndex).toBe(0);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
  bus.dispatchEvent(
    new CustomEvent("peitho:navigate", { detail: { to: { index: 0 } } })
  );
  expect(shell.currentIndex).toBe(0);
  expect(note.value).toBe("A");
  expect(fixture.notes.notes.intro).toBe("A");
});

it("a_no_op_transition_does_not_cancel_a_pending_one", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "A";
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } })
  );
  root.querySelectorAll<HTMLElement>(".peitho-preview-thumb")[0].click();
  fixture.resolveNotesPost(okJson({ saved: true }));

  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
  expect(fixture.notesPosts()).toHaveLength(1);
});

it("reload_state_keeps_text_while_a_save_is_in_flight", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "A";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  note.value = "";
  note.focus();
  shell.saveState();

  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state")!)).toMatchObject({
    draft: { key: "intro", text: "", focused: true }
  });
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notes.notes.intro).toBe("A"));
});

it("pagehide_before_load_keeps_the_stored_state", async () => {
  const storedState = JSON.stringify({
    mode: "single",
    index: 1,
    draft: {
      key: "middle",
      text: "kept",
      selectionStart: 0,
      selectionEnd: 0,
      focused: true
    }
  });
  sessionStorage.setItem("peitho:preview-state", storedState);
  const root = document.createElement("main");
  const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/sync") {
      return okJson({ seq: 0, message: null, generation: 0, buildError: null });
    }
    if (url === "manifest.json") throw new Error("manifest unavailable");
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  });
  const fetcher = fetchMock as unknown as typeof fetch;
  const shell = await mountPreviewShell({
    root,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  window.dispatchEvent(new Event("pagehide"));

  expect(sessionStorage.getItem("peitho:preview-state")).toBe(storedState);
  expect(fetchMock.mock.calls.filter(([input]) => String(input) === "/notes")).toHaveLength(0);
});

it("pagehide_saves_a_draft_and_posts_with_keepalive", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 1 }));
  const shell = await mountPreviewShell({
    root,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "before pagehide";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(fixture.notesPosts()[0][1].keepalive).toBe(false);

  note.value = "reload draft";
  note.focus();
  note.setSelectionRange(3, 8);
  window.dispatchEvent(new Event("pagehide"));
  expect(fixture.notesPosts()).toHaveLength(2);
  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state")!)).toMatchObject({
    draft: {
      key: "middle",
      text: "reload draft",
      selectionStart: 3,
      selectionEnd: 8,
      focused: true
    }
  });
  expect(fixture.notesPosts().at(-1)![1].keepalive).toBe(true);
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notes.notes.middle).toBe("before pagehide"));
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notes.notes.middle).toBe("reload draft"));

  note.value = "あ".repeat(25_000);
  window.dispatchEvent(new Event("pagehide"));
  expect(fixture.notesPosts()).toHaveLength(3);
  expect(fixture.notesPosts().at(-1)![1].keepalive).toBe(false);
  fixture.resolveNotesPost(okJson({ saved: true }));
});

it("restores_and_clears_a_focused_draft_with_selection", async () => {
  const draft = {
    key: "middle",
    text: "reload draft",
    selectionStart: 3,
    selectionEnd: 8,
    focused: true
  };
  const state = { mode: "single", index: 1, draft };

  sessionStorage.setItem("peitho:preview-state", JSON.stringify(state));
  const cleanFixture = previewFetchFixture(manifest, cssText, {
    version: 1,
    notes: { middle: "reload draft" }
  });
  const cleanRoot = document.createElement("main");
  document.body.appendChild(cleanRoot);
  cleanups.push(() => cleanRoot.remove());
  const cleanShell = await mountPreviewShell({
    root: cleanRoot,
    fetcher: cleanFixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(cleanShell);
  const cleanNote = cleanRoot.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;
  expect(cleanNote.value).toBe("reload draft");
  expect(cleanNote.selectionStart).toBe(3);
  expect(cleanNote.selectionEnd).toBe(8);
  expect(document.activeElement).toBe(cleanNote);
  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state")!)).toEqual({
    mode: "single",
    index: 1
  });
  cleanNote.blur();
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(cleanFixture.notesPosts()).toHaveLength(0);
  cleanShell.destroy();
  shells.pop();
  cleanRoot.remove();

  sessionStorage.setItem("peitho:preview-state", JSON.stringify(state));
  const dirtyFixture = previewFetchFixture();
  const dirtyRoot = document.createElement("main");
  document.body.appendChild(dirtyRoot);
  cleanups.push(() => dirtyRoot.remove());
  const dirtyShell = await mountPreviewShell({
    root: dirtyRoot,
    fetcher: dirtyFixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(dirtyShell);
  const dirtyNote = dirtyRoot.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;
  expect(dirtyNote.value).toBe("reload draft");
  expect(dirtyNote.selectionStart).toBe(3);
  expect(dirtyNote.selectionEnd).toBe(8);
  expect(document.activeElement).toBe(dirtyNote);
  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state")!)).toEqual({
    mode: "single",
    index: 1
  });

  dirtyNote.blur();
  await vi.waitFor(() => expect(dirtyFixture.notesPosts()).toHaveLength(1));
  expect(JSON.parse(dirtyFixture.notesPosts()[0][1].body as string)).toEqual({
    key: "middle",
    text: "reload draft"
  });
  dirtyFixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(dirtyFixture.notes.notes.middle).toBe("reload draft"));
  await new Promise((resolve) => setTimeout(resolve, 0));

  dirtyNote.focus();
  dirtyShell.saveState();
  const settledDraft = JSON.parse(sessionStorage.getItem("peitho:preview-state")!).draft;
  expect(settledDraft).toMatchObject({ key: "middle", focused: true });
  expect(settledDraft).not.toHaveProperty("text");
});

it("clean_draft_does_not_overwrite_freshly_loaded_notes", async () => {
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({
      mode: "single",
      index: 1,
      draft: {
        key: "middle",
        selectionStart: 1,
        selectionEnd: 3,
        focused: true
      }
    })
  );
  const fixture = previewFetchFixture(manifest, cssText, {
    version: 1,
    notes: { middle: "new" }
  });
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const shell = await mountPreviewShell({
    root,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  expect(note.value).toBe("new");
  expect(note.selectionStart).toBe(1);
  expect(note.selectionEnd).toBe(3);
  expect(document.activeElement).toBe(note);
  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state")!)).toEqual({
    mode: "single",
    index: 1
  });

  note.blur();
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(fixture.notesPosts()).toHaveLength(0);
});

it("restores_an_unfocused_draft_without_focusing", async () => {
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({
      mode: "single",
      index: 1,
      draft: {
        key: "middle",
        text: "draft",
        selectionStart: 1,
        selectionEnd: 4,
        focused: false
      }
    })
  );
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const shell = await mountForTest(root, new EventTarget());
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  expect(note.value).toBe("draft");
  expect(note.selectionStart).toBe(1);
  expect(note.selectionEnd).toBe(4);
  expect(document.activeElement).not.toBe(note);
});

it("drops_a_draft_whose_key_is_gone", async () => {
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({
      mode: "single",
      index: 1,
      draft: {
        key: "gone",
        text: "x",
        selectionStart: 0,
        selectionEnd: 0,
        focused: true
      }
    })
  );
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const shell = await mountForTest(root, new EventTarget());
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const restoredState = JSON.parse(sessionStorage.getItem("peitho:preview-state")!);

  expect(note.value).toBe("Pause here.\nThen ask.");
  expect(document.activeElement).not.toBe(note);
  expect(restoredState).toEqual({ mode: "single", index: 1 });
  expect(restoredState).not.toHaveProperty("draft");
});

it("ignores_a_malformed_draft_without_discarding_valid_preview_state", async () => {
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({
      mode: "single",
      index: 2,
      draft: {
        key: "end",
        text: "malformed",
        selectionStart: -1,
        selectionEnd: 4,
        focused: true
      }
    })
  );
  const root = document.createElement("main");
  const shell = await mountForTest(root, new EventTarget());
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(2);
  expect(note.value).toBe("");
});

it("navigating_away_and_back_during_an_in_flight_save_does_not_revert_it", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "A";
  bus.dispatchEvent(
    new CustomEvent("peitho:navigate", { detail: { to: { index: 1 } } })
  );
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);

  note.value = "";
  bus.dispatchEvent(
    new CustomEvent("peitho:navigate", { detail: { to: { index: 1 } } })
  );
  expect(shell.currentIndex).toBe(0);
  expect(fixture.notesPosts()).toHaveLength(1);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  expect(JSON.parse(fixture.notesPosts()[1][1].body as string)).toEqual({
    key: "intro",
    text: ""
  });
  expect(shell.currentIndex).toBe(0);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
  bus.dispatchEvent(
    new CustomEvent("peitho:navigate", { detail: { to: { index: 0 } } })
  );

  expect(shell.currentIndex).toBe(0);
  expect(note.value).toBe("");
  note.dispatchEvent(new Event("blur"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(fixture.notesPosts()).toHaveLength(2);
});

it("destroy_cancels_a_pending_transition_commit", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "A";
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);
  sessionStorage.removeItem("peitho:preview-state");

  shell.destroy();
  shells.pop();
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notes.notes.intro).toBe("A"));

  expect(shell.currentIndex).toBe(0);
  expect(sessionStorage.getItem("peitho:preview-state")).toBeNull();
});

it("resize_does_not_overwrite_a_dirty_note", async () => {
  const root = document.createElement("main");
  const fixture = previewFetchFixture();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 1 }));
  const shell = await mountPreviewShell({
    root,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  expect(note.value).toBe("Pause here.\nThen ask.");
  note.value = "Keep this draft.";
  window.dispatchEvent(new Event("resize"));

  expect(note.value).toBe("Keep this draft.");
  expect(fixture.notesPosts()).toHaveLength(0);
});

it("hides the speaker notes panel in grid mode", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: standardFetch(),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const panel = root.querySelector<HTMLElement>('[data-peitho-preview="notes"]')!;
  expect(shell.mode).toBe("grid");
  expect(panel.hidden).toBe(true);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  expect(panel.hidden).toBe(false);
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } }));
  expect(panel.hidden).toBe(true);
});

it("shows a filmstrip of every slide beside the stage in single mode", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: standardFetch(),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const strip = root.querySelector<HTMLElement>('[data-peitho-preview="strip"]')!;
  expect(strip.hidden).toBe(false);
  const thumbs = Array.from(strip.querySelectorAll<HTMLElement>(".peitho-preview-thumb"));
  expect(thumbs.map((thumb) => thumb.dataset.slideKey)).toEqual(manifest.slides.map((s) => s.key));
  expect(thumbs.map((thumb) => thumb.getAttribute("aria-current"))).toEqual([
    "true",
    "false",
    "false"
  ]);
  // Every thumbnail renders the slide in its own shadow root, scaled to the strip.
  const thumbHost = thumbs[1].querySelector<HTMLElement>(".peitho-preview-thumb-slide")!;
  expect(thumbHost.shadowRoot?.querySelector("h1")?.textContent).toBe("slides/001-middle.html");
  expect(thumbHost.style.pointerEvents).toBe("none");

  // The stage and notes sit to the right of the strip.
  const tile = root.querySelector<HTMLElement>('.peitho-preview-tile[data-slide-key="intro"]')!;
  expect(tile.style.left).toBe(`${PREVIEW_STRIP_WIDTH}px`);
  const panel = root.querySelector<HTMLElement>('[data-peitho-preview="notes"]')!;
  expect(panel.style.left).toBe(`${PREVIEW_STRIP_WIDTH}px`);

  const changes: number[] = [];
  bus.addEventListener("peitho:slidechange", (event) => {
    changes.push((event as CustomEvent<{ index: number }>).detail.index);
  });
  thumbs[2].click();
  expect(shell.mode).toBe("single");
  expect(shell.currentIndex).toBe(2);
  expect(changes).toEqual([2]);
  expect(thumbs.map((thumb) => thumb.classList.contains("is-selected"))).toEqual([
    false,
    false,
    true
  ]);
});

it("numbers every thumbnail and grid tile, and hides the stage number in single mode", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: standardFetch(),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const numbersIn = (selector: string) =>
    Array.from(root.querySelectorAll<HTMLElement>(`${selector} .peitho-preview-number`));
  expect(shell.mode).toBe("grid");
  expect(numbersIn(".peitho-preview-tile").map((n) => n.textContent)).toEqual(["1", "2", "3"]);
  expect(numbersIn(".peitho-preview-tile").every((n) => !n.hidden)).toBe(true);

  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  expect(numbersIn(".peitho-preview-thumb").map((n) => n.textContent)).toEqual(["1", "2", "3"]);
  expect(numbersIn(".peitho-preview-tile").every((n) => n.hidden)).toBe(true);
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } }));
  expect(numbersIn(".peitho-preview-tile").every((n) => !n.hidden)).toBe(true);
});

it("hides the filmstrip in grid mode", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: standardFetch(),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const strip = root.querySelector<HTMLElement>('[data-peitho-preview="strip"]')!;
  expect(shell.mode).toBe("grid");
  expect(strip.hidden).toBe(true);
  expect(strip.style.display).toBe("");
  bus.dispatchEvent(new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } }));
  expect(strip.hidden).toBe(false);
  expect(strip.style.display).toBe("flex");
});

it("walks the filmstrip with ArrowUp/ArrowDown in single mode, skipping skipped slides", async () => {
  const root = document.createElement("main");
  const bus = new EventTarget();
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 0 }));
  const deck = manifestWithSlides([{ key: "a" }, { key: "b", skip: true }, { key: "c" }]);
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const down = new CustomEvent("peitho:navigate", { cancelable: true, detail: { to: "down" } });
  bus.dispatchEvent(down);
  expect(down.defaultPrevented).toBe(true);
  expect(shell.currentIndex).toBe(2);
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "up" } }));
  expect(shell.currentIndex).toBe(0);
  const atStart = new CustomEvent("peitho:navigate", { cancelable: true, detail: { to: "up" } });
  bus.dispatchEvent(atStart);
  expect(atStart.defaultPrevented).toBe(false);
  expect(shell.currentIndex).toBe(0);
});

it("marks the document ready only once the root has content, so a reload holds the previous frame", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  delete document.documentElement.dataset.peithoReady;

  let readyWhileEmpty: boolean | null = null;
  const fetcher = standardFetch();
  const watchingFetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    // Every fetch of the mount happens before content lands; the ground must stay unpainted.
    if (readyWhileEmpty !== true) {
      readyWhileEmpty =
        document.documentElement.dataset.peithoReady !== undefined && root.children.length === 0;
    }
    return fetcher(input, init);
  }) as unknown as typeof fetch;

  const shell = await mountPreviewShell({
    root,
    bus: new EventTarget(),
    fetcher: watchingFetch,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  expect(readyWhileEmpty).toBe(false);
  expect(root.children.length).toBeGreaterThan(0);
  expect(document.documentElement.dataset.peithoReady).toBe("");
  expect(root.style.background).toBe("rgb(0, 0, 0)");

  root.remove();
  delete document.documentElement.dataset.peithoReady;
});

it("paints the ground on the error path so a failed mount is not invisible", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  delete document.documentElement.dataset.peithoReady;

  const shell = await mountPreviewShell({
    root,
    bus: new EventTarget(),
    fetcher: (async () => {
      throw new Error("deck is broken");
    }) as unknown as typeof fetch,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  expect(root.textContent).toContain("deck is broken");
  expect(document.documentElement.dataset.peithoReady).toBe("");

  root.remove();
  delete document.documentElement.dataset.peithoReady;
});

it("centres the restored slide on first layout, then steps with nearest", async () => {
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode: "single", index: 2 }));
  const calls: Array<ScrollIntoViewOptions | boolean | undefined> = [];
  const original = HTMLElement.prototype.scrollIntoView;
  HTMLElement.prototype.scrollIntoView = function (options) {
    calls.push(options);
  };

  try {
    const bus = new EventTarget();
    const root = document.createElement("main");
    const shell = await mountForTest(root, bus);

    // The strip starts at scrollTop 0 after a reload, so `nearest` would pin the restored
    // slide to the bottom edge instead of where it sat before.
    expect(calls).toEqual([{ block: "center" }]);
    expect(shell.currentIndex).toBe(2);

    calls.length = 0;
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
    expect(calls).toEqual([{ block: "nearest" }]);
  } finally {
    HTMLElement.prototype.scrollIntoView = original;
  }
});
