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
import { calculateCanvasFit } from "../src/canvas";
import { resetKeepaliveBudgetForTests } from "../src/previewHttp";
import { SHADOW_MOUNTED_EVENT, type ShadowMountedDetail } from "../src/scripts";
import type { Notes } from "../../../bindings/Notes";
import type { SlideSources } from "../../../bindings/SlideSources";
import type { SyncChannel } from "../src/sync";

type WindowWithShadowMountedBacklog = Window & {
  __peithoShadowRoots?: unknown;
};

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function errorJson(status: number, message: string): Response {
  return {
    ok: false,
    status,
    text: async () => JSON.stringify({ error: message })
  } as Response;
}

function press(
  target: EventTarget,
  key: string,
  init: KeyboardEventInit = {}
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    composed: true,
    cancelable: true,
    ...init,
    key
  });
  target.dispatchEvent(event);
  return event;
}

function openSourceEditor(root: HTMLElement, bus: EventTarget): HTMLTextAreaElement {
  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  const editor = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="source"]'
  );
  if (editor === null) throw new Error("Missing preview source editor");
  return editor;
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
const slideSources: SlideSources = {
  version: 1,
  sources: { intro: "# Intro", middle: "# Middle", end: "# End" },
  unavailable: {}
};
const cssText = ".slot-title { color: red; }";
const EDIT_AFFORDANCE_TEXT = "Click text to edit · e for Markdown · Enter for notes";
const EDIT_AFFORDANCE_WITHOUT_SOURCE_TEXT = "Click text to edit · Enter for notes";
const SOURCE_EDIT_HINT_TEXT =
  "Cmd/Ctrl+Enter or click away saves · Enter inserts a newline · Esc cancels";
const INLINE_EDIT_HINT_TEXT =
  "Enter or click away saves · Shift+Enter inserts a newline · Esc cancels";
const NOTES_EDIT_HINT_TEXT = "Esc or click away saves · Enter inserts a newline";
const SAVING_HINT_TEXT = "Saving…";
const RESTORE_OFFER_TEXT = "Draft discarded · Press u to restore";
const KEY_HINT_COLOR = "rgb(203, 213, 225)";
const keyTokens = (hint: HTMLSpanElement): string[] =>
  Array.from(hint.children)
    .filter((child) => (child as HTMLSpanElement).style.color === KEY_HINT_COLOR)
    .map((child) => child.textContent ?? "");
// Opaque text installed verbatim by the shell; Rust owns the contents of fontscope.css.
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
  sources: SlideSources;
  notesPosts(): Array<[string, RequestInit]>;
  resolveNotesPost(response: Response): void;
  rejectNotesPost(error: unknown): void;
};

function previewFetchFixture(
  deck: typeof manifest = manifest,
  css = cssText,
  sourceNotes: Notes = notes,
  sourceSlideSources: SlideSources = slideSources,
  fontCss = ""
): PreviewFetchFixture {
  // The shell receives these same objects, so map assertions observe their updates.
  const loadedNotes: Notes = { version: sourceNotes.version, notes: { ...sourceNotes.notes } };
  const loadedSources: SlideSources = {
    version: sourceSlideSources.version,
    sources: { ...sourceSlideSources.sources },
    unavailable: { ...sourceSlideSources.unavailable }
  };
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
    if (url === "sources.json") return okJson(loadedSources);
    if (url === "peitho.css") return okText(css);
    if (url === "fontscope.css") return okText(fontCss);
    if (url.startsWith("slides/")) return okText(`<section><h1>${url}</h1></section>`);
    return { ok: false, status: 404, text: async () => "not found" } as Response;
  }) as unknown as typeof fetch;
  return {
    fetcher,
    notes: loadedNotes,
    sources: loadedSources,
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

const inlineEditSlideHtml: Record<string, string> = {
  "slides/000-intro.html": `
    <section>
      <h1><span class="slot-title"><span id="editable-heading" data-peitho-src="40-67" data-peitho-md="A &quot;quote&quot; &amp; **mark**">A &quot;quote&quot; &amp; <strong>mark</strong></span></span></h1>
      <p id="editable-paragraph" data-peitho-src="120-143" data-peitho-md="Peitho is a *fast* tool">Peitho is a <em id="paragraph-emphasis">fast</em> tool</p>
      <ul><li id="editable-tight-item" data-peitho-src="200-212" data-peitho-md="parent *one*">parent <em id="tight-emphasis">one</em><ul id="nested-list"><li>child</li></ul></li></ul>
      <p id="editable-link" data-peitho-src="240-279" data-peitho-md="[Open docs](https://example.com)"><a id="external-link" href="https://example.com" target="_blank">Open docs</a></p>
      <p id="editable-crlf" data-peitho-src="300-313" data-peitho-md="first&#13;&#10;second">first<br>second</p>
    </section>
  `,
  "slides/001-middle.html": `
    <section><p id="middle-editable" data-peitho-src="400-406" data-peitho-md="Middle">Middle</p></section>
  `,
  "slides/002-end.html": `
    <section><p id="end-editable" data-peitho-src="500-503" data-peitho-md="End">End</p></section>
  `
};

type InlineEditFetchFixture = PreviewFetchFixture & {
  slideEditPosts(): Array<[string, RequestInit]>;
  resolveSlideEditPost(response: Response): void;
  rejectSlideEditPost(error: unknown): void;
};

function inlineEditFetchFixture(
  sourceSlideSources: SlideSources = slideSources,
  sourceNotes: Notes = notes
): InlineEditFetchFixture {
  const base = previewFetchFixture(manifest, cssText, sourceNotes, sourceSlideSources);
  const posts: Array<[string, RequestInit]> = [];
  const settlers: Array<{
    resolve(response: Response): void;
    reject(error: unknown): void;
  }> = [];
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/slide-edit") {
      posts.push([url, init ?? {}]);
      return new Promise<Response>((resolve, reject) => settlers.push({ resolve, reject }));
    }
    const slideHtml = inlineEditSlideHtml[url];
    if (slideHtml !== undefined) return okText(slideHtml);
    return base.fetcher(input, init);
  }) as unknown as typeof fetch;
  return {
    ...base,
    fetcher,
    slideEditPosts: () => posts,
    resolveSlideEditPost(response: Response): void {
      const settler = settlers.shift();
      if (settler === undefined) throw new Error("No pending /slide-edit request");
      settler.resolve(response);
    },
    rejectSlideEditPost(error: unknown): void {
      const settler = settlers.shift();
      if (settler === undefined) throw new Error("No pending /slide-edit request");
      settler.reject(error);
    }
  };
}

type SourceEditFetchFixture = InlineEditFetchFixture & {
  sourceEditPosts(): Array<[string, RequestInit]>;
  resolveSourceEditPost(response: Response): void;
  rejectSourceEditPost(error: unknown): void;
};

function sourceEditFetchFixture(
  sourceSlideSources: SlideSources = slideSources,
  sourceNotes: Notes = notes
): SourceEditFetchFixture {
  const base = inlineEditFetchFixture(sourceSlideSources, sourceNotes);
  const posts: Array<[string, RequestInit]> = [];
  const settlers: Array<{
    resolve(response: Response): void;
    reject(error: unknown): void;
  }> = [];
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/slide-source") {
      posts.push([url, init ?? {}]);
      return new Promise<Response>((resolve, reject) => settlers.push({ resolve, reject }));
    }
    return base.fetcher(input, init);
  }) as unknown as typeof fetch;
  return {
    ...base,
    fetcher,
    sourceEditPosts: () => posts,
    resolveSourceEditPost(response: Response): void {
      const settler = settlers.shift();
      if (settler === undefined) throw new Error("No pending /slide-source request");
      settler.resolve(response);
    },
    rejectSourceEditPost(error: unknown): void {
      const settler = settlers.shift();
      if (settler === undefined) throw new Error("No pending /slide-source request");
      settler.reject(error);
    }
  };
}

function fetchForManifest(deck: typeof manifest, css = cssText, fontCss = ""): typeof fetch {
  return previewFetchFixture(deck, css, notes, slideSources, fontCss).fetcher;
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
const testWindow = window as WindowWithShadowMountedBacklog;

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  while (shells.length > 0) shells.pop()?.destroy();
  resetKeepaliveBudgetForTests();
  sessionStorage.clear();
  delete testWindow.__peithoShadowRoots;
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

async function mountInlineEditForTest(options: {
  mode?: "single" | "grid";
  index?: number;
  bus?: EventTarget;
  fixture?: InlineEditFetchFixture;
  viewport?: () => { width: number; height: number };
  selectionRangeProvider?: (editor: HTMLElement) => {
    range: Range;
    select(range: Range): void;
  } | null;
} = {}): Promise<{
  root: HTMLElement;
  shell: PreviewShell;
  fixture: InlineEditFetchFixture;
  bus: EventTarget;
}> {
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const mode = options.mode ?? "single";
  const index = options.index ?? 0;
  sessionStorage.setItem("peitho:preview-state", JSON.stringify({ mode, index }));
  const fixture = options.fixture ?? inlineEditFetchFixture();
  const bus = options.bus ?? window;
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fixture.fetcher,
    window,
    storage: sessionStorage,
    selectionRangeProvider: options.selectionRangeProvider,
    viewport: options.viewport ?? (() => ({ width: 1280, height: 720 }))
  });
  shells.push(shell);
  return { root, shell, fixture, bus };
}

function slideShadow(root: HTMLElement, key: string, thumbnail = false): ShadowRoot {
  const className = thumbnail ? "peitho-preview-thumb-slide" : "peitho-preview-slide";
  const host = root.querySelector<HTMLElement>(`.${className}[data-slide-key="${key}"]`);
  if (host?.shadowRoot === null || host?.shadowRoot === undefined) {
    throw new Error(`Missing ${className} shadow root for ${key}`);
  }
  return host.shadowRoot;
}

function dispatchShadowClick(target: Element): MouseEvent {
  const event = new MouseEvent("click", {
    bubbles: true,
    composed: true,
    cancelable: true
  });
  target.dispatchEvent(event);
  return event;
}

function injectedEditorRange(start: number | "end", end = start): {
  provider(editor: HTMLElement): { range: Range; select(range: Range): void };
  selected(): Range | null;
} {
  let selected: Range | null = null;
  return {
    provider(editor: HTMLElement) {
      const text = editor.firstChild;
      if (!(text instanceof Text)) throw new Error("Expected one editor text node");
      const range = editor.ownerDocument.createRange();
      const startOffset = start === "end" ? text.length : start;
      const endOffset = end === "end" ? text.length : end;
      range.setStart(text, startOffset);
      range.setEnd(text, endOffset);
      return {
        range,
        select(nextRange: Range): void {
          selected = nextRange.cloneRange();
        }
      };
    },
    selected: () => selected
  };
}

function expectSameNodes(actual: NodeListOf<ChildNode>, expected: Node[]): void {
  expect(actual).toHaveLength(expected.length);
  Array.from(actual).forEach((node, index) => expect(node).toBe(expected[index]));
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

it("resets inherited color scheme before deck css for stage and thumbnail hosts", async () => {
  const root = document.createElement("main");
  await mountForTest(root);

  for (const shadow of [
    slideShadow(root, "intro"),
    slideShadow(root, "intro", true)
  ]) {
    const styles = Array.from(shadow.querySelectorAll("style"));
    const resetIndex = styles.findIndex((style) =>
      style.textContent?.includes(":host{color-scheme:light !important}")
    );
    const deckCssIndex = styles.findIndex((style) => style.textContent === cssText);

    expect(resetIndex).toBeGreaterThanOrEqual(0);
    expect(deckCssIndex).toBeGreaterThan(resetIndex);
  }
});

// jsdom never executes scripts because vitest does not opt into runScripts: "dangerously";
// this test is structural, while real execution is covered by the plan's real-Chrome checklist.
it("layout_script_runs_on_the_stage_but_never_in_a_thumbnail", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const fixture = previewFetchFixture();
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input) === "slides/000-intro.html") {
      return okText("<section><script>let n = 0</script></section>");
    }
    return fixture.fetcher(input, init);
  }) as unknown as typeof fetch;

  const shell = await mountPreviewShell({
    root,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  const stageScripts = slideShadow(root, "intro").querySelectorAll("script");
  expect(stageScripts).toHaveLength(1);
  expect(stageScripts[0]?.textContent).toBe("(function () {\nlet n = 0\n})();");

  const thumbnailScripts = slideShadow(root, "intro", true).querySelectorAll("script");
  expect(thumbnailScripts).toHaveLength(1);
  expect(thumbnailScripts[0]?.textContent).toBe("let n = 0");
});

// jsdom coverage is structural; real behaviour is covered by the real-Chrome checklist.
it("shadow_mounted_fires_for_the_stage_only", async () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0 })
  );

  let backlogAtFirstTileAppend: unknown;
  let backlogLengthAtFirstTileAppend: number | null = null;
  const appendChild = root.appendChild.bind(root);
  vi.spyOn(root, "appendChild").mockImplementation(<T extends Node>(node: T): T => {
    if (
      backlogLengthAtFirstTileAppend === null &&
      node instanceof HTMLElement &&
      node.classList.contains("peitho-preview-tile")
    ) {
      backlogAtFirstTileAppend = testWindow.__peithoShadowRoots;
      backlogLengthAtFirstTileAppend = Array.isArray(backlogAtFirstTileAppend)
        ? backlogAtFirstTileAppend.length
        : -1;
    }
    return appendChild(node) as T;
  });

  const events: CustomEvent<ShadowMountedDetail>[] = [];
  let visibilityAtFirstAnnouncement: boolean[] | null = null;
  let visibilityAfterSynchronousNavigation: boolean[] | null = null;
  const listener: EventListener = (event) => {
    if (events.length === 0) {
      const stageHosts = Array.from(
        root.querySelectorAll<HTMLElement>(".peitho-preview-slide")
      );
      visibilityAtFirstAnnouncement = stageHosts.map((host) => Boolean(host.hidden));
      window.dispatchEvent(
        new CustomEvent("peitho:navigate", { detail: { to: "last" } })
      );
      visibilityAfterSynchronousNavigation = stageHosts.map((host) =>
        Boolean(host.hidden)
      );
      window.dispatchEvent(
        new CustomEvent("peitho:navigate", { detail: { to: "first" } })
      );
    }
    events.push(event as CustomEvent<ShadowMountedDetail>);
  };
  document.addEventListener(SHADOW_MOUNTED_EVENT, listener);
  cleanups.push(() => document.removeEventListener(SHADOW_MOUNTED_EVENT, listener));

  const shell = await mountPreviewShell({
    root,
    fetcher: standardFetch(),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  const stageHosts = Array.from(
    root.querySelectorAll<HTMLElement>(".peitho-preview-slide")
  );
  const thumbHosts = Array.from(
    root.querySelectorAll<HTMLElement>(".peitho-preview-thumb-slide")
  );
  const thumbRoots = new Set<Element | ShadowRoot | null>(
    thumbHosts.map((host) => host.shadowRoot)
  );

  expect(Array.isArray(backlogAtFirstTileAppend)).toBe(true);
  expect(backlogLengthAtFirstTileAppend).toBe(0);
  expect(stageHosts).toHaveLength(manifest.slides.length);
  expect(thumbHosts).toHaveLength(manifest.slides.length);
  expect(events).toHaveLength(stageHosts.length);
  expect(events.map((event) => event.detail.key)).toEqual(
    manifest.slides.map((slide) => slide.key)
  );
  expect(visibilityAtFirstAnnouncement).toEqual([false, true, true]);
  expect(visibilityAfterSynchronousNavigation).toEqual([true, true, false]);

  for (const [index, host] of stageHosts.entries()) {
    const event = events[index];
    expect(event.target).toBe(host);
    expect(event.bubbles).toBe(true);
    expect(event.composed).toBe(true);
    expect(event.detail.root).toBe(host.shadowRoot);
    expect(thumbRoots.has(event.detail.root)).toBe(false);
    expect(event.detail.key).toBe(host.dataset.slideKey);
    expect(event.detail.index).toBe(Number(host.dataset.slideIndex));
  }

  const backlog = testWindow.__peithoShadowRoots;
  expect(backlog).toBe(backlogAtFirstTileAppend);
  expect(backlog).toHaveLength(events.length);
  for (const [index, event] of events.entries()) {
    expect((backlog as ShadowMountedDetail[])[index]).toBe(event.detail);
  }
});

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

it("preview_keyboard_only_emits_source_edit_request", () => {
  const bus = new EventTarget();
  const requests: Event[] = [];
  const unrelatedRequest = vi.fn();
  const onSourceEditRequest = (event: Event): void => {
    requests.push(event);
  };
  bus.addEventListener("peitho:sourceeditrequest", onSourceEditRequest);
  bus.addEventListener("peitho:navigate", unrelatedRequest);
  bus.addEventListener("peitho:overviewrequest", unrelatedRequest);
  cleanups.push(() => {
    bus.removeEventListener("peitho:sourceeditrequest", onSourceEditRequest);
    bus.removeEventListener("peitho:navigate", unrelatedRequest);
    bus.removeEventListener("peitho:overviewrequest", unrelatedRequest);
  });
  cleanups.push(installPreviewKeyboard(window, bus));

  const notesTextarea = document.createElement("textarea");
  notesTextarea.dataset.peithoPreview = "note";
  const input = document.createElement("input");
  const select = document.createElement("select");
  const contenteditableRoot = document.createElement("div");
  contenteditableRoot.setAttribute("contenteditable", "true");
  const contenteditableChild = document.createElement("span");
  contenteditableRoot.appendChild(contenteditableChild);
  const inlineHost = document.createElement("div");
  const inlineEditor = document.createElement("span");
  inlineEditor.setAttribute("contenteditable", "plaintext-only");
  inlineHost.attachShadow({ mode: "open" }).appendChild(inlineEditor);
  document.body.append(notesTextarea, input, select, contenteditableRoot, inlineHost);
  cleanups.push(() => {
    notesTextarea.remove();
    input.remove();
    select.remove();
    contenteditableRoot.remove();
    inlineHost.remove();
  });

  const fetchSpy = vi.spyOn(globalThis, "fetch");
  const bodyMarkup = document.body.innerHTML;
  const inlineMarkup = inlineHost.shadowRoot?.innerHTML;
  const bare = new KeyboardEvent("keydown", { key: "e", cancelable: true });
  window.dispatchEvent(bare);

  const suppressed = [
    new KeyboardEvent("keydown", { key: "E", cancelable: true }),
    new KeyboardEvent("keydown", { key: "e", shiftKey: true, cancelable: true }),
    new KeyboardEvent("keydown", { key: "e", metaKey: true, cancelable: true }),
    new KeyboardEvent("keydown", { key: "e", ctrlKey: true, cancelable: true }),
    new KeyboardEvent("keydown", { key: "e", altKey: true, cancelable: true }),
    new KeyboardEvent("keydown", { key: "e", isComposing: true, cancelable: true })
  ];
  const safariComposition = new KeyboardEvent("keydown", { key: "e", cancelable: true });
  Object.defineProperty(safariComposition, "keyCode", { value: 229 });
  suppressed.push(safariComposition);
  for (const event of suppressed) window.dispatchEvent(event);

  const editableEvents = [notesTextarea, input, select, contenteditableChild, inlineEditor].map(
    (target) => {
      const event = new KeyboardEvent("keydown", {
        key: "e",
        bubbles: true,
        composed: true,
        cancelable: true
      });
      target.dispatchEvent(event);
      return event;
    }
  );

  expect(bare.defaultPrevented).toBe(true);
  expect(requests).toHaveLength(1);
  expect(requests[0]).toBeInstanceOf(CustomEvent);
  expect((requests[0] as CustomEvent).detail).toBeNull();
  expect([...suppressed, ...editableEvents].every((event) => !event.defaultPrevented)).toBe(true);
  expect(unrelatedRequest).not.toHaveBeenCalled();
  expect(fetchSpy).not.toHaveBeenCalled();
  expect(document.body.innerHTML).toBe(bodyMarkup);
  expect(inlineHost.shadowRoot?.innerHTML).toBe(inlineMarkup);
});

it("preview_keyboard_only_emits_cancelable_restore_requests_for_plain_u", () => {
  const bus = new EventTarget();
  const requests: Event[] = [];
  let accept = false;
  const onRestoreRequest = (event: Event): void => {
    requests.push(event);
    if (accept) event.preventDefault();
  };
  bus.addEventListener("peitho:restorerequest", onRestoreRequest);
  cleanups.push(() => bus.removeEventListener("peitho:restorerequest", onRestoreRequest));
  cleanups.push(installPreviewKeyboard(window, bus));

  const noOffer = press(window, "u");
  expect(noOffer.defaultPrevented).toBe(false);
  expect(requests).toHaveLength(1);

  accept = true;
  const accepted = press(window, "u");
  expect(accepted.defaultPrevented).toBe(true);
  expect(requests).toHaveLength(2);

  for (const modifier of [{ metaKey: true }, { ctrlKey: true }, { altKey: true }]) {
    const chord = press(window, "u", modifier);
    expect(chord.defaultPrevented).toBe(false);
  }
  expect(requests).toHaveLength(2);

  const textarea = document.createElement("textarea");
  textarea.value = "note";
  document.body.appendChild(textarea);
  cleanups.push(() => textarea.remove());
  const typed = press(textarea, "u");
  if (!typed.defaultPrevented) textarea.value += "u";
  expect(typed.defaultPrevented).toBe(false);
  expect(textarea.value).toBe("noteu");
  expect(requests).toHaveLength(2);
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

it("preview keyboard leaves Enter on links in a slide shadow root untouched", () => {
  const bus = new EventTarget();
  const overviewRequests: unknown[] = [];
  bus.addEventListener("peitho:overviewrequest", (event) =>
    overviewRequests.push((event as CustomEvent).detail)
  );
  cleanups.push(installPreviewKeyboard(window, bus));
  const host = document.createElement("div");
  host.dataset.slideKey = "intro";
  const link = document.createElement("a");
  link.href = "#x";
  host.attachShadow({ mode: "open" }).appendChild(link);
  document.body.appendChild(host);
  cleanups.push(() => host.remove());
  link.focus();

  const enter = new KeyboardEvent("keydown", {
    key: "Enter",
    bubbles: true,
    composed: true,
    cancelable: true
  });
  link.dispatchEvent(enter);

  expect(overviewRequests).toEqual([]);
  expect(enter.defaultPrevented).toBe(false);
});

it("preview keyboard leaves Enter on a shadow-root button untouched", () => {
  const bus = new EventTarget();
  const overviewRequests: unknown[] = [];
  bus.addEventListener("peitho:overviewrequest", (event) =>
    overviewRequests.push((event as CustomEvent).detail)
  );
  cleanups.push(installPreviewKeyboard(window, bus));
  const host = document.createElement("div");
  host.dataset.slideKey = "intro";
  const button = document.createElement("button");
  host.attachShadow({ mode: "open" }).appendChild(button);
  document.body.appendChild(host);
  cleanups.push(() => host.remove());

  const enter = press(button, "Enter");

  expect(enter.defaultPrevented).toBe(false);
  expect(overviewRequests).toEqual([]);
});

it("preview keyboard handles Enter from a focused light-DOM chrome button", () => {
  const bus = new EventTarget();
  const overviewRequests: unknown[] = [];
  bus.addEventListener("peitho:overviewrequest", (event) =>
    overviewRequests.push((event as CustomEvent).detail)
  );
  cleanups.push(installPreviewKeyboard(window, bus));
  const button = document.createElement("button");
  document.body.appendChild(button);
  cleanups.push(() => button.remove());

  const enter = press(button, "Enter");

  expect(enter.defaultPrevented).toBe(true);
  expect(overviewRequests).toEqual([{ action: "activate" }]);
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
    fetcher: fetchForManifest(manifest, cssText, fontCssText),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  const second = await mountPreviewShell({
    root: secondRoot,
    fetcher: fetchForManifest(manifest, cssText, fontCssText),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(first, second);

  const styles = document.head.querySelectorAll<HTMLStyleElement>(
    "style[data-peitho-font-scope]"
  );
  expect(styles).toHaveLength(1);
  expect(styles[0].textContent).toBe(fontCssText);
});

it("removes document scoped font css when the last preview shell is destroyed", async () => {
  const firstRoot = document.createElement("main");
  const secondRoot = document.createElement("main");
  const first = await mountPreviewShell({
    root: firstRoot,
    fetcher: fetchForManifest(manifest, cssText, fontCssText),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  const second = await mountPreviewShell({
    root: secondRoot,
    fetcher: fetchForManifest(manifest, cssText, fontCssText),
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

it("clicking a layout button in a shadow-root grid tile does not open the slide", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  const fixture = previewFetchFixture();
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input) === "slides/002-end.html") {
      return okText('<section><button id="layout-button">Run</button></section>');
    }
    return fixture.fetcher(input, init);
  }) as unknown as typeof fetch;
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  mockSelection(true);

  const button = slideShadow(root, "end").querySelector<HTMLButtonElement>("#layout-button")!;
  dispatchShadowClick(button);

  expect(shell.mode).toBe("grid");
  expect(shell.currentIndex).toBe(0);
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
    if (url === "sources.json") return Promise.resolve(okJson(slideSources));
    if (url === "peitho.css") return Promise.resolve(okText(cssText));
    if (url === "fontscope.css") return Promise.resolve(okText(""));
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
    if (url === "sources.json") return okJson(slideSources);
    if (url === "peitho.css") return okText(cssText);
    if (url === "fontscope.css") return okText("");
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

  expect(calls).toEqual([
    "/sync",
    "manifest.json",
    "notes.json",
    "sources.json",
    "peitho.css",
    "fontscope.css",
    "slides/000-intro.html",
    "slides/001-middle.html",
    "slides/002-end.html"
  ]);
  expect(shell.generation).toBe(4);

  for (const testCase of [
    {
      response: { ok: false, status: 503, text: async () => "unavailable" } as Response,
      message: "Failed to load sources.json: 503"
    },
    {
      response: okJson({ version: 1, sources: { intro: 42 }, unavailable: {} }),
      message: "Invalid sources.json"
    },
    {
      response: okJson({ version: 1 }),
      message: "Invalid sources.json"
    },
    {
      response: okJson({ version: 1, sources: {} }),
      message: "Invalid sources.json"
    },
    {
      response: okJson(null),
      message: "Invalid sources.json"
    },
    {
      response: okJson({ version: 1, sources: [], unavailable: [] }),
      message: "Invalid sources.json"
    },
    {
      response: okJson({ version: "1", sources: {}, unavailable: {} }),
      message: "Invalid sources.json"
    }
  ]) {
    const failedRoot = document.createElement("main");
    const failedFetcher = vi.fn(async (url: string) => {
      if (url === "/sync") {
        return okJson({ seq: 7, message: null, generation: 4, buildError: null });
      }
      if (url === "manifest.json") return okJson(manifest);
      if (url === "notes.json") return okJson(notes);
      if (url === "sources.json") return testCase.response;
      throw new Error(`unexpected ${url}`);
    }) as typeof fetch;
    const failedShell = await mountPreviewShell({
      root: failedRoot,
      fetcher: failedFetcher,
      window,
      storage: sessionStorage,
      viewport: () => ({ width: 1280, height: 720 })
    });
    shells.push(failedShell);

    expect(failedShell.manifest).toBeNull();
    expect(failedRoot.textContent).toContain(testCase.message);
    expect(failedRoot.querySelectorAll(".peitho-preview-slide")).toHaveLength(0);
  }
});

it("shows a visible error when fontscope css fetch fails", async () => {
  const root = document.createElement("main");
  const fetcher = vi.fn(async (url: string) => {
    if (url === "/sync") {
      return okJson({ seq: 7, message: null, generation: 4, buildError: null });
    }
    if (url === "manifest.json") return okJson(manifest);
    if (url === "notes.json") return okJson(notes);
    if (url === "sources.json") return okJson(slideSources);
    if (url === "peitho.css") return okText(cssText);
    if (url === "fontscope.css") {
      return { ok: false, status: 404, text: async () => "" } as Response;
    }
    if (url === "slides/000-intro.html") return okText("<section>Intro</section>");
    if (url === "slides/001-middle.html") return okText("<section>Middle</section>");
    if (url === "slides/002-end.html") return okText("<section>End</section>");
    throw new Error(`unexpected ${url}`);
  }) as typeof fetch;

  const shell = await mountPreviewShell({
    root,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  expect(shell.manifest).toBeNull();
  expect(root.textContent).toContain("Failed to load fontscope.css: 404");
  expect(root.querySelectorAll(".peitho-preview-slide")).toHaveLength(0);
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
    if (url === "sources.json") return Promise.resolve(okJson(slideSources));
    if (url === "peitho.css") return Promise.resolve(okText(cssText));
    if (url === "fontscope.css") return Promise.resolve(okText(""));
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

it("build_error_message_renders_in_fixed_preformatted_banner", async () => {
  const root = document.createElement("main");
  const shell = await mountForTest(root, new EventTarget());
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));

  channel.onmessage?.({ data: { buildError: "broken build\nhelp: fix the deck" } });

  const banner = root.querySelector<HTMLElement>('[data-peitho-preview="build-error"]')!;
  expect(root.querySelectorAll('[data-peitho-preview="build-error"]')).toHaveLength(1);
  expect(banner.hidden).toBe(false);
  expect(banner.textContent).toBe("broken build\nhelp: fix the deck");
  expect(banner.style.position).toBe("fixed");
  expect(banner.style.maxHeight).toBe("40vh");
  expect(banner.style.overflowY).toBe("auto");
  expect(banner.style.whiteSpace).toBe("pre-wrap");
  expect(Number(banner.style.zIndex)).toBeGreaterThan(1000);
  expect(reload).not.toHaveBeenCalled();
});

it("build_error_uses_text_content_and_cannot_inject_html", async () => {
  const root = document.createElement("main");
  const shell = await mountForTest(root, new EventTarget());
  const channel = mockChannel();
  cleanups.push(installPreviewReload(shell, () => channel, vi.fn()));

  channel.onmessage?.({ data: { buildError: "<b>broken</b>\nline 2" } });

  const banner = root.querySelector<HTMLElement>('[data-peitho-preview="build-error"]')!;
  expect(banner.textContent).toBe("<b>broken</b>\nline 2");
  expect(banner.querySelector("b")).toBeNull();
});

it("null_build_error_hides_and_clears_banner", async () => {
  const root = document.createElement("main");
  const shell = await mountForTest(root, new EventTarget());
  const channel = mockChannel();
  cleanups.push(installPreviewReload(shell, () => channel, vi.fn()));

  channel.onmessage?.({ data: { buildError: "broken build" } });
  channel.onmessage?.({ data: { buildError: null } });

  const banner = root.querySelector<HTMLElement>('[data-peitho-preview="build-error"]')!;
  expect(banner.hidden).toBe(true);
  expect(banner.textContent).toBe("");
  expect(banner.style.display).toBe("");
});

it("build_error_does_not_replace_or_reload_last_good_slides", async () => {
  const root = document.createElement("main");
  const shell = await mountForTest(root, new EventTarget());
  const lastGoodSlide = root.querySelector<HTMLElement>('[data-slide-key="intro"]')!;
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));

  channel.onmessage?.({ data: { buildError: "layout selector no longer matches" } });

  expect(root.contains(lastGoodSlide)).toBe(true);
  expect(root.querySelectorAll(".peitho-preview-slide")).toHaveLength(manifest.slideCount);
  expect(shell.manifest?.title).toBe(manifest.title);
  expect(reload).not.toHaveBeenCalled();
});

it("initial_sync_build_error_is_rendered_after_mount", async () => {
  const root = document.createElement("main");
  const fixture = previewFetchFixture();
  const fetcherMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input) === "/sync") {
      return Promise.resolve(
        okJson({ seq: 3, message: null, generation: 7, buildError: "initial failure" })
      );
    }
    return fixture.fetcher(input, init);
  });
  const fetcher = fetcherMock as unknown as typeof fetch;

  const shell = await mountPreviewShell({
    root,
    bus: new EventTarget(),
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  const banner = root.querySelector<HTMLElement>('[data-peitho-preview="build-error"]')!;
  expect(shell.generation).toBe(7);
  expect(banner.hidden).toBe(false);
  expect(banner.textContent).toBe("initial failure");
  expect(fetcherMock.mock.calls.filter(([input]) => String(input) === "/sync")).toHaveLength(1);
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
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  expect(shell.mode).toBe("grid");
  expect(panel.hidden).toBe(true);
  expect(status.textContent).toBe("");

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

it("source_edit_request_obeys_single_mode_and_one_edit_union", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  let viewport = { width: 1280, height: 720 };
  const { root, shell } = await mountInlineEditForTest({
    mode: "grid",
    bus,
    fixture,
    viewport: () => viewport
  });

  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } })
  );
  expect(shell.mode).toBe("single");
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  note.value = "settling note";
  shell.navigate({ index: 1 });
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));

  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));

  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  const textarea = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="source"]'
  )!;
  const introHost = root.querySelector<HTMLElement>(
    '.peitho-preview-slide[data-slide-key="intro"]'
  )!;
  const middleHost = root.querySelector<HTMLElement>(
    '.peitho-preview-slide[data-slide-key="middle"]'
  )!;
  const endHost = root.querySelector<HTMLElement>(
    '.peitho-preview-slide[data-slide-key="end"]'
  )!;
  expect(textarea.value).toBe("# Middle");
  expect(textarea.closest<HTMLElement>('[data-slide-key="middle"]')).not.toBeNull();
  expect(introHost.hidden).toBe(true);
  expect(middleHost.hidden).toBe(true);
  expect(endHost.hidden).toBe(true);
  const initialFit = calculateCanvasFit(
    {
      width: viewport.width - PREVIEW_STRIP_WIDTH,
      height: viewport.height - PREVIEW_NOTES_HEIGHT
    },
    manifest.canvasWidth,
    manifest.canvasHeight
  );
  expect(parseFloat(textarea.style.left)).toBeCloseTo(initialFit.left);
  expect(parseFloat(textarea.style.top)).toBeCloseTo(initialFit.top);
  expect(parseFloat(textarea.style.width)).toBeCloseTo(
    manifest.canvasWidth * initialFit.scale
  );
  expect(parseFloat(textarea.style.height)).toBeCloseTo(
    manifest.canvasHeight * initialFit.scale
  );
  expect(textarea.style.transform).toBe("");

  viewport = { width: 1440, height: 900 };
  window.dispatchEvent(new Event("resize"));
  const resizedFit = calculateCanvasFit(
    {
      width: viewport.width - PREVIEW_STRIP_WIDTH,
      height: viewport.height - PREVIEW_NOTES_HEIGHT
    },
    manifest.canvasWidth,
    manifest.canvasHeight
  );
  expect(parseFloat(textarea.style.left)).toBeCloseTo(resizedFit.left);
  expect(parseFloat(textarea.style.top)).toBeCloseTo(resizedFit.top);
  expect(parseFloat(textarea.style.width)).toBeCloseTo(
    manifest.canvasWidth * resizedFit.scale
  );
  expect(parseFloat(textarea.style.height)).toBeCloseTo(
    manifest.canvasHeight * resizedFit.scale
  );

  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  expect(root.querySelectorAll('[data-peitho-preview="source"]')).toHaveLength(1);
  const middleEditable = slideShadow(root, "middle").querySelector<HTMLElement>(
    "#middle-editable"
  )!;
  dispatchShadowClick(middleEditable);
  expect(middleEditable.hasAttribute("contenteditable")).toBe(false);

  textarea.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true
    })
  );
  expect(textarea.isConnected).toBe(false);
  expect(introHost.hidden).toBe(true);
  expect(middleHost.hidden).toBe(false);
  expect(endHost.hidden).toBe(true);

  dispatchShadowClick(middleEditable);
  middleEditable.textContent = "pending inline edit";
  middleEditable.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(middleEditable.getAttribute("contenteditable")).toBe("false");
  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() =>
    expect(middleEditable.hasAttribute("contenteditable")).toBe(false)
  );

  const unavailableReason =
    "bare CR line endings are not supported by preview editing\n" +
    "  = help: convert the deck to LF or CRLF line endings, then reload the preview";
  const unavailableFixture = sourceEditFetchFixture({
    version: 1,
    sources: { middle: "# Middle", end: "# End" },
    unavailable: { intro: unavailableReason }
  });
  const unavailableBus = new EventTarget();
  const unavailableMount = await mountInlineEditForTest({
    bus: unavailableBus,
    fixture: unavailableFixture
  });
  unavailableBus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  const unavailableStatus = unavailableMount.root.querySelector<HTMLSpanElement>(
    '[data-peitho-preview="status"]'
  )!;
  const unavailableHost = unavailableMount.root.querySelector<HTMLElement>(
    '.peitho-preview-slide[data-slide-key="intro"]'
  )!;
  expect(unavailableMount.root.querySelector('[data-peitho-preview="source"]')).toBeNull();
  expect(unavailableHost.hidden).toBe(false);
  expect(unavailableStatus.textContent).toBe(unavailableReason);
  expect(unavailableStatus.style.whiteSpace).toBe("pre-wrap");

  delete unavailableFixture.sources.unavailable.intro;
  unavailableBus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  expect(unavailableStatus.textContent).toBe("this slide cannot be edited from preview");
  expect(unavailableHost.hidden).toBe(false);

  unavailableFixture.sources.sources.intro = "# Intro";
  unavailableBus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  const reopenedEditor = unavailableMount.root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="source"]'
  )!;
  expect(reopenedEditor.value).toBe("# Intro");
  expect(unavailableStatus.textContent).toBe("");
  reopenedEditor.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })
  );
});

it("source_edit_refusal_status_clears_on_committed_index_and_mode_transitions", async () => {
  const refusal = "bare CR line endings are not supported by preview editing";
  const fixture = sourceEditFetchFixture({
    version: 1,
    sources: { middle: "# Middle", end: "# End" },
    unavailable: { intro: refusal }
  });
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus, fixture });
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;

  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  expect(status.textContent).toBe(refusal);
  shell.navigate("next");
  expect(shell.currentIndex).toBe(1);
  expect(status.textContent).toBe("");

  shell.navigate("prev");
  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  expect(status.textContent).toBe(refusal);
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } })
  );
  expect(shell.mode).toBe("grid");
  expect(status.textContent).toBe("");
});

it("source_edit_success_clears_a_prior_save_failure", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;

  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  const editor = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="source"]'
  )!;
  editor.value = "# First draft";
  editor.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      metaKey: true,
      bubbles: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  fixture.resolveSourceEditPost({
    ok: false,
    status: 422,
    text: async () => JSON.stringify({ error: "slide source refused" })
  } as Response);
  await vi.waitFor(() => expect(status.textContent).toBe("slide source refused"));

  editor.value = "# Fixed draft";
  editor.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      metaKey: true,
      bubbles: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(2));
  fixture.resolveSourceEditPost(okJson({ key: "fixed", body: "# Fixed canonical" }));
  await vi.waitFor(() => expect(editor.isConnected).toBe(false));
  expect(status.textContent).toBe("");
});

it("derives_one_exact_hint_from_the_total_panel_priority_chain", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const panel = root.querySelector<HTMLElement>('[data-peitho-preview="notes"]')!;

  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  expect(keyTokens(hint)).toEqual(["e", "Enter"]);
  expect(status.textContent).toBe("");
  expect(status.style.background).toBe("");
  expect(panel.style.borderTop).toBe("1px solid rgba(255, 255, 255, 0.16)");
  // CSSOM serializes the neutral #15181e background as rgb().
  expect(panel.style.background).toBe("rgb(21, 24, 30)");

  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  note.focus();
  expect(document.activeElement).toBe(note);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(NOTES_EDIT_HINT_TEXT);
  expect(keyTokens(hint)).toEqual(["Esc", "Enter"]);
  expect(hint.textContent).not.toBe(EDIT_AFFORDANCE_TEXT);
  expect(status.textContent).toBe("");

  note.blur();
  expect(document.activeElement).not.toBe(note);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  expect(keyTokens(hint)).toEqual(["e", "Enter"]);

  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(INLINE_EDIT_HINT_TEXT);
  expect(keyTokens(hint)).toEqual(["Enter", "Shift+Enter", "Esc"]);
  expect(status.textContent).toBe("");
  press(paragraph, "Escape");
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  expect(keyTokens(hint)).toEqual(["e", "Enter"]);

  const editor = openSourceEditor(root, bus);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(SOURCE_EDIT_HINT_TEXT);
  expect(keyTokens(hint)).toEqual(["Cmd/Ctrl+Enter", "Enter", "Esc"]);
  expect(status.textContent).toBe("");

  editor.value = "# Discarded source draft";
  press(editor, "Escape");
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  expect(keyTokens(hint)).toEqual(["u"]);
  expect(status.textContent).toBe("");

  note.focus();
  expect(document.activeElement).toBe(note);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(NOTES_EDIT_HINT_TEXT);
  expect(keyTokens(hint)).toEqual(["Esc", "Enter"]);

  note.value = "Note that will fail";
  note.dispatchEvent(new Event("blur"));
  expect(document.activeElement).toBe(note);
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  fixture.resolveNotesPost(errorJson(500, "note save failed"));
  await vi.waitFor(() => expect(status.textContent).toBe("note save failed"));

  expect(document.activeElement).toBe(note);
  expect(hint.hidden).toBe(true);
  expect(hint.textContent).toBe("");
  expect(status.style.background).toBe("rgb(127, 29, 29)");
  expect(status.style.color).toBe("rgb(254, 226, 226)");
  expect(panel.style.borderTop).toBe("3px solid rgb(239, 68, 68)");
  expect(panel.style.background).toBe("rgb(36, 20, 22)");
});

it("styles_only_affordance_key_tokens_so_the_prose_remains_neutral", async () => {
  const { root } = await mountInlineEditForTest();
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;

  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  expect(hint.style.color).toBe("rgb(156, 163, 175)");
  expect(
    Array.from(hint.children, (child) => ({
      tagName: child.tagName,
      text: child.textContent,
      color: (child as HTMLSpanElement).style.color
    }))
  ).toEqual([
    { tagName: "SPAN", text: "Click text to edit · ", color: "" },
    { tagName: "SPAN", text: "e", color: KEY_HINT_COLOR },
    { tagName: "SPAN", text: " for Markdown · ", color: "" },
    { tagName: "SPAN", text: "Enter", color: KEY_HINT_COLOR },
    { tagName: "SPAN", text: " for notes", color: "" }
  ]);
});

it("styles_source_editor_key_tokens_so_chords_are_distinct_from_instructions", async () => {
  const bus = new EventTarget();
  const { root } = await mountInlineEditForTest({ bus });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;

  openSourceEditor(root, bus);

  expect(hint.textContent).toBe(SOURCE_EDIT_HINT_TEXT);
  expect(
    Array.from(hint.children, (child) => ({
      text: child.textContent,
      color: (child as HTMLSpanElement).style.color
    }))
  ).toEqual([
    { text: "Cmd/Ctrl+Enter", color: KEY_HINT_COLOR },
    { text: " or click away saves · ", color: "" },
    { text: "Enter", color: KEY_HINT_COLOR },
    { text: " inserts a newline · ", color: "" },
    { text: "Esc", color: KEY_HINT_COLOR },
    { text: " cancels", color: "" }
  ]);
});

it("renders_saving_as_prose_because_it_contains_no_key", async () => {
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ fixture });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;

  dispatchShadowClick(paragraph);
  paragraph.textContent = "Inline save held open";
  press(paragraph, "Enter");
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));

  expect(hint.textContent).toBe(SAVING_HINT_TEXT);
  expect(
    Array.from(hint.children, (child) => ({
      text: child.textContent,
      color: (child as HTMLSpanElement).style.color
    }))
  ).toEqual([{ text: SAVING_HINT_TEXT, color: "" }]);

  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(paragraph.hasAttribute("data-peitho-src")).toBe(false));
});

it("clears_hint_token_spans_when_the_single_slot_becomes_hidden", async () => {
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ fixture });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  expect(hint.hidden).toBe(false);
  expect(hint.children.length).toBeGreaterThan(0);

  note.value = "Note that will fail";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  fixture.resolveNotesPost(errorJson(500, "note save failed"));
  await vi.waitFor(() => expect(status.textContent).toBe("note save failed"));

  expect(hint.hidden).toBe(true);
  expect(hint.textContent).toBe("");
  expect(hint.children).toHaveLength(0);
});

it("affordance_names_enter_because_enter_focuses_the_notes_textarea", async () => {
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  expect(shell.mode).toBe("single");
  expect(document.activeElement).not.toBe(note);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);

  const enter = press(window, "Enter");

  expect(enter.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(note);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(NOTES_EDIT_HINT_TEXT);
});

it("restore_offer_yields_to_notes_focus_because_u_cannot_reach_it", async () => {
  const bus = new EventTarget();
  const { root } = await mountInlineEditForTest({ bus });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const draft = "# Restore after leaving notes";

  const editor = openSourceEditor(root, bus);
  editor.value = draft;
  press(editor, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  note.focus();
  expect(document.activeElement).toBe(note);
  expect(hint.textContent).toBe(NOTES_EDIT_HINT_TEXT);
  expect(hint.textContent).not.toBe(RESTORE_OFFER_TEXT);
  expect(press(note, "u").defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();

  note.blur();
  expect(document.activeElement).not.toBe(note);
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  const restore = press(window, "u");
  expect(restore.defaultPrevented).toBe(true);
  expect(
    root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="source"]')?.value
  ).toBe(draft);
});

it("locked_editor_shows_saving_instead_of_keys_it_swallows", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;

  dispatchShadowClick(paragraph);
  paragraph.textContent = "Inline save held open";
  press(paragraph, "Enter");
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));

  expect(paragraph.getAttribute("contenteditable")).toBe("false");
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(SAVING_HINT_TEXT);
  const inlineEscape = press(paragraph, "Escape");
  expect(inlineEscape.defaultPrevented).toBe(true);
  expect(paragraph.getAttribute("contenteditable")).toBe("false");
  expect(paragraph.hasAttribute("data-peitho-src")).toBe(true);
  expect(hint.textContent).toBe(SAVING_HINT_TEXT);

  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
  expect(paragraph.hasAttribute("data-peitho-src")).toBe(false);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);

  const source = openSourceEditor(root, bus);
  source.value = "# Source save held open";
  press(source, "Enter", { metaKey: true });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));

  expect(source.readOnly).toBe(true);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(SAVING_HINT_TEXT);
  const sourceEscape = press(source, "Escape");
  expect(sourceEscape.defaultPrevented).toBe(true);
  expect(source.isConnected).toBe(true);
  expect(hint.textContent).toBe(SAVING_HINT_TEXT);

  fixture.resolveSourceEditPost(errorJson(500, "source save failed"));
  await vi.waitFor(() => expect(status.textContent).toBe("source save failed"));
  expect(source.readOnly).toBe(false);
  expect(source.isConnected).toBe(true);
  expect(hint.hidden).toBe(true);
  expect(hint.textContent).toBe("");
});

it("omits_only_the_Markdown_shortcut_when_the_current_slide_source_is_unavailable", async () => {
  const fixture = sourceEditFetchFixture({
    version: 1,
    sources: { middle: "# Middle", end: "# End" },
    unavailable: { intro: "bare CR line endings are not supported by preview editing" }
  });
  const { root } = await mountInlineEditForTest({ fixture });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;

  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_WITHOUT_SOURCE_TEXT);
  expect(keyTokens(hint)).toEqual(["Enter"]);
  expect(status.textContent).toBe("");
});

it("rederives_the_edit_affordance_when_navigation_changes_source_availability", async () => {
  const fixture = sourceEditFetchFixture({
    version: 1,
    sources: { intro: "# Intro", end: "# End" },
    unavailable: { middle: "bare CR line endings are not supported by preview editing" }
  });
  const { root, shell } = await mountInlineEditForTest({ fixture });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;

  expect(shell.currentIndex).toBe(0);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);

  shell.navigate("next");
  expect(shell.currentIndex).toBe(1);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_WITHOUT_SOURCE_TEXT);

  shell.navigate("next");
  expect(shell.currentIndex).toBe(2);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
});

it("source_edit_shows_key_hint_and_dark_styling_and_an_error_replaces_the_hint", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  expect(status.textContent).toBe("");

  const editor = openSourceEditor(root, bus);

  // The save keys differ from the inline editor's, so the editor must announce them.
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toContain("Cmd/Ctrl+Enter");
  expect(hint.textContent).toContain("Esc cancels");
  // A hint is not a failure: it must not ride the alerting status span.
  expect(status.textContent).toBe("");
  // Readable against the shell's dark ground rather than the browser default.
  expect(editor.style.background).not.toBe("");
  expect(editor.style.color).not.toBe("");
  expect(parseFloat(editor.style.fontSize)).toBeGreaterThan(13);

  editor.value = "# Intro edited";
  press(editor, "Enter", { metaKey: true });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  fixture.resolveSourceEditPost(errorJson(422, "refused for a reason"));

  // An error replaces the hint rather than sitting beside it.
  await vi.waitFor(() => expect(status.textContent).toBe("refused for a reason"));
  expect(hint.hidden).toBe(true);
});

it("source_edit_hint_returns_to_the_affordance_when_the_editor_closes", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;

  const editor = openSourceEditor(root, bus);
  expect(hint.hidden).toBe(false);
  press(editor, "Escape");

  await vi.waitFor(() => expect(editor.isConnected).toBe(false));
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
});

it("dirty_source_escape_offers_and_restores_the_exact_draft_once", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;

  const noOffer = press(window, "u");
  expect(noOffer.defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();

  const draft = "# Exact discarded source\n\n- one\n- two\n";
  const editor = openSourceEditor(root, bus);
  editor.value = draft;
  press(editor, "Escape");

  expect(editor.isConnected).toBe(false);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  expect(status.textContent).toBe("");

  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  note.value = "typed ";
  note.setSelectionRange(note.value.length, note.value.length);
  const typed = press(note, "u");
  if (!typed.defaultPrevented) note.setRangeText("u", note.selectionStart, note.selectionEnd, "end");
  expect(typed.defaultPrevented).toBe(false);
  expect(note.value).toBe("typed u");
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  for (const modifier of [{ metaKey: true }, { ctrlKey: true }]) {
    const chord = press(window, "u", modifier);
    expect(chord.defaultPrevented).toBe(false);
  }
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();

  const restore = press(window, "u");
  expect(restore.defaultPrevented).toBe(true);
  const restored = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="source"]'
  )!;
  expect(restored.value).toBe(draft);
  expect(restored.selectionStart).toBe(0);
  expect(restored.selectionEnd).toBe(0);
  expect(hint.textContent).not.toBe(RESTORE_OFFER_TEXT);
  expect(status.textContent).toBe("");

  press(restored, "Enter", { metaKey: true });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  fixture.resolveSourceEditPost(okJson({ key: "intro", body: draft }));
  await vi.waitFor(() => expect(restored.isConnected).toBe(false));

  const afterSave = press(window, "u");
  expect(afterSave.defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
});

it("an_unrelated_successful_notes_save_keeps_a_discarded_source_draft_restorable", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const draft = "# Source draft unrelated to the notes save";

  const editor = openSourceEditor(root, bus);
  editor.value = draft;
  press(editor, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  note.focus();
  note.value = "Saved note that says nothing about the source draft";
  note.blur();
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notes.notes.intro).toBe(note.value));

  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  expect(press(window, "u").defaultPrevented).toBe(true);
  expect(
    root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="source"]')?.value
  ).toBe(draft);
});

it("a_panel_failure_blocks_restore_until_a_successful_notes_save_clears_it", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const draft = "# Restore after the notes failure clears";

  const editor = openSourceEditor(root, bus);
  editor.value = draft;
  press(editor, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  note.value = "Retry this note";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  fixture.resolveNotesPost(errorJson(500, "note save failed"));
  await vi.waitFor(() => expect(status.textContent).toBe("note save failed"));

  expect(hint.hidden).toBe(true);
  expect(hint.textContent).toBe("");
  expect(press(window, "u").defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();

  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(status.textContent).toBe(""));

  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  expect(press(window, "u").defaultPrevented).toBe(true);
  expect(
    root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="source"]')?.value
  ).toBe(draft);
});

it("a_dirty_escape_during_a_panel_failure_is_hidden_and_not_restorable", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  note.value = "This note fails first";
  note.dispatchEvent(new Event("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  fixture.resolveNotesPost(errorJson(409, "note save conflict"));
  await vi.waitFor(() => expect(status.textContent).toBe("note save conflict"));

  const editor = openSourceEditor(root, bus);
  editor.value = "# Discarded while the failure stands";
  press(editor, "Escape");

  expect(hint.hidden).toBe(true);
  expect(hint.textContent).toBe("");
  expect(press(window, "u").defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
});

it("an_invalidating_transition_eagerly_releases_the_retained_draft", async () => {
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus });
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "Retained DOM-backed draft";
  press(paragraph, "Escape");

  const shellState = shell as unknown as { discardedDraft: unknown };
  expect(shellState.discardedDraft).not.toBeNull();
  shell.navigate("next");
  expect(shell.currentIndex).toBe(1);
  expect(shellState.discardedDraft).toBeNull();
});

it("restore_request_cannot_open_an_editor_while_a_transition_is_settling", async () => {
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const source = openSourceEditor(root, bus);
  source.value = "# Restore only after the transition settles";
  press(source, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  const shellState = shell as unknown as { pendingTransitionSettlements: number };
  shellState.pendingTransitionSettlements = 1;
  const blocked = new CustomEvent("peitho:restorerequest", { cancelable: true });
  bus.dispatchEvent(blocked);
  shellState.pendingTransitionSettlements = 0;

  expect(blocked.defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  expect(press(window, "u").defaultPrevented).toBe(true);
  expect(root.querySelector('[data-peitho-preview="source"]')).not.toBeNull();
});

it("clean_escape_offers_no_restore_and_returns_to_the_affordance", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;

  const source = openSourceEditor(root, bus);
  press(source, "Escape");
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);

  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(INLINE_EDIT_HINT_TEXT);
  press(paragraph, "Escape");
  expect(paragraph.hasAttribute("contenteditable")).toBe(false);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  expect(fixture.sourceEditPosts()).toHaveLength(0);
  expect(fixture.slideEditPosts()).toHaveLength(0);
});

it("dirty_inline_escape_offers_and_restores_the_exact_same_block", async () => {
  const bus = new EventTarget();
  const { root, fixture } = await mountInlineEditForTest({ bus });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  const heading = shadow.querySelector<HTMLElement>("#editable-heading")!;
  const draft = "Exact *discarded* inline text";

  dispatchShadowClick(paragraph);
  paragraph.textContent = draft;
  press(paragraph, "Escape");

  expect(paragraph.hasAttribute("contenteditable")).toBe(false);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  expect(status.textContent).toBe("");

  const restore = press(window, "u");
  expect(restore.defaultPrevented).toBe(true);
  expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only");
  expect(paragraph.textContent).toBe(draft);
  expect(heading.hasAttribute("contenteditable")).toBe(false);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(INLINE_EDIT_HINT_TEXT);
  expect(status.textContent).toBe("");
  expect(fixture.slideEditPosts()).toHaveLength(0);

  press(paragraph, "Enter");
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(JSON.parse(fixture.slideEditPosts()[0][1].body as string).new).toBe(draft);
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
  const afterSave = press(window, "u");
  expect(afterSave.defaultPrevented).toBe(false);
  expect(paragraph.hasAttribute("contenteditable")).toBe(false);
});

it("opening_an_editor_drops_the_restore_offer", async () => {
  const bus = new EventTarget();
  const { root } = await mountInlineEditForTest({ bus });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const source = openSourceEditor(root, bus);
  source.value = "# Offer that opening another editor drops";
  press(source, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only");
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(INLINE_EDIT_HINT_TEXT);

  press(paragraph, "Escape");
  const restore = press(window, "u");
  expect(restore.defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
  expect(paragraph.hasAttribute("contenteditable")).toBe(false);
});

it("slide_and_mode_changes_drop_the_restore_offer", async () => {
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus });
  cleanups.push(installPreviewKeyboard(window, bus));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;

  const beforeSlideChange = openSourceEditor(root, bus);
  beforeSlideChange.value = "# Lost on slide change";
  press(beforeSlideChange, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  shell.navigate("next");
  expect(shell.currentIndex).toBe(1);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  shell.navigate("prev");
  expect(press(window, "u").defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();

  const beforeModeChange = openSourceEditor(root, bus);
  beforeModeChange.value = "# Lost on mode change";
  press(beforeModeChange, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } })
  );
  expect(shell.mode).toBe("grid");
  expect(
    root.querySelector<HTMLElement>('[data-peitho-preview="notes"]')!.hidden
  ).toBe(true);
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } })
  );
  expect(shell.mode).toBe("single");
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  expect(press(window, "u").defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
});

it("activate_selection_rerenders_after_dropping_a_same_position_restore_offer", async () => {
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const source = openSourceEditor(root, bus);
  source.value = "# Offer dropped by same-position activation";
  press(source, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  const controller = Object.getPrototypeOf(shell) as { isEditOpen(): boolean };
  vi.spyOn(controller, "isEditOpen").mockReturnValueOnce(true).mockReturnValue(false);
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } })
  );

  expect(document.activeElement).toBe(note);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(NOTES_EDIT_HINT_TEXT);
});

it("generation_reload_drops_the_restore_offer", async () => {
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus });
  cleanups.push(installPreviewKeyboard(window, bus));
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const source = openSourceEditor(root, bus);
  source.value = "# Lost on generation reload";
  press(source, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);

  channel.onmessage?.({ data: { generation: shell.generation + 1 } });

  expect(reload).toHaveBeenCalledTimes(1);
  expect(hint.hidden).toBe(false);
  expect(hint.textContent).toBe(EDIT_AFFORDANCE_TEXT);
  expect(press(window, "u").defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
});

it("teardown_drops_the_restore_offer_and_listener", async () => {
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus });
  const hint = root.querySelector<HTMLSpanElement>('[data-peitho-preview="source-hint"]')!;
  const source = openSourceEditor(root, bus);
  source.value = "# Lost on teardown";
  press(source, "Escape");
  expect(hint.textContent).toBe(RESTORE_OFFER_TEXT);
  const shellState = shell as unknown as { discardedDraft: unknown };
  const retainedDraft = shellState.discardedDraft;
  expect(retainedDraft).not.toBeNull();

  shell.destroy();
  shells.pop();

  expect(shellState.discardedDraft).toBeNull();
  expect(hint.hidden).toBe(true);
  expect(hint.textContent).toBe("");

  // Re-seeding after teardown isolates listener removal from draft invalidation.
  shellState.discardedDraft = retainedDraft;
  const restore = new CustomEvent("peitho:restorerequest", { cancelable: true });
  bus.dispatchEvent(restore);
  expect(restore.defaultPrevented).toBe(false);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
});

it("source_edit_transition_commits_before_notes_and_navigation", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root, shell } = await mountInlineEditForTest({ bus, fixture });
  cleanups.push(installPreviewKeyboard(window, bus));
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;

  const firstEditor = openSourceEditor(root, bus);
  firstEditor.value = "# Intro edited";
  note.value = "Notes saved after source";

  const arrowRight = press(firstEditor, "ArrowRight");
  expect(arrowRight.defaultPrevented).toBe(false);
  expect(fixture.sourceEditPosts()).toHaveLength(0);

  const pageDown = press(firstEditor, "PageDown");
  press(firstEditor, "PageDown");

  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  expect(pageDown.defaultPrevented).toBe(true);
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(shell.currentIndex).toBe(0);
  expect(JSON.parse(fixture.sourceEditPosts()[0][1].body as string)).toEqual({
    key: "intro",
    old: "# Intro",
    new: "# Intro edited"
  });

  fixture.resolveSourceEditPost(okJson({ key: "intro", body: "# Intro canonical" }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));

  const secondEditor = openSourceEditor(root, bus);
  secondEditor.value = "# Middle edited";
  press(secondEditor, "PageUp");

  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(2));
  expect(shell.currentIndex).toBe(1);
  fixture.resolveSourceEditPost(okJson({ key: "middle", body: "# Middle canonical" }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(0));

  const renderedHeading = slideShadow(root, "intro").querySelector("#editable-heading")!;
  const renderedText = renderedHeading.textContent;
  const focusEditor = openSourceEditor(root, bus);
  expect(focusEditor.value).toBe("# Intro canonical");
  focusEditor.value = "# Saved while focusing notes";
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } })
  );

  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(3));
  expect(JSON.parse(fixture.sourceEditPosts()[2][1].body as string)).toEqual({
    key: "intro",
    old: "# Intro canonical",
    new: "# Saved while focusing notes"
  });
  expect(document.activeElement).toBe(focusEditor);
  expect(focusEditor.readOnly).toBe(true);
  fixture.resolveSourceEditPost(
    okJson({ key: "intro", body: "# Saved while focusing notes" })
  );
  await vi.waitFor(() => expect(focusEditor.isConnected).toBe(false));
  expect(document.activeElement).toBe(note);
  expect(renderedHeading.textContent).toBe(renderedText);
  expect(
    root.querySelector<HTMLElement>('.peitho-preview-slide[data-slide-key="intro"]')!.hidden
  ).toBe(false);
});

it.each([
  { outcome: "saved", response: okJson({ key: "intro", body: "# Saved body" }) },
  { outcome: "failed", response: errorJson(409, "source save failed") }
])("source_edit_blur_then_navigation_finishes_once ($outcome)", async ({ outcome, response }) => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root, shell } = await mountInlineEditForTest({ bus, fixture });
  const controller = Object.getPrototypeOf(shell) as {
    finishSourceEditCommit: (...args: unknown[]) => boolean;
  };
  const finish = vi.spyOn(controller, "finishSourceEditCommit");
  const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
  const scrollIntoView = vi.fn();
  HTMLElement.prototype.scrollIntoView = scrollIntoView;
  cleanups.push(() => {
    HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
  });
  const editor = openSourceEditor(root, bus);
  editor.value = "# Blur then navigate";
  scrollIntoView.mockClear();

  editor.blur();
  shell.navigate({ index: 1 });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  fixture.resolveSourceEditPost(response);

  if (outcome === "saved") {
    await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
    expect(editor.isConnected).toBe(false);
    expect(scrollIntoView).toHaveBeenCalledTimes(1);
  } else {
    await vi.waitFor(() => expect(editor.readOnly).toBe(false));
    expect(shell.currentIndex).toBe(0);
    expect(editor.isConnected).toBe(true);
    expect(editor.value).toBe("# Blur then navigate");
    expect(document.activeElement).toBe(editor);
  }
  expect(finish).toHaveBeenCalledTimes(1);
});

it("source_edit_unchanged_blur_then_thumbnail_proceeds_once_without_post", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root, shell } = await mountInlineEditForTest({ bus, fixture });
  const changes: number[] = [];
  bus.addEventListener("peitho:slidechange", (event) => {
    changes.push((event as CustomEvent<{ index: number }>).detail.index);
  });
  const editor = openSourceEditor(root, bus);
  const thumbnail = root.querySelectorAll<HTMLElement>(".peitho-preview-thumb")[1];

  editor.dispatchEvent(new FocusEvent("blur"));
  thumbnail.click();

  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
  expect(changes).toEqual([1]);
  expect(editor.isConnected).toBe(false);
  expect(fixture.sourceEditPosts()).toHaveLength(0);
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(fixture.slideEditPosts()).toHaveLength(0);
});

it.each([
  {
    trigger: "navigate",
    status: 409,
    run: (_root: HTMLElement, bus: EventTarget): void => {
      bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
    }
  },
  {
    trigger: "thumbnail",
    status: 422,
    run: (root: HTMLElement): void => {
      root.querySelectorAll<HTMLElement>(".peitho-preview-thumb")[1].click();
    }
  },
  {
    trigger: "grid",
    status: 500,
    run: (_root: HTMLElement, bus: EventTarget): void => {
      bus.dispatchEvent(
        new CustomEvent("peitho:overviewrequest", { detail: { action: "toggle" } })
      );
    }
  },
  {
    trigger: "activate",
    status: 409,
    run: (_root: HTMLElement, bus: EventTarget): void => {
      bus.dispatchEvent(
        new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } })
      );
    }
  }
])("source_edit_failure_blocks_transition ($trigger, HTTP $status)", async (row) => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root, shell } = await mountInlineEditForTest({ bus, fixture });
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const panelStatus = root.querySelector<HTMLSpanElement>(
    '[data-peitho-preview="status"]'
  )!;

  const editor = openSourceEditor(root, bus);
  const draft = `# Draft blocked by ${row.trigger}`;
  editor.value = draft;
  note.value = "Notes must wait";
  channel.onmessage?.({ data: { generation: shell.generation + 1 } });

  row.run(root, bus);

  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(shell.currentIndex).toBe(0);
  expect(shell.mode).toBe("single");

  const message = `slide source refused with ${row.status}`;
  fixture.resolveSourceEditPost(errorJson(row.status, message));

  await vi.waitFor(() => expect(editor.readOnly).toBe(false));
  expect(editor.isConnected).toBe(true);
  expect(editor.value).toBe(draft);
  expect(document.activeElement).toBe(editor);
  expect(panelStatus.textContent).toBe(message);
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(fixture.sources.sources.intro).toBe("# Intro");
  expect(shell.currentIndex).toBe(0);
  expect(shell.mode).toBe("single");
  expect(reload).not.toHaveBeenCalled();
});

it("source_edit_reload_releases_once", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root, shell } = await mountInlineEditForTest({ bus, fixture });
  const channel = mockChannel();
  const statesAtReload: unknown[] = [];
  const reload = vi.fn(() => {
    statesAtReload.push(JSON.parse(sessionStorage.getItem("peitho:preview-state")!));
  });
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const deferReload = (): void => {
    channel.onmessage?.({ data: { generation: shell.generation + 1 } });
  };

  const saved = openSourceEditor(root, bus);
  saved.value = "# Saved once";
  deferReload();
  press(saved, "Enter", { ctrlKey: true });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  fixture.resolveSourceEditPost(okJson({ key: "intro", body: "# Saved canonical" }));
  await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  expect(statesAtReload).toEqual([{ mode: "single", index: 0 }]);

  reload.mockClear();
  statesAtReload.length = 0;
  const unchanged = openSourceEditor(root, bus);
  deferReload();
  unchanged.blur();
  await vi.waitFor(() => expect(unchanged.isConnected).toBe(false));
  expect(fixture.sourceEditPosts()).toHaveLength(1);
  await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));

  reload.mockClear();
  const cancelled = openSourceEditor(root, bus);
  cancelled.value = "# Cancelled source draft";
  deferReload();
  press(cancelled, "Escape");
  expect(reload).toHaveBeenCalledTimes(1);

  reload.mockClear();
  const failed = openSourceEditor(root, bus);
  failed.value = "# Source text must never reach PreviewState";
  deferReload();
  const stored = sessionStorage.getItem("peitho:preview-state")!;
  expect(JSON.parse(stored)).toEqual({ mode: "single", index: 0 });
  expect(stored).not.toContain("Source text must never reach PreviewState");
  press(failed, "Enter", { metaKey: true });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(2));
  fixture.resolveSourceEditPost(errorJson(409, "source save failed"));
  await vi.waitFor(() => expect(failed.readOnly).toBe(false));
  expect(reload).not.toHaveBeenCalled();
  press(failed, "Escape");
  expect(reload).toHaveBeenCalledTimes(1);

  reload.mockClear();
  statesAtReload.length = 0;
  const transition = openSourceEditor(root, bus);
  transition.value = "# Saved by transition";
  deferReload();
  shell.navigate({ index: 1 });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(3));
  fixture.resolveSourceEditPost(okJson({ key: "intro", body: "# Transition canonical" }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
  expect(reload).toHaveBeenCalledTimes(1);
  expect(statesAtReload).toEqual([{ mode: "single", index: 1 }]);
});

it("source_edit_derived_key_can_recover_after_build_failure", async () => {
  const bus = new EventTarget();
  const sourceNotes: Notes = { version: 1, notes: { intro: "Keep this note." } };
  const fixture = sourceEditFetchFixture(slideSources, sourceNotes);
  const { root, shell } = await mountInlineEditForTest({ bus, fixture });
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const host = root.querySelector<HTMLElement>(
    '.peitho-preview-slide[data-slide-key="intro"]'
  )!;

  const firstEditor = openSourceEditor(root, bus);
  firstEditor.value = "# Typed";
  press(firstEditor, "Enter", { ctrlKey: true });

  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  expect(JSON.parse(fixture.sourceEditPosts()[0][1].body as string)).toEqual({
    key: "intro",
    old: "# Intro",
    new: "# Typed"
  });
  expect(firstEditor.readOnly).toBe(true);
  fixture.resolveSourceEditPost(okJson({ key: "renamed", body: "# Server canonical" }));
  await vi.waitFor(() => expect(firstEditor.isConnected).toBe(false));
  expect(fixture.sources.sources).toEqual({
    middle: "# Middle",
    end: "# End",
    renamed: "# Server canonical"
  });
  expect(host.hidden).toBe(false);
  expect(host.dataset.slideKey).toBe("intro");
  channel.onmessage?.({ data: { buildError: "layout check failed after source save" } });
  expect(
    root.querySelector<HTMLElement>('[data-peitho-preview="build-error"]')!.textContent
  ).toBe("layout check failed after source save");
  expect(reload).not.toHaveBeenCalled();

  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  expect(note.value).toBe("Keep this note.");
  note.dispatchEvent(new FocusEvent("blur"));
  await Promise.resolve();
  await Promise.resolve();
  expect(fixture.notes.notes).toEqual({ intro: "Keep this note." });
  expect(fixture.notesPosts()).toHaveLength(0);

  note.value = "note still uses manifest identity";
  note.dispatchEvent(new FocusEvent("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(JSON.parse(fixture.notesPosts()[0][1].body as string)).toEqual({
    key: "intro",
    text: "note still uses manifest identity"
  });
  fixture.resolveNotesPost(errorJson(409, "note save refused"));
  await vi.waitFor(() => expect(status.textContent).toBe("note save refused"));

  const unchangedEditor = openSourceEditor(root, bus);
  expect(unchangedEditor.value).toBe("# Server canonical");
  expect(unchangedEditor.closest<HTMLElement>('[data-slide-key="intro"]')).not.toBeNull();
  unchangedEditor.dispatchEvent(new FocusEvent("blur"));
  await vi.waitFor(() => expect(unchangedEditor.isConnected).toBe(false));
  expect(fixture.sourceEditPosts()).toHaveLength(1);
  expect(host.hidden).toBe(false);
  expect(status.textContent).toBe("note save refused");

  const secondEditor = openSourceEditor(root, bus);
  expect(secondEditor.value).toBe("# Server canonical");
  secondEditor.value = "# Fix the build";
  press(secondEditor, "Enter", { metaKey: true });

  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(2));
  expect(JSON.parse(fixture.sourceEditPosts()[1][1].body as string)).toEqual({
    key: "renamed",
    old: "# Server canonical",
    new: "# Fix the build"
  });
  const sourceError = "slide source refused\n  = help: close the unclosed code fence";
  fixture.resolveSourceEditPost(errorJson(422, sourceError));
  await vi.waitFor(() => expect(secondEditor.readOnly).toBe(false));
  expect(secondEditor.isConnected).toBe(true);
  expect(status.textContent).toBe(`note save refused\n${sourceError}`);
  expect(status.style.whiteSpace).toBe("pre-wrap");

  press(secondEditor, "Escape");
  expect(secondEditor.isConnected).toBe(false);
  expect(host.hidden).toBe(false);
  expect(status.textContent).toBe("note save refused");
});

it.each(["inline", "external"] as const)(
  "stale_source_map_after_inline_or_external_failed_rebuild_preserves_draft_on_409 (%s)",
  async (cause) => {
    const bus = new EventTarget();
    const fixture = sourceEditFetchFixture({
      version: 1,
      sources: { ...slideSources.sources, intro: "# Old" },
      unavailable: {}
    });
    const { root, shell } = await mountInlineEditForTest({ bus, fixture });
    const channel = mockChannel();
    const reload = vi.fn();
    cleanups.push(installPreviewReload(shell, () => channel, reload));

    if (cause === "inline") {
      const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
        "#editable-paragraph"
      )!;
      dispatchShadowClick(paragraph);
      paragraph.textContent = "Inline change on disk";
      press(paragraph, "Enter");
      await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
      fixture.resolveSlideEditPost(okJson({ saved: true }));
      await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
    }

    channel.onmessage?.({ data: { buildError: `${cause} change failed to rebuild` } });
    const editor = openSourceEditor(root, bus);
    editor.value = "# Draft must survive";
    shell.navigate({ index: 1 });

    await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
    expect(JSON.parse(fixture.sourceEditPosts()[0][1].body as string)).toEqual({
      key: "intro",
      old: "# Old",
      new: "# Draft must survive"
    });
    const drift = "the deck changed on disk; reload and retry";
    fixture.resolveSourceEditPost(errorJson(409, drift));

    await vi.waitFor(() => expect(editor.readOnly).toBe(false));
    expect(editor.isConnected).toBe(true);
    expect(editor.value).toBe("# Draft must survive");
    expect(document.activeElement).toBe(editor);
    expect(
      root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!.textContent
    ).toBe(drift);
    expect(fixture.sources.sources.intro).toBe("# Old");
    expect(shell.currentIndex).toBe(0);
    expect(shell.mode).toBe("single");
    expect(reload).not.toHaveBeenCalled();
  }
);

it("inline_edit_click_uses_composed_path_inside_current_slide_shadow_root", async () => {
  const { root } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  const emphasis = shadow.querySelector<HTMLElement>("#paragraph-emphasis")!;

  dispatchShadowClick(emphasis);

  expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only");
  expect(paragraph.textContent).toBe("Peitho is a *fast* tool");
});

it("inline_edit_grid_tile_thumbnail_and_non_current_roots_do_not_start", async () => {
  const { root, shell } = await mountInlineEditForTest({ mode: "grid" });
  const introStage = slideShadow(root, "intro");
  const gridParagraph = introStage.querySelector<HTMLElement>("#editable-paragraph")!;

  dispatchShadowClick(gridParagraph);
  expect(shell.mode).toBe("single");
  expect(gridParagraph.hasAttribute("contenteditable")).toBe(false);

  const thumbnailParagraph = slideShadow(root, "intro", true).querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(thumbnailParagraph);
  expect(thumbnailParagraph.hasAttribute("contenteditable")).toBe(false);

  const nonCurrent = slideShadow(root, "middle").querySelector<HTMLElement>("#middle-editable")!;
  dispatchShadowClick(nonCurrent);
  expect(nonCurrent.hasAttribute("contenteditable")).toBe(false);
});

it("inline_edit_plain_link_click_edits_the_block_instead_of_navigating", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const link = shadow.querySelector<HTMLAnchorElement>("#external-link")!;
  const paragraph = shadow.querySelector<HTMLElement>("#editable-link")!;

  const click = dispatchShadowClick(link);

  expect(click.defaultPrevented).toBe(true);
  expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only");
  expect(paragraph.textContent).toBe("[Open docs](https://example.com)");
  expect(fixture.slideEditPosts()).toHaveLength(0);
});

it.each(["metaKey", "ctrlKey", "altKey", "shiftKey"] as const)(
  "inline_edit_modified_link_click_keeps_browser_behavior_%s",
  async (modifier) => {
    const { root, fixture } = await mountInlineEditForTest();
    const shadow = slideShadow(root, "intro");
    const link = shadow.querySelector<HTMLAnchorElement>("#external-link")!;
    const paragraph = shadow.querySelector<HTMLElement>("#editable-link")!;

    const click = new MouseEvent("click", {
      bubbles: true,
      composed: true,
      cancelable: true,
      [modifier]: true
    });
    link.dispatchEvent(click);

    expect(click.defaultPrevented).toBe(false);
    expect(paragraph.hasAttribute("contenteditable")).toBe(false);
    expect(fixture.slideEditPosts()).toHaveLength(0);
  }
);

it("inline_edit_editor_uses_plaintext_only_and_shows_data_peitho_md", async () => {
  const { root } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const heading = shadow.querySelector<HTMLElement>("#editable-heading")!;

  dispatchShadowClick(heading.querySelector("strong")!);

  expect(heading.getAttribute("contenteditable")).toBe("plaintext-only");
  expect(heading.textContent).toBe('A "quote" & **mark**');
  expect(heading.style.outline).not.toBe("");
  expect(heading.parentElement?.classList.contains("slot-title")).toBe(true);
});

it("inline_edit_tight_list_wraps_only_leading_inline_nodes", async () => {
  const { root } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const item = shadow.querySelector<HTMLLIElement>("#editable-tight-item")!;
  const emphasis = shadow.querySelector<HTMLElement>("#tight-emphasis")!;
  const nested = shadow.querySelector<HTMLUListElement>("#nested-list")!;
  const nestedChild = nested.firstElementChild;

  dispatchShadowClick(emphasis);

  const editor = item.firstElementChild as HTMLElement;
  expect(item.hasAttribute("contenteditable")).toBe(false);
  expect(editor).toBeInstanceOf(HTMLSpanElement);
  expect(editor.getAttribute("contenteditable")).toBe("plaintext-only");
  expect(editor.textContent).toBe("parent *one*");
  expect(editor.nextSibling).toBe(nested);
  expect(item.lastChild).toBe(nested);
  expect(nested.firstElementChild).toBe(nestedChild);
});

it("inline_edit_escape_restores_rendered_nodes_without_posting", async () => {
  const bus = new EventTarget();
  const overviewRequests: unknown[] = [];
  bus.addEventListener("peitho:overviewrequest", (event) => {
    overviewRequests.push((event as CustomEvent).detail);
  });
  cleanups.push(installPreviewKeyboard(window, bus));
  const { root, shell, fixture } = await mountInlineEditForTest({ bus });
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  const item = shadow.querySelector<HTMLElement>("#editable-tight-item")!;
  const heading = shadow.querySelector<HTMLElement>("#editable-heading")!;
  const cases = [
    {
      target: paragraph,
      click: shadow.querySelector<HTMLElement>("#paragraph-emphasis")!,
      editor: () => paragraph
    },
    {
      target: item,
      click: shadow.querySelector<HTMLElement>("#tight-emphasis")!,
      editor: () => item.firstElementChild as HTMLElement
    },
    {
      target: heading,
      click: heading.querySelector<HTMLElement>("strong")!,
      editor: () => heading
    }
  ];

  for (const testCase of cases) {
    const originalNodes = Array.from(testCase.target.childNodes);
    dispatchShadowClick(testCase.click);
    const editor = testCase.editor();
    editor.textContent = "changed";
    const escape = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      composed: true,
      cancelable: true
    });
    editor.dispatchEvent(escape);
    expect(escape.defaultPrevented).toBe(true);
    expect(testCase.target.hasAttribute("contenteditable")).toBe(false);
    expectSameNodes(testCase.target.childNodes, originalNodes);
  }

  expect(overviewRequests).toEqual([]);
  expect(shell.mode).toBe("single");
  expect(fixture.slideEditPosts()).toHaveLength(0);
});

it("inline_edit_shift_enter_inserts_one_source_newline", async () => {
  const injected = injectedEditorRange(6, 9);
  const { root, fixture } = await mountInlineEditForTest({
    selectionRangeProvider: injected.provider
  });
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);

  const enter = new KeyboardEvent("keydown", {
    key: "Enter",
    shiftKey: true,
    bubbles: true,
    composed: true,
    cancelable: true
  });
  paragraph.dispatchEvent(enter);

  expect(enter.defaultPrevented).toBe(true);
  expect(paragraph.textContent).toBe("Peitho\n a *fast* tool");
  const caret = injected.selected()!;
  expect(caret.collapsed).toBe(true);
  const beforeCaret = document.createRange();
  beforeCaret.selectNodeContents(paragraph);
  beforeCaret.setEnd(caret.startContainer, caret.startOffset);
  expect(beforeCaret.toString()).toBe("Peitho\n");
  expect(fixture.slideEditPosts()).toHaveLength(0);
});

it("inline_edit_shift_enter_at_end_uses_and_omits_one_trailing_sentinel", async () => {
  const injected = injectedEditorRange("end");
  const { root, fixture } = await mountInlineEditForTest({
    selectionRangeProvider: injected.provider
  });
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  const old = "Peitho is a *fast* tool";
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      shiftKey: true,
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  expect(paragraph.textContent).toBe(`${old}\n\n`);
  const caret = injected.selected()!;
  const beforeCaret = document.createRange();
  beforeCaret.selectNodeContents(paragraph);
  beforeCaret.setEnd(caret.startContainer, caret.startOffset);
  expect(beforeCaret.toString()).toBe(`${old}\n`);
  const afterCaret = document.createRange();
  afterCaret.selectNodeContents(paragraph);
  afterCaret.setStart(caret.startContainer, caret.startOffset);
  expect(afterCaret.toString()).toBe("\n");

  caret.insertNode(document.createTextNode("Q"));
  expect(paragraph.textContent).toBe(`${old}\nQ\n`);
  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(JSON.parse(fixture.slideEditPosts()[0][1].body as string).new).toBe(`${old}\nQ`);
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
  expect(paragraph.textContent).toBe(`${old}\nQ`);
});

it("inline_edit_trailing_sentinel_is_ignored_by_the_unchanged_check", async () => {
  const injected = injectedEditorRange("end");
  const { root, fixture } = await mountInlineEditForTest({
    selectionRangeProvider: injected.provider
  });
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  const originalNodes = Array.from(paragraph.childNodes);
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      shiftKey: true,
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  const caret = injected.selected()!;
  const backspace = document.createRange();
  backspace.setStart(caret.startContainer, caret.startOffset - 1);
  backspace.setEnd(caret.startContainer, caret.startOffset);
  backspace.deleteContents();
  expect(paragraph.textContent).toBe("Peitho is a *fast* tool\n");

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
  expect(fixture.slideEditPosts()).toHaveLength(0);
  expectSameNodes(paragraph.childNodes, originalNodes);
});

it("inline_edit_shift_enter_without_an_editor_range_appends_one_newline", async () => {
  const { root, fixture } = await mountInlineEditForTest({
    selectionRangeProvider: () => null
  });
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      shiftKey: true,
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  expect(paragraph.textContent).toBe("Peitho is a *fast* tool\n");
  expect(fixture.slideEditPosts()).toHaveLength(0);
});

it("inline_edit_enter_and_blur_post_the_exact_request_once", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);
  paragraph.textContent = "Peitho is a **very fast** tool";

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  paragraph.dispatchEvent(new FocusEvent("blur"));

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(paragraph.getAttribute("contenteditable")).toBe("false");
  expect(Number(paragraph.style.opacity)).toBeLessThan(1);
  expect(paragraph.style.outline).not.toBe("");
  expect(fixture.slideEditPosts()[0][0]).toBe("/slide-edit");
  expect(fixture.slideEditPosts()[0][1]).toMatchObject({
    method: "POST",
    headers: { "Content-Type": "application/json" }
  });
  expect(JSON.parse(fixture.slideEditPosts()[0][1].body as string)).toEqual({
    key: "intro",
    start: 120,
    end: 143,
    old: "Peitho is a *fast* tool",
    new: "Peitho is a **very fast** tool"
  });

  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
  expect(paragraph.textContent).toBe("Peitho is a **very fast** tool");
  dispatchShadowClick(paragraph);
  expect(paragraph.hasAttribute("contenteditable")).toBe(false);
});

it("inline_edit_keys_are_ignored_while_the_save_is_in_flight", async () => {
  const bus = new EventTarget();
  const overviewRequests: unknown[] = [];
  bus.addEventListener("peitho:overviewrequest", (event) => {
    overviewRequests.push((event as CustomEvent).detail);
  });
  cleanups.push(installPreviewKeyboard(window, bus));
  const { root, fixture } = await mountInlineEditForTest({ bus });
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);
  paragraph.textContent = "pending source edit";

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(paragraph.getAttribute("contenteditable")).toBe("false");

  const escape = new KeyboardEvent("keydown", {
    key: "Escape",
    bubbles: true,
    composed: true,
    cancelable: true
  });
  paragraph.dispatchEvent(escape);
  const shiftEnter = new KeyboardEvent("keydown", {
    key: "Enter",
    shiftKey: true,
    bubbles: true,
    composed: true,
    cancelable: true
  });
  paragraph.dispatchEvent(shiftEnter);

  expect(escape.defaultPrevented).toBe(true);
  expect(shiftEnter.defaultPrevented).toBe(true);
  expect(overviewRequests).toEqual([]);
  expect(paragraph.textContent).toBe("pending source edit");
  expect(paragraph.getAttribute("contenteditable")).toBe("false");
  expect(fixture.slideEditPosts()).toHaveLength(1);

  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
  expect(paragraph.textContent).toBe("pending source edit");
  expect(paragraph.hasAttribute("data-peitho-src")).toBe(false);
  expect(fixture.slideEditPosts()).toHaveLength(1);
});

it("inline_edit_ime_composition_and_keycode_229_do_not_commit", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);
  paragraph.textContent = "composition in progress";

  const composing = new KeyboardEvent("keydown", {
    key: "Enter",
    isComposing: true,
    bubbles: true,
    composed: true,
    cancelable: true
  });
  paragraph.dispatchEvent(composing);
  const safariComposition = new KeyboardEvent("keydown", {
    key: "Enter",
    bubbles: true,
    composed: true,
    cancelable: true
  });
  Object.defineProperty(safariComposition, "keyCode", { value: 229 });
  paragraph.dispatchEvent(safariComposition);

  expect(composing.defaultPrevented).toBe(false);
  expect(safariComposition.defaultPrevented).toBe(false);
  expect(fixture.slideEditPosts()).toHaveLength(0);
  expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only");
});

it("inline_edit_crlf_attribute_round_trips_exact_old_bytes", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-crlf")!;
  expect(paragraph.getAttribute("data-peitho-md")).toBe("first\r\nsecond");
  dispatchShadowClick(paragraph);
  paragraph.textContent = "first\r\nupdated";
  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(JSON.parse(fixture.slideEditPosts()[0][1].body as string)).toEqual({
    key: "intro",
    start: 300,
    end: 313,
    old: "first\r\nsecond",
    new: "first\r\nupdated"
  });
  fixture.resolveSlideEditPost(okJson({ saved: true }));
});

it("inline_edit_failed_save_stays_open_and_reports_json_error", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);
  paragraph.textContent = "bad edit";
  paragraph.dispatchEvent(new FocusEvent("blur"));

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(paragraph.getAttribute("contenteditable")).toBe("false");
  expect(Number(paragraph.style.opacity)).toBeLessThan(1);
  fixture.resolveSlideEditPost({
    ok: false,
    status: 422,
    text: async () => JSON.stringify({ error: "slide edit refused" })
  } as Response);
  await vi.waitFor(() => expect(status.textContent).toBe("slide edit refused"));
  expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only");
  expect(paragraph.style.opacity).toBe("");
  expect(paragraph.style.outline).not.toBe("");
  expect(shadow.activeElement).toBe(paragraph);

  note.value = "saved note";
  note.dispatchEvent(new FocusEvent("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notes.notes.intro).toBe("saved note"));
  expect(status.textContent).toBe("slide edit refused");

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(2));
  fixture.resolveSlideEditPost({ ok: false, status: 500, text: async () => "{}" } as Response);
  await vi.waitFor(() => expect(status.textContent).toBe("slide edit failed (HTTP 500)"));

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(3));
  fixture.rejectSlideEditPost(new Error("offline"));
  await vi.waitFor(() =>
    expect(status.textContent).toBe("failed to save slide edit: Error: offline")
  );

  note.value = "bad note";
  note.dispatchEvent(new FocusEvent("blur"));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(2));
  fixture.resolveNotesPost({
    ok: false,
    status: 409,
    text: async () => JSON.stringify({ error: "note save refused" })
  } as Response);
  await vi.waitFor(() => expect(status.textContent).toContain("note save refused"));

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(4));
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
  expect(status.textContent).toBe("note save refused");
});

it("inline_edit_unchanged_close_restores_without_posting", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  const originalNodes = Array.from(paragraph.childNodes);
  dispatchShadowClick(shadow.querySelector<HTMLElement>("#paragraph-emphasis")!);

  paragraph.dispatchEvent(new FocusEvent("blur"));

  await vi.waitFor(() => expect(paragraph.hasAttribute("contenteditable")).toBe(false));
  expect(Array.from(paragraph.childNodes)).toEqual(originalNodes);
  expect(fixture.slideEditPosts()).toHaveLength(0);
});

it("commit_transition_saves_slide_edit_before_notes_and_navigation", async () => {
  const bus = new EventTarget();
  const { root, shell, fixture } = await mountInlineEditForTest({ bus });
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "transition source edit";
  note.value = "transition note edit";

  shell.navigate({ index: 1 });

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(shell.currentIndex).toBe(0);
  expect(shell.mode).toBe("single");

  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  expect(shell.mode).toBe("grid");
  mockSelection(true);
  root.querySelectorAll<HTMLElement>(".peitho-preview-tile")[2].click();
  expect(shell.currentIndex).toBe(2);
  expect(shell.mode).toBe("single");

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:navigate", { detail: { to: { index: 1 } } })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } })
  );
  expect(shell.currentIndex).toBe(1);
  expect(shell.mode).toBe("single");
});

it("inline_edit_cannot_start_while_a_transition_is_settling", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const shadow = slideShadow(root, "intro");
  const paragraph = shadow.querySelector<HTMLElement>("#editable-paragraph")!;
  const heading = shadow.querySelector<HTMLElement>("#editable-heading")!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "settling source edit";
  note.value = "settling note edit";

  shell.navigate({ index: 1 });
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));

  dispatchShadowClick(heading);
  expect(heading.hasAttribute("contenteditable")).toBe(false);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
  const middle = slideShadow(root, "middle").querySelector<HTMLElement>("#middle-editable")!;
  dispatchShadowClick(middle);
  expect(middle.getAttribute("contenteditable")).toBe("plaintext-only");
});

it("failed_slide_edit_blocks_slide_change_and_grid_entry", async () => {
  const bus = new EventTarget();
  const { root, shell, fixture } = await mountInlineEditForTest({ bus });
  const panel = root.querySelector<HTMLElement>('[data-peitho-preview="notes"]')!;
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "stale edit";

  root.querySelectorAll<HTMLElement>(".peitho-preview-thumb")[1].click();
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  fixture.resolveSlideEditPost({
    ok: false,
    status: 409,
    text: async () => JSON.stringify({ error: "the deck changed on disk; reload and retry" })
  } as Response);

  await vi.waitFor(() =>
    expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only")
  );
  expect(shell.currentIndex).toBe(0);
  expect(shell.mode).toBe("single");
  expect(panel.hidden).toBe(false);

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(2));
  fixture.resolveSlideEditPost({
    ok: false,
    status: 409,
    text: async () => JSON.stringify({ error: "still stale" })
  } as Response);
  await vi.waitFor(() => expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only"));
  expect(shell.currentIndex).toBe(0);
  expect(shell.mode).toBe("single");
  expect(panel.hidden).toBe(false);
});

it("pageup_pagedown_from_editor_wait_for_commit_before_navigation", async () => {
  const bus = new EventTarget();
  cleanups.push(installPreviewKeyboard(window, bus));
  const { root, shell, fixture } = await mountInlineEditForTest({ bus });
  const intro = slideShadow(root, "intro").querySelector<HTMLElement>("#editable-paragraph")!;
  dispatchShadowClick(intro);
  intro.textContent = "page down edit";

  const pageDown = new KeyboardEvent("keydown", {
    key: "PageDown",
    bubbles: true,
    composed: true,
    cancelable: true
  });
  intro.dispatchEvent(pageDown);
  intro.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "PageDown",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(pageDown.defaultPrevented).toBe(true);
  expect(shell.currentIndex).toBe(0);
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));

  const middle = slideShadow(root, "middle").querySelector<HTMLElement>("#middle-editable")!;
  dispatchShadowClick(middle);
  middle.textContent = "page up edit";
  middle.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "PageUp",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(2));
  expect(shell.currentIndex).toBe(1);
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(0));
});

it("generation_reload_is_deferred_while_edit_is_open", async () => {
  const { root, shell } = await mountInlineEditForTest();
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);

  channel.onmessage?.({ data: { generation: shell.generation + 1 } });
  channel.onmessage?.({ data: { generation: shell.generation + 2 } });

  expect(reload).not.toHaveBeenCalled();
  expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only");
});

it("successful_commit_releases_deferred_reload", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const channel = mockChannel();
  const statesAtReload: unknown[] = [];
  const reload = vi.fn(() => {
    statesAtReload.push(JSON.parse(sessionStorage.getItem("peitho:preview-state")!));
  });
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "saved before reload";
  channel.onmessage?.({ data: { generation: shell.generation + 1 } });

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(reload).not.toHaveBeenCalled();

  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  expect(statesAtReload).toEqual([{ mode: "single", index: 0 }]);
});

it("transition_commit_saves_destination_state_before_releasing_deferred_reload", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const channel = mockChannel();
  const statesAtReload: unknown[] = [];
  const reload = vi.fn(() => {
    statesAtReload.push(JSON.parse(sessionStorage.getItem("peitho:preview-state")!));
  });
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "save on the destination slide";
  channel.onmessage?.({ data: { generation: shell.generation + 1 } });

  paragraph.dispatchEvent(new FocusEvent("blur"));
  shell.navigate({ index: 1 });
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);
  fixture.resolveSlideEditPost(okJson({ saved: true }));

  await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  expect(shell.currentIndex).toBe(1);
  expect(statesAtReload).toEqual([{ mode: "single", index: 1 }]);
});

it("generation_reload_during_settling_transition_waits_for_navigation", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const channel = mockChannel();
  const statesAtReload: unknown[] = [];
  const reload = vi.fn(() => {
    statesAtReload.push(JSON.parse(sessionStorage.getItem("peitho:preview-state")!));
  });
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "reload while settling";
  note.value = "save before reload";

  shell.navigate({ index: 1 });
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));

  channel.onmessage?.({ data: { generation: shell.generation + 1 } });
  expect(reload).not.toHaveBeenCalled();
  expect(shell.currentIndex).toBe(0);

  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  expect(shell.currentIndex).toBe(1);
  expect(statesAtReload).toEqual([{ mode: "single", index: 1 }]);
});

it("generation_reload_deferred_by_settling_is_released_when_notes_flush_fails", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const channel = mockChannel();
  const statesAtReload: unknown[] = [];
  const reload = vi.fn(() => {
    statesAtReload.push(JSON.parse(sessionStorage.getItem("peitho:preview-state")!));
  });
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "saved before failed notes";
  note.value = "keep failed note draft";

  shell.navigate({ index: 1 });
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));

  channel.onmessage?.({ data: { generation: shell.generation + 1 } });
  expect(reload).not.toHaveBeenCalled();

  fixture.resolveNotesPost({
    ok: false,
    status: 409,
    text: async () => JSON.stringify({ error: "notes changed on disk" })
  } as Response);
  await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  expect(shell.currentIndex).toBe(0);
  expect(statesAtReload).toEqual([
    expect.objectContaining({
      mode: "single",
      index: 0,
      draft: expect.objectContaining({ key: "intro", text: "keep failed note draft" })
    })
  ]);
});

it("escape_cancel_releases_deferred_reload", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "discarded edit";
  channel.onmessage?.({ data: { generation: shell.generation + 1 } });

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  expect(fixture.slideEditPosts()).toHaveLength(0);
  expect(reload).toHaveBeenCalledTimes(1);
  expect(paragraph.hasAttribute("contenteditable")).toBe(false);
});

it("drift_409_keeps_editor_open_until_escape_releases_reload", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "conflicting edit";
  channel.onmessage?.({ data: { generation: shell.generation + 1 } });
  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  fixture.resolveSlideEditPost({
    ok: false,
    status: 409,
    text: async () => JSON.stringify({ error: "the deck changed on disk; reload and retry" })
  } as Response);
  await vi.waitFor(() =>
    expect(paragraph.getAttribute("contenteditable")).toBe("plaintext-only")
  );
  expect(reload).not.toHaveBeenCalled();

  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  expect(reload).toHaveBeenCalledTimes(1);
});

it("pagehide_posts_dirty_slide_edit_with_keepalive", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "page exit edit";

  window.dispatchEvent(new Event("pagehide"));

  expect(fixture.slideEditPosts()).toHaveLength(1);
  expect(fixture.slideEditPosts()[0][1]).toMatchObject({
    method: "POST",
    headers: { "Content-Type": "application/json" },
    keepalive: true
  });
  expect(JSON.parse(fixture.slideEditPosts()[0][1].body as string)).toEqual({
    key: "intro",
    start: 120,
    end: 143,
    old: "Peitho is a *fast* tool",
    new: "page exit edit"
  });
  fixture.resolveSlideEditPost(okJson({ saved: true }));
});

it("pagehide_during_in_flight_commit_sends_a_keepalive_safety_post", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "in-flight page exit edit";
  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));

  window.dispatchEvent(new Event("pagehide"));

  expect(fixture.slideEditPosts()).toHaveLength(2);
  expect(fixture.slideEditPosts().map(([, init]) => init.keepalive)).toEqual([false, true]);
  expect(fixture.slideEditPosts()[1][1].body).toBe(fixture.slideEditPosts()[0][1].body);
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  fixture.resolveSlideEditPost(okJson({ saved: true }));
});

it("pagehide_does_not_post_clean_or_cancelled_slide_edit", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);

  window.dispatchEvent(new Event("pagehide"));
  expect(fixture.slideEditPosts()).toHaveLength(0);

  paragraph.textContent = "cancelled page exit edit";
  paragraph.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  window.dispatchEvent(new Event("pagehide"));
  expect(fixture.slideEditPosts()).toHaveLength(0);
});

it("pagehide_during_in_flight_source_commit_sends_a_keepalive_safety_post", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const editor = openSourceEditor(root, bus);
  editor.value = "# Source page exit";
  press(editor, "Enter", { metaKey: true });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));

  window.dispatchEvent(new Event("pagehide"));

  expect(fixture.sourceEditPosts()).toHaveLength(2);
  expect(fixture.sourceEditPosts().map(([, init]) => init.keepalive)).toEqual([false, true]);
  expect(fixture.sourceEditPosts()[1][1].body).toBe(fixture.sourceEditPosts()[0][1].body);
  expect(JSON.parse(fixture.sourceEditPosts()[1][1].body as string)).toEqual({
    key: "intro",
    old: "# Intro",
    new: "# Source page exit"
  });
  fixture.resolveSourceEditPost(okJson({ key: "intro", body: "# Source page exit" }));
  fixture.resolveSourceEditPost(okJson({ key: "intro", body: "# Source page exit" }));
});

it("pagehide_does_not_post_absent_clean_or_cancelled_source_edits", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });

  window.dispatchEvent(new Event("pagehide"));
  expect(fixture.sourceEditPosts()).toHaveLength(0);

  const clean = openSourceEditor(root, bus);
  window.dispatchEvent(new Event("pagehide"));
  expect(fixture.sourceEditPosts()).toHaveLength(0);

  clean.value = "# Cancelled source page exit";
  press(clean, "Escape");
  window.dispatchEvent(new Event("pagehide"));
  expect(fixture.sourceEditPosts()).toHaveLength(0);
});

it("pagehide_shares_the_keepalive_budget_between_source_and_notes", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const editor = openSourceEditor(root, bus);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const emptySourceSize = new TextEncoder().encode(
    JSON.stringify({ key: "intro", old: "# Intro", new: "" })
  ).length;
  editor.value = "x".repeat(59_000 - emptySourceSize);
  note.value = "n".repeat(1_500);
  const sourceSize = new TextEncoder().encode(
    JSON.stringify({ key: "intro", old: "# Intro", new: editor.value })
  ).length;
  const noteSize = new TextEncoder().encode(
    JSON.stringify({ key: "intro", text: note.value })
  ).length;
  expect(sourceSize).toBe(59_000);
  expect(sourceSize + noteSize).toBeGreaterThan(60_000);

  window.dispatchEvent(new Event("pagehide"));

  expect(fixture.sourceEditPosts()).toHaveLength(1);
  expect(fixture.notesPosts()).toHaveLength(1);
  expect(fixture.sourceEditPosts()[0][1].keepalive).toBe(true);
  expect(fixture.notesPosts()[0][1].keepalive).toBe(false);
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(fixture.notes.notes.intro).toBe(note.value));
});

it("pagehide_downgrades_an_oversized_inline_edit", async () => {
  const { root, fixture } = await mountInlineEditForTest();
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  const emptyRequestSize = new TextEncoder().encode(
    JSON.stringify({
      key: "intro",
      start: 120,
      end: 143,
      old: "Peitho is a *fast* tool",
      new: ""
    })
  ).length;
  paragraph.textContent = "x".repeat(60_001 - emptyRequestSize);

  window.dispatchEvent(new Event("pagehide"));

  expect(fixture.slideEditPosts()).toHaveLength(1);
  expect(
    new TextEncoder().encode(fixture.slideEditPosts()[0][1].body as string)
  ).toHaveLength(60_001);
  expect(fixture.slideEditPosts()[0][1].keepalive).toBe(false);
});

it("slide_edits_are_never_written_to_session_storage", async () => {
  const { root, shell } = await mountInlineEditForTest();
  const channel = mockChannel();
  cleanups.push(installPreviewReload(shell, () => channel, vi.fn()));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "never persist this slide edit";

  shell.saveState();
  channel.onmessage?.({ data: { generation: shell.generation + 1 } });
  window.dispatchEvent(new Event("pagehide"));

  const stored = sessionStorage.getItem("peitho:preview-state")!;
  expect(JSON.parse(stored)).toEqual({ mode: "single", index: 0 });
  expect(stored).not.toContain("never persist this slide edit");
  expect(stored).not.toContain("Peitho is a *fast* tool");
});

it("discarded_drafts_are_never_written_to_session_storage", async () => {
  const bus = new EventTarget();
  const { root, shell } = await mountInlineEditForTest({ bus });
  const sourceDraft = "# Retained source must stay in memory";
  const source = openSourceEditor(root, bus);
  source.value = sourceDraft;
  press(source, "Escape");

  sessionStorage.removeItem("peitho:preview-state");
  shell.saveState();
  let stored = sessionStorage.getItem("peitho:preview-state");
  expect(stored).not.toBeNull();
  expect(JSON.parse(stored!)).toEqual({ mode: "single", index: 0 });
  expect(stored!).not.toContain(sourceDraft);

  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  const inlineDraft = "Retained inline must stay in memory";
  dispatchShadowClick(paragraph);
  paragraph.textContent = inlineDraft;
  press(paragraph, "Escape");

  sessionStorage.removeItem("peitho:preview-state");
  shell.saveState();
  stored = sessionStorage.getItem("peitho:preview-state");
  expect(stored).not.toBeNull();
  expect(JSON.parse(stored!)).toEqual({ mode: "single", index: 0 });
  expect(stored!).not.toContain(sourceDraft);
  expect(stored!).not.toContain(inlineDraft);
  expect(stored!).not.toContain("Peitho is a *fast* tool");

  shell.requestGenerationReload(shell.generation + 1, vi.fn());
  window.dispatchEvent(new Event("pagehide"));
  stored = sessionStorage.getItem("peitho:preview-state");
  expect(stored).not.toBeNull();
  expect(JSON.parse(stored!)).toEqual({ mode: "single", index: 0 });
  expect(stored!).not.toContain(sourceDraft);
  expect(stored!).not.toContain(inlineDraft);
  expect(stored!).not.toContain("Peitho is a *fast* tool");
});

it("destroy_removes_editor_tile_and_pagehide_listeners", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "must not save after destroy";
  note.value = "must not save notes after destroy";

  shell.destroy();
  shells.pop();
  paragraph.dispatchEvent(new FocusEvent("blur"));
  window.dispatchEvent(new Event("pagehide"));
  root.querySelectorAll<HTMLElement>(".peitho-preview-thumb")[1].click();
  await new Promise((resolve) => setTimeout(resolve, 0));

  expect(fixture.slideEditPosts()).toHaveLength(0);
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(shell.currentIndex).toBe(0);
});

it("destroy_removes_an_open_source_edit_and_every_source_listener", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  let viewport = { width: 1280, height: 720 };
  const { root, shell } = await mountInlineEditForTest({
    bus,
    fixture,
    viewport: () => viewport
  });
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const editor = openSourceEditor(root, bus);
  editor.value = "# Refused before destroy";
  press(editor, "Enter", { metaKey: true });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  fixture.resolveSourceEditPost(errorJson(422, "source status before destroy"));
  await vi.waitFor(() => expect(status.textContent).toBe("source status before destroy"));
  expect(editor.isConnected).toBe(true);
  note.value = "must not save notes after source destroy";

  shell.destroy();
  shells.pop();

  expect(editor.isConnected).toBe(false);
  expect(status.textContent).toBe("");
  const htmlAfterDestroy = root.innerHTML;
  const sourcePostsAfterDestroy = fixture.sourceEditPosts().length;
  bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
  press(editor, "Enter", { metaKey: true });
  press(editor, "Escape");
  editor.dispatchEvent(new FocusEvent("blur"));
  viewport = { width: 900, height: 500 };
  window.dispatchEvent(new Event("resize"));
  window.dispatchEvent(new Event("pagehide"));
  root.querySelectorAll<HTMLElement>(".peitho-preview-thumb")[1].click();
  await new Promise((resolve) => setTimeout(resolve, 0));

  expect(root.innerHTML).toBe(htmlAfterDestroy);
  expect(root.querySelector('[data-peitho-preview="source"]')).toBeNull();
  expect(fixture.sourceEditPosts()).toHaveLength(sourcePostsAfterDestroy);
  expect(fixture.slideEditPosts()).toHaveLength(0);
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(shell.currentIndex).toBe(0);
});

it("destroy_removes_notes_textarea_listeners", async () => {
  const { root, shell, fixture } = await mountInlineEditForTest();
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const controller = Object.getPrototypeOf(shell) as { renderPanelStatus(): void };
  const renderPanelStatus = vi.spyOn(controller, "renderPanelStatus");

  shell.destroy();
  shells.pop();
  const renderCallsAfterDestroy = renderPanelStatus.mock.calls.length;
  note.value = "must not save after destroy";
  expect(() => note.dispatchEvent(new FocusEvent("focus"))).not.toThrow();
  note.dispatchEvent(new FocusEvent("blur"));
  await new Promise((resolve) => setTimeout(resolve, 0));

  expect(renderPanelStatus).toHaveBeenCalledTimes(renderCallsAfterDestroy);
  expect(fixture.notesPosts()).toHaveLength(0);
});

it("installer_cleanups_remove_keyboard_and_sync_listeners", async () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const root = document.createElement("main");
  const shell = await mountForTest(root, bus);
  const channel = mockChannel();
  const reload = vi.fn();
  const keyboardCleanup = installPreviewKeyboard(window, bus);
  const reloadCleanup = installPreviewReload(shell, () => channel, reload);
  cleanups.push(keyboardCleanup, reloadCleanup);

  keyboardCleanup();
  reloadCleanup();
  window.dispatchEvent(
    new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true })
  );

  expect(requests).toEqual([]);
  expect(channel.onmessage).toBeNull();
  expect(channel.closed).toBe(true);
  expect(reload).not.toHaveBeenCalled();
});

it("destroy_discards_deferred_reload_and_pending_edit_completion", async () => {
  const bus = new EventTarget();
  const { root, shell, fixture } = await mountInlineEditForTest({ bus });
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(shell, () => channel, reload));
  const paragraph = slideShadow(root, "intro").querySelector<HTMLElement>(
    "#editable-paragraph"
  )!;
  dispatchShadowClick(paragraph);
  paragraph.textContent = "pending destroy edit";
  channel.onmessage?.({ data: { generation: shell.generation + 1 } });
  shell.navigate("next");
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));

  shell.destroy();
  shells.pop();
  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await Promise.resolve();
  await Promise.resolve();

  expect(reload).not.toHaveBeenCalled();
  expect(shell.currentIndex).toBe(0);
  paragraph.dispatchEvent(new FocusEvent("blur"));
  expect(fixture.slideEditPosts()).toHaveLength(1);
});

it.each([
  ["success", okJson({ key: "renamed", body: "# Late canonical" })],
  ["failure", errorJson(422, "late source failure")]
])("destroy_makes_late_source_commit_%s_inert", async (_outcome, response) => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root, shell } = await mountInlineEditForTest({ bus, fixture });
  const finish = vi.spyOn(Object.getPrototypeOf(shell), "finishSourceEditCommit");
  const status = root.querySelector<HTMLSpanElement>('[data-peitho-preview="status"]')!;
  const reload = vi.fn();
  const beforeSources = { ...fixture.sources.sources };
  const editor = openSourceEditor(root, bus);
  editor.value = "# Pending source destroy";
  press(editor, "Enter", { ctrlKey: true });
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  shell.requestGenerationReload(shell.generation + 1, reload);

  shell.destroy();
  shells.pop();
  fixture.resolveSourceEditPost(response);
  await vi.waitFor(() => expect(finish).toHaveBeenCalledTimes(1));

  expect(editor.isConnected).toBe(false);
  expect(fixture.sources.sources).toEqual(beforeSources);
  expect(status.textContent).toBe("");
  expect(reload).not.toHaveBeenCalled();
});
