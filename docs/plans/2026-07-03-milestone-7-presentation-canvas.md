# Peitho Milestone 7 Presentation Canvas Plan

## Purpose

Milestone 7 changes Peitho presentation output from a reflowing web document into a presentation-style fixed canvas. The logical slide size is always `1280x720`. The shell and lightweight distribution index scale that canvas to the viewport or presenter pane; slide templates and theme CSS remain authored against the fixed logical canvas.

This milestone does not change Markdown parsing, template contracts, manifest schema, notes schema, or the build/present/publish command split. `examples/deck.md` remains unchanged.

## Design Decisions

- Canvas size is centralized in TypeScript as `CANVAS_WIDTH = 1280` and `CANVAS_HEIGHT = 720`.
- The presentation shell owns scaling and letterboxing. Slide CSS never computes viewport scale.
- `present.html` contains a blank presentation surface only. The old always-visible presenter link is removed.
- Presentation controls are UI components outside `PresentShell`. They dispatch `peitho:navigate`, subscribe to `peitho:slidechange`, call browser fullscreen APIs, and open `presenter.html`; they do not call shell methods.
- Timer ownership stays in `PresentShell`, but the session starts only after `peitho:timercontrol` with `{ action: "start" }`.
- `reset` returns the timer to the not-started state: `startedAt() === null`, `elapsedMs() === 0`, and a later `start` emits a new `peitho:presentationstart`.
- `presentationend` is emitted only for a started session, once, on `pagehide` or `destroy()`.
- `dist/index.html` stays shell-free and uses inline JavaScript only. It shows one slide at a time in a scaled light-DOM canvas with keyboard and click navigation.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `packages/peitho-present/src/canvas.ts` | canvas constants, pure fit calculation, DOM scaler installer | browser DOM |
| `packages/peitho-present/src/controls.ts` | presentation control bar, click navigation, fullscreen shortcut | `NavigateDetail`, browser DOM |
| `packages/peitho-present/src/shell.ts` | Shadow DOM slide hosts, canvas scaling, manual timer/session lifecycle | `canvas.ts`, generated manifest types |
| `packages/peitho-present/src/presenter.ts` | presenter view Start button, scaled current and next panes | `canvas.ts`, `shell.ts`, `sync.ts` |
| `packages/peitho-present/src/index.ts` | exports shell, presenter, scaler, and controls for bundled `shell.js` | local TS modules |
| `packages/peitho-present/test/canvas.test.ts` | fit and resize tests | `canvas.ts` |
| `packages/peitho-present/test/controls.test.ts` | control bar, click navigation, fullscreen shortcut tests | `controls.ts` |
| `packages/peitho-present/test/session.test.ts` | manual start, pause/resume/reset, end-once tests | `shell.ts` |
| `packages/peitho-present/test/presenter.test.ts` | presenter Start button, preview scaling, notes, sync tests | `presenter.ts` |
| `packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts` | shell canvas host and existing navigation/fetch regressions | `shell.ts`, `keyboard.ts` |
| `crates/peitho-core/src/render.rs` | updated `present.html`, `presenter.html`, and distribution `index.html` | existing render module |
| `crates/peitho-core/src/lib.rs` | exports unchanged render functions | `render.rs` |
| `crates/peitho/tests/present.rs` | present cache smoke assertions for new generated HTML | CLI binary |
| `crates/peitho/tests/build.rs` | base theme fixed-canvas assertion and distribution output smoke | CLI binary |
| `themes/base.css` | 1280x720 fixed-canvas theme | rendered slide classes |

Dependency order:

1. Add canvas constants and scaler helpers.
2. Apply scaler to shell slide hosts.
3. Change shell session lifecycle to manual start.
4. Add presentation controls and wire `present.html`.
5. Update presenter view and presenter HTML.
6. Replace distribution index behavior.
7. Rewrite base theme and update integration smoke tests.

## Implementation Tasks

### Task 1 - Add Canvas Constants and Pure Fit Calculation

Goal: define the canonical logical canvas size and deterministic fit math before touching DOM code.

Files:

- `packages/peitho-present/src/canvas.ts`
- `packages/peitho-present/test/canvas.test.ts`
- `packages/peitho-present/src/index.ts`

Test:

```ts
// packages/peitho-present/test/canvas.test.ts
import { expect, it } from "vitest";
import { CANVAS_HEIGHT, CANVAS_WIDTH, calculateCanvasFit } from "../src/index";

it("exports the fixed Peitho canvas size", () => {
  expect(CANVAS_WIDTH).toBe(1280);
  expect(CANVAS_HEIGHT).toBe(720);
});

it("fits a 16 by 9 viewport without letterbox", () => {
  expect(calculateCanvasFit({ width: 1920, height: 1080 })).toEqual({
    scale: 1.5,
    width: 1920,
    height: 1080,
    left: 0,
    top: 0
  });
});

it("letterboxes vertically in a square viewport", () => {
  expect(calculateCanvasFit({ width: 1000, height: 1000 })).toEqual({
    scale: 0.78125,
    width: 1000,
    height: 562.5,
    left: 0,
    top: 218.75
  });
});

it("letterboxes horizontally in a narrow viewport", () => {
  expect(calculateCanvasFit({ width: 500, height: 720 })).toEqual({
    scale: 0.390625,
    width: 500,
    height: 281.25,
    left: 0,
    top: 219.375
  });
});
```

Expected Red:

```text
No matching export in "src/index.ts" for import "calculateCanvasFit"
```

Implementation:

```ts
// packages/peitho-present/src/canvas.ts
export const CANVAS_WIDTH = 1280;
export const CANVAS_HEIGHT = 720;

export type CanvasViewport = {
  width: number;
  height: number;
};

export type CanvasFit = {
  scale: number;
  width: number;
  height: number;
  left: number;
  top: number;
};

export function calculateCanvasFit(
  viewport: CanvasViewport,
  canvasWidth = CANVAS_WIDTH,
  canvasHeight = CANVAS_HEIGHT
): CanvasFit {
  const scale = Math.min(viewport.width / canvasWidth, viewport.height / canvasHeight);
  const width = canvasWidth * scale;
  const height = canvasHeight * scale;
  return {
    scale,
    width,
    height,
    left: (viewport.width - width) / 2,
    top: (viewport.height - height) / 2
  };
}
```

```ts
// packages/peitho-present/src/index.ts
export {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  calculateCanvasFit
} from "./canvas";
export type { CanvasFit, CanvasViewport } from "./canvas";
```

Verification:

```sh
cd packages/peitho-present
npm test -- canvas.test.ts
```

### Task 2 - Add DOM Canvas Scaler

Goal: install and remove a resize-driven transform that scales a `1280x720` element into a viewport or pane.

Files:

- `packages/peitho-present/src/canvas.ts`
- `packages/peitho-present/test/canvas.test.ts`

Test:

```ts
// packages/peitho-present/test/canvas.test.ts
import { afterEach, expect, it, vi } from "vitest";
import { installCanvasScaler } from "../src/index";

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
});

it("applies fixed canvas dimensions and a centered transform", () => {
  const target = document.createElement("section");
  const cleanup = installCanvasScaler({
    window,
    target,
    viewport: () => ({ width: 1920, height: 1080 })
  });
  cleanups.push(cleanup);

  expect(target.style.width).toBe("1280px");
  expect(target.style.height).toBe("720px");
  expect(target.style.transformOrigin).toBe("top left");
  expect(target.style.transform).toBe("translate(0px, 0px) scale(1.5)");
});

it("updates the transform on resize and stops after cleanup", () => {
  let viewport = { width: 1000, height: 1000 };
  const target = document.createElement("section");
  const cleanup = installCanvasScaler({
    window,
    target,
    viewport: () => viewport
  });

  expect(target.style.transform).toBe("translate(0px, 218.75px) scale(0.78125)");
  viewport = { width: 1280, height: 720 };
  window.dispatchEvent(new Event("resize"));
  expect(target.style.transform).toBe("translate(0px, 0px) scale(1)");

  cleanup();
  viewport = { width: 1920, height: 1080 };
  window.dispatchEvent(new Event("resize"));
  expect(target.style.transform).toBe("translate(0px, 0px) scale(1)");
});

it("uses window inner size by default", () => {
  const target = document.createElement("section");
  vi.spyOn(window, "innerWidth", "get").mockReturnValue(1280);
  vi.spyOn(window, "innerHeight", "get").mockReturnValue(720);

  const cleanup = installCanvasScaler({ window, target });
  cleanups.push(cleanup);

  expect(target.style.transform).toBe("translate(0px, 0px) scale(1)");
});
```

Implementation:

```ts
// packages/peitho-present/src/canvas.ts
export type CanvasScalerOptions = {
  target: HTMLElement;
  window?: Window;
  viewport?: () => CanvasViewport;
  canvasWidth?: number;
  canvasHeight?: number;
};

export function installCanvasScaler(options: CanvasScalerOptions): () => void {
  const win = options.window ?? window;
  const canvasWidth = options.canvasWidth ?? CANVAS_WIDTH;
  const canvasHeight = options.canvasHeight ?? CANVAS_HEIGHT;
  const viewport =
    options.viewport ??
    (() => ({
      width: win.innerWidth,
      height: win.innerHeight
    }));

  function apply(): void {
    const fit = calculateCanvasFit(viewport(), canvasWidth, canvasHeight);
    options.target.style.width = `${canvasWidth}px`;
    options.target.style.height = `${canvasHeight}px`;
    options.target.style.transformOrigin = "top left";
    options.target.style.transform = `translate(${fit.left}px, ${fit.top}px) scale(${fit.scale})`;
  }

  apply();
  win.addEventListener("resize", apply);
  return () => win.removeEventListener("resize", apply);
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- canvas.test.ts
```

### Task 3 - Scale Shell Slide Hosts

Goal: every shell-rendered slide host is a fixed `1280x720` Shadow DOM canvas scaled to the viewport, with resize cleanup on destroy.

Files:

- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts`

Test:

```ts
// packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts
import { expect, it, vi } from "vitest";
import { mountPresentShell } from "../src/index";

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
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
import { installCanvasScaler, type CanvasViewport } from "./canvas";

export type ShellOptions = {
  root: HTMLElement;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  console?: Pick<Console, "error">;
  bus?: EventTarget;
  now?: () => number;
  viewport?: () => CanvasViewport;
};

class PresentShellController implements PresentShell {
  private readonly viewport?: () => CanvasViewport;
  private readonly canvasCleanups: Array<() => void> = [];

  constructor(options: ShellOptions) {
    this.viewport = options.viewport;
    this.root = options.root;
    this.root.classList.add("peitho-shell-viewport");
    this.root.style.position = "relative";
    this.root.style.overflow = "hidden";
    this.root.style.background = "#000";
  }

  destroy(): void {
    this.endPresentation();
    while (this.canvasCleanups.length > 0) this.canvasCleanups.pop()?.();
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:timercontrol", this.onTimerControl);
    this.win.removeEventListener("pagehide", this.onPageHide);
  }

  private createSlideHost(slide: ManifestSlide, html: string, css: string): HTMLElement {
    const host = this.doc.createElement("section");
    host.classList.add("peitho-slide");
    host.dataset.slideKey = slide.key;
    host.dataset.slideIndex = String(slide.index);
    host.dataset.peithoCanvas = "slide";
    host.style.position = "absolute";
    host.style.left = "0";
    host.style.top = "0";
    this.canvasCleanups.push(
      installCanvasScaler({
        window: this.win,
        target: host,
        viewport: this.viewport
      })
    );

    const shadow = host.attachShadow({ mode: "open" });
    const style = this.doc.createElement("style");
    style.textContent = css;
    shadow.appendChild(style);
    const template = this.doc.createElement("template");
    template.innerHTML = html;
    shadow.appendChild(template.content.cloneNode(true));
    return host;
  }
}
```

When implementing, preserve the existing fetch, Shadow DOM CSS injection, navigation, invalid target logging, and no-op slidechange behavior.

Verification:

```sh
cd packages/peitho-present
npm test -- loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts
```

### Task 4 - Make Presentation Session Start Manual

Goal: remove mount-time `presentationstart`; start the timer only on `peitho:timercontrol` `{ action: "start" }`.

Files:

- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/session.test.ts`

Test:

```ts
// packages/peitho-present/test/session.test.ts
import { afterEach, expect, it } from "vitest";
import { mountPresentShell } from "../src/index";
import type { PresentShell } from "../src/index";

const shells: PresentShell[] = [];

afterEach(() => {
  while (shells.length > 0) shells.pop()?.destroy();
});

it("does not start the presentation on mount", async () => {
  const starts: unknown[] = [];
  const bus = new EventTarget();
  bus.addEventListener("peitho:presentationstart", (event) =>
    starts.push((event as CustomEvent).detail)
  );

  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => 1000
  });
  shells.push(shell);

  expect(starts).toEqual([]);
  expect(shell.startedAt()).toBeNull();
  expect(shell.elapsedMs()).toBe(0);
  expect(shell.isPaused()).toBe(false);
});

it("starts once from timercontrol start and ignores duplicate start", async () => {
  let now = 1000;
  const starts: unknown[] = [];
  const bus = new EventTarget();
  bus.addEventListener("peitho:presentationstart", (event) =>
    starts.push((event as CustomEvent).detail)
  );
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  shells.push(shell);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  now = 1750;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));

  expect(starts).toEqual([{ total: 2, startedAt: 1000 }]);
  expect(shell.startedAt()).toBe(1000);
  expect(shell.elapsedMs()).toBe(750);
});

it("pauses resumes and resets only after a manual start", async () => {
  let now = 1000;
  const starts: unknown[] = [];
  const bus = new EventTarget();
  bus.addEventListener("peitho:presentationstart", (event) =>
    starts.push((event as CustomEvent).detail)
  );
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  shells.push(shell);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  expect(shell.isPaused()).toBe(false);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  now = 1500;
  expect(shell.elapsedMs()).toBe(500);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  expect(shell.isPaused()).toBe(true);
  now = 2500;
  expect(shell.elapsedMs()).toBe(500);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "resume" } }));
  now = 3000;
  expect(shell.elapsedMs()).toBe(1000);

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } }));
  expect(shell.startedAt()).toBeNull();
  expect(shell.elapsedMs()).toBe(0);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  expect(starts).toEqual([
    { total: 2, startedAt: 1000 },
    { total: 2, startedAt: 3000 }
  ]);
});

it("emits presentationend only once and only after start", async () => {
  let now = 1000;
  const bus = new EventTarget();
  const ends: unknown[] = [];
  bus.addEventListener("peitho:presentationend", (event) =>
    ends.push((event as CustomEvent).detail)
  );
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });

  shell.destroy();
  expect(ends).toEqual([]);

  const startedShell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  now = 1750;
  window.dispatchEvent(new Event("pagehide"));
  startedShell.destroy();

  expect(ends).toEqual([{ endedAt: 1750, elapsedMs: 750 }]);
});
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
export type TimerControlDetail = {
  action: "start" | "pause" | "resume" | "reset";
};

private readonly onTimerControl = (event: Event): void => {
  const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
  if (action === "start") this.startPresentation();
  else if (action === "pause") this.pauseTimer();
  else if (action === "resume") this.resumeTimer();
  else if (action === "reset") this.resetTimer();
  else this.log.error("Invalid peitho:timercontrol event");
};

async load(): Promise<void> {
  try {
    const manifest = await this.fetchJson<Manifest>("manifest.json");
    const css = await this.fetchText("peitho.css");
    const pending: SlideView[] = [];
    for (const slide of manifest.slides) {
      const html = await this.fetchText(slide.src);
      const host = this.createSlideHost(slide, html, css);
      pending.push({ meta: slide, host });
    }
    this.manifest = manifest;
    for (const view of pending) {
      this.root.appendChild(view.host);
      this.slides.push(view);
    }
    this.show(0);
  } catch (error) {
    this.root.replaceChildren();
    this.root.textContent = error instanceof Error ? error.message : String(error);
  }
}

private startPresentation(): void {
  if (this.startedAtValue !== null) return;
  this.startedAtValue = this.now();
  this.pausedAtValue = null;
  this.pausedTotalMs = 0;
  this.ended = false;
  this.bus.dispatchEvent(
    new CustomEvent<PresentationStartDetail>("peitho:presentationstart", {
      detail: { total: this.slides.length, startedAt: this.startedAtValue }
    })
  );
}

private resetTimer(): void {
  this.startedAtValue = null;
  this.pausedAtValue = null;
  this.pausedTotalMs = 0;
  this.ended = false;
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- session.test.ts
```

### Task 5 - Add Presentation Control Bar

Goal: provide a Google Slides-style bottom-left control bar that fades in on mousemove and fades out after 3 seconds.

Files:

- `packages/peitho-present/src/controls.ts`
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/controls.test.ts`

Test:

```ts
// packages/peitho-present/test/controls.test.ts
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installPresentationControls } from "../src/index";

const cleanups: Array<() => void> = [];

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.useRealTimers();
  document.body.replaceChildren();
});

it("shows controls on mousemove and hides them after the idle timeout", () => {
  const root = document.createElement("main");
  document.body.appendChild(root);
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    idleMs: 3000
  });
  cleanups.push(cleanup);

  const bar = root.querySelector<HTMLElement>('[data-peitho-control-bar="true"]');
  expect(bar?.hidden).toBe(true);

  window.dispatchEvent(new MouseEvent("mousemove"));
  expect(bar?.hidden).toBe(false);
  vi.advanceTimersByTime(2999);
  expect(bar?.hidden).toBe(false);
  vi.advanceTimersByTime(1);
  expect(bar?.hidden).toBe(true);
});

it("dispatches navigate requests and updates the counter from slidechange", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  window.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installPresentationControls({ root, window, document, bus: window });
  cleanups.push(cleanup);

  expect(root.querySelector('[data-peitho-control="counter"]')?.textContent).toBe("– / –");
  root.querySelector<HTMLButtonElement>('[data-peitho-action="prev"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="next"]')?.click();
  window.dispatchEvent(
    new CustomEvent("peitho:slidechange", {
      detail: { key: "arch-1", index: 2, total: 12, previousIndex: 1 }
    })
  );

  expect(requests).toEqual([{ to: "prev" }, { to: "next" }]);
  expect(root.querySelector('[data-peitho-control="counter"]')?.textContent).toBe("3 / 12");
});

it("opens presenter.html from the presenter button", () => {
  const root = document.createElement("main");
  const openPresenter = vi.fn();
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    openPresenter
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="presenter"]')?.click();

  expect(openPresenter).toHaveBeenCalledTimes(1);
});
```

Implementation:

```ts
// packages/peitho-present/src/controls.ts
import type { NavigateDetail, SlideChangeDetail } from "./shell";

export type PresentationControlsOptions = {
  root: HTMLElement;
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  idleMs?: number;
  openPresenter?: () => void;
};

export function installPresentationControls(options: PresentationControlsOptions): () => void {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
  const idleMs = options.idleMs ?? 3000;
  const openPresenter =
    options.openPresenter ??
    (() => {
      win.open("presenter.html", "_blank", "noopener");
    });

  const bar = doc.createElement("nav");
  bar.dataset.peithoControlBar = "true";
  bar.className = "peitho-control-bar";
  bar.hidden = true;
  bar.innerHTML = [
    '<button type="button" data-peitho-action="prev" aria-label="Previous slide">◀</button>',
    '<button type="button" data-peitho-action="next" aria-label="Next slide">▶</button>',
    '<output data-peitho-control="counter">– / –</output>',
    '<button type="button" data-peitho-action="fullscreen" aria-label="Toggle fullscreen">⛶</button>',
    '<button type="button" data-peitho-action="presenter">Presenter</button>'
  ].join("");
  options.root.appendChild(bar);

  let hideTimer: number | null = null;
  const clearHideTimer = (): void => {
    if (hideTimer !== null) win.clearTimeout(hideTimer);
    hideTimer = null;
  };
  const show = (): void => {
    bar.hidden = false;
    clearHideTimer();
    hideTimer = win.setTimeout(() => {
      bar.hidden = true;
      hideTimer = null;
    }, idleMs);
  };
  const dispatchNavigate = (to: "prev" | "next"): void => {
    bus.dispatchEvent(new CustomEvent<NavigateDetail>("peitho:navigate", { detail: { to } }));
  };
  const onClick = (event: Event): void => {
    event.stopPropagation();
    const action = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-peitho-action]")
      ?.dataset.peithoAction;
    if (action === "prev" || action === "next") dispatchNavigate(action);
    if (action === "presenter") openPresenter();
    if (action === "fullscreen") toggleFullscreen(doc);
  };
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<SlideChangeDetail>).detail;
    const counter = bar.querySelector<HTMLOutputElement>('[data-peitho-control="counter"]');
    if (counter) counter.textContent = `${detail.index + 1} / ${detail.total}`;
  };

  win.addEventListener("mousemove", show);
  bar.addEventListener("click", onClick);
  bus.addEventListener("peitho:slidechange", onSlideChange);

  return () => {
    clearHideTimer();
    win.removeEventListener("mousemove", show);
    bar.removeEventListener("click", onClick);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bar.remove();
  };
}

export function toggleFullscreen(doc: Document = document): void {
  if (doc.fullscreenElement) {
    void doc.exitFullscreen?.();
    return;
  }
  void doc.documentElement.requestFullscreen?.();
}
```

```ts
// packages/peitho-present/src/index.ts
export {
  installPresentationControls,
  toggleFullscreen
} from "./controls";
export type { PresentationControlsOptions } from "./controls";
```

Verification:

```sh
cd packages/peitho-present
npm test -- controls.test.ts
```

### Task 6 - Add Click Navigation and Fullscreen Shortcut UI

Goal: canvas-area clicks and the `f` key are UI-only behaviors. They emit navigation or call fullscreen APIs without touching shell internals.

Files:

- `packages/peitho-present/src/controls.ts`
- `packages/peitho-present/test/controls.test.ts`

Test:

```ts
// packages/peitho-present/test/controls.test.ts
import {
  installCanvasClickNavigation,
  installFullscreenShortcut
} from "../src/index";

it("clicks in the left viewport quarter request prev and other canvas clicks request next", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  vi.spyOn(window, "innerWidth", "get").mockReturnValue(1000);
  window.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanup = installCanvasClickNavigation({ root, window, bus: window });
  cleanups.push(cleanup);

  root.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 100 }));
  root.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 250 }));
  root.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 900 }));

  expect(requests).toEqual([{ to: "prev" }, { to: "next" }, { to: "next" }]);
});

it("does not navigate when a click starts inside the control bar", () => {
  const root = document.createElement("main");
  const requests: unknown[] = [];
  window.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });
  const cleanupA = installCanvasClickNavigation({ root, window, bus: window });
  const cleanupB = installPresentationControls({ root, window, document, bus: window });
  cleanups.push(cleanupA, cleanupB);

  root
    .querySelector<HTMLElement>('[data-peitho-control-bar="true"]')
    ?.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 900 }));

  expect(requests).toEqual([]);
});

it("toggles fullscreen from the f key through browser APIs", () => {
  const requestFullscreen = vi.fn();
  const exitFullscreen = vi.fn();
  Object.defineProperty(document.documentElement, "requestFullscreen", {
    value: requestFullscreen,
    configurable: true
  });
  Object.defineProperty(document, "exitFullscreen", {
    value: exitFullscreen,
    configurable: true
  });
  Object.defineProperty(document, "fullscreenElement", {
    value: null,
    configurable: true
  });
  const cleanup = installFullscreenShortcut({ window, document });
  cleanups.push(cleanup);

  window.dispatchEvent(new KeyboardEvent("keydown", { key: "f" }));

  expect(requestFullscreen).toHaveBeenCalledTimes(1);
});
```

Implementation:

```ts
// packages/peitho-present/src/controls.ts
export type CanvasClickNavigationOptions = {
  root: HTMLElement;
  window?: Window;
  bus?: EventTarget;
};

export function installCanvasClickNavigation(options: CanvasClickNavigationOptions): () => void {
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const onClick = (event: MouseEvent): void => {
    if ((event.target as HTMLElement).closest('[data-peitho-control-bar="true"]')) return;
    const to = event.clientX < win.innerWidth / 4 ? "prev" : "next";
    bus.dispatchEvent(new CustomEvent<NavigateDetail>("peitho:navigate", { detail: { to } }));
  };
  options.root.addEventListener("click", onClick);
  return () => options.root.removeEventListener("click", onClick);
}

export type FullscreenShortcutOptions = {
  window?: Window;
  document?: Document;
};

export function installFullscreenShortcut(options: FullscreenShortcutOptions = {}): () => void {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key !== "f") return;
    event.preventDefault();
    toggleFullscreen(doc);
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
```

Update the export:

```ts
// packages/peitho-present/src/index.ts
export {
  installCanvasClickNavigation,
  installFullscreenShortcut,
  installPresentationControls,
  toggleFullscreen
} from "./controls";
export type {
  CanvasClickNavigationOptions,
  FullscreenShortcutOptions,
  PresentationControlsOptions
} from "./controls";
```

Verification:

```sh
cd packages/peitho-present
npm test -- controls.test.ts
```

### Task 7 - Wire Present HTML to Shell UI Components

Goal: generated `present.html` loads the shell plus keyboard, sync, control bar, click navigation, and fullscreen shortcut. It no longer contains the static Presenter view link.

Files:

- `crates/peitho-core/src/render.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn present_index_mounts_shell_controls_keyboard_sync_and_notes() {
    let html = render_present_index();

    assert!(html.contains(r#"<main id="peitho-present-root"></main>"#));
    assert!(html.contains("installPresentationControls"));
    assert!(html.contains("installCanvasClickNavigation"));
    assert!(html.contains("installFullscreenShortcut"));
    assert!(html.contains("installKeyboardNavigation(window)"));
    assert!(html.contains("installSyncBridge(window)"));
    assert!(html.contains("fetchOk('notes.json')"));
    assert!(html.contains("await mountPresentShell({ root })"));
    let controls_index = html
        .find("installPresentationControls({ root, window, document })")
        .unwrap();
    let mount_index = html.find("await mountPresentShell({ root })").unwrap();
    assert!(controls_index < mount_index);
    assert!(!html.contains("peitho-presenter-link"));
    assert!(!html.contains(">Presenter view</a>"));
}
```

```rust
// crates/peitho/tests/present.rs
#[test]
fn repository_example_present_no_serve_smoke() {
    let cache = workspace_root().join(".peitho/present-cache");
    let present_html = std::fs::read_to_string(cache.join("present.html")).unwrap();
    let shell_js = std::fs::read_to_string(cache.join("shell.js")).unwrap();

    assert!(present_html.contains("installPresentationControls"));
    assert!(present_html.contains("installCanvasClickNavigation"));
    assert!(present_html.contains("installFullscreenShortcut"));
    assert!(!present_html.contains("peitho-presenter-link"));
    assert!(shell_js.contains("installPresentationControls"));
}
```

Implementation:

```rust
// crates/peitho-core/src/render.rs
pub fn render_present_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Present</title>
  <style>
    html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; }
    #peitho-present-root { position: fixed; inset: 0; overflow: hidden; background: #000; }
    .peitho-control-bar { position: fixed; left: 16px; bottom: 16px; z-index: 10; display: flex; gap: 8px; align-items: center; padding: 8px; background: rgba(0, 0, 0, 0.72); color: #fff; border-radius: 6px; }
    .peitho-control-bar[hidden] { display: none; }
  </style>
</head>
<body>
  <main id="peitho-present-root"></main>
  <script type="module">
    import {
      installCanvasClickNavigation,
      installFullscreenShortcut,
      installKeyboardNavigation,
      installPresentationControls,
      installSyncBridge,
      mountPresentShell
    } from './shell.js';

    function showError(message) {
      const root = document.getElementById('peitho-present-root');
      root.textContent = message;
    }

    async function fetchOk(url) {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
      return response;
    }

    async function main() {
      const root = document.getElementById('peitho-present-root');
      try {
        window.peithoNotes = await fetchOk('notes.json').then((response) => response.json());
        installKeyboardNavigation(window);
        installSyncBridge(window);
        installPresentationControls({ root, window, document });
        installCanvasClickNavigation({ root, window });
        installFullscreenShortcut({ window, document });
        await mountPresentShell({ root });
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#
        .to_owned()
}
```

Verification:

```sh
cargo test -p peitho-core render::tests::present_index_mounts_shell_controls_keyboard_sync_and_notes
```

### Task 8 - Update Presenter View for Manual Start

Goal: presenter UI gains a Start button that dispatches `peitho:timercontrol` only; timer display reads shell getters and shows `00:00` before start.

Files:

- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/test/presenter.test.ts`

Test:

```ts
// packages/peitho-present/test/presenter.test.ts
it("shows 00:00 before start and the Start button emits timercontrol", async () => {
  let now = 1000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);
  const events: unknown[] = [];
  const onTimerControl = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  window.addEventListener("peitho:timercontrol", onTimerControl);
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));

  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("00:00");
  root.querySelector<HTMLButtonElement>('[data-peitho-action="start"]')?.click();
  now = 65000;
  view.tick();

  expect(events).toEqual([{ action: "start" }]);
  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("01:04");
});

it("buttons emit navigate and timercontrol requests only", async () => {
  const root = document.createElement("main");
  const { channel, factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);
  const events: unknown[] = [];
  const onNavigate = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  const onTimerControl = (event: Event): void => {
    events.push((event as CustomEvent).detail);
  };
  window.addEventListener("peitho:navigate", onNavigate);
  window.addEventListener("peitho:timercontrol", onTimerControl);
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));

  root.querySelector<HTMLButtonElement>('[data-peitho-action="next"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="start"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="pause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="reset"]')?.click();

  expect(events).toEqual([
    { to: "next" },
    { action: "start" },
    { action: "pause" },
    { action: "reset" }
  ]);
  expect(channel.sent).toEqual([{ index: 1 }]);
});
```

Implementation:

```ts
// packages/peitho-present/src/presenter.ts
options.root.innerHTML = `
  <section class="peitho-presenter">
    <div data-peitho-presenter="current"></div>
    <aside>
      <div data-peitho-presenter="preview"></div>
      <p data-peitho-presenter="preview-end" hidden>End of deck</p>
      <section data-peitho-presenter="notes"></section>
      <output data-peitho-presenter="timer">00:00</output>
      <div class="peitho-presenter-controls">
        <button type="button" data-peitho-action="prev">Prev</button>
        <button type="button" data-peitho-action="next">Next</button>
        <button type="button" data-peitho-action="start">Start</button>
        <button type="button" data-peitho-action="pause">Pause</button>
        <button type="button" data-peitho-action="resume">Resume</button>
        <button type="button" data-peitho-action="reset">Reset</button>
      </div>
    </aside>
  </section>`;

for (const action of ["start", "pause", "resume", "reset"] as const) {
  options.root.querySelector(`[data-peitho-action="${action}"]`)?.addEventListener("click", () => {
    bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action } }));
    tick();
  });
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- presenter.test.ts
```

### Task 9 - Scale Presenter Current and Next Preview Panes

Goal: both presenter panes use the same `1280x720` shell canvas, scaled to pane dimensions instead of the full window.

Files:

- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/test/presenter.test.ts`

Test:

```ts
// packages/peitho-present/test/presenter.test.ts
function sizeElement(element: HTMLElement, width: number, height: number): void {
  Object.defineProperty(element, "clientWidth", { value: width, configurable: true });
  Object.defineProperty(element, "clientHeight", { value: height, configurable: true });
}

it("scales current and next preview shells to their pane sizes", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  const currentPane = root.querySelector<HTMLElement>('[data-peitho-presenter="current"]')!;
  const previewPane = root.querySelector<HTMLElement>('[data-peitho-presenter="preview"]')!;
  sizeElement(currentPane, 640, 360);
  sizeElement(previewPane, 320, 180);
  window.dispatchEvent(new Event("resize"));

  expect(currentPane.querySelector<HTMLElement>(".peitho-slide")?.style.transform).toBe(
    "translate(0px, 0px) scale(0.5)"
  );
  expect(previewPane.querySelector<HTMLElement>(".peitho-slide")?.style.transform).toBe(
    "translate(0px, 0px) scale(0.25)"
  );
});
```

Implementation:

```ts
// packages/peitho-present/src/presenter.ts
function paneViewport(pane: HTMLElement): () => { width: number; height: number } {
  return () => ({
    width: pane.clientWidth,
    height: pane.clientHeight
  });
}

const mainShell = await mountPresentShell({
  root: currentRoot,
  fetcher,
  window: win,
  document: doc,
  bus,
  now,
  viewport: paneViewport(currentRoot)
});
const previewShell = await mountPresentShell({
  root: previewRoot,
  fetcher,
  window: win,
  document: doc,
  bus: previewBus,
  now,
  viewport: paneViewport(previewRoot)
});
```

Add pane classes in the existing template:

```ts
options.root.innerHTML = `
  <section class="peitho-presenter">
    <div class="peitho-presenter-pane" data-peitho-presenter="current"></div>
    <aside>
      <div class="peitho-presenter-pane" data-peitho-presenter="preview"></div>
      <p data-peitho-presenter="preview-end" hidden>End of deck</p>
      <section data-peitho-presenter="notes"></section>
      <output data-peitho-presenter="timer">00:00</output>
      <div class="peitho-presenter-controls">
        <button type="button" data-peitho-action="prev">Prev</button>
        <button type="button" data-peitho-action="next">Next</button>
        <button type="button" data-peitho-action="start">Start</button>
        <button type="button" data-peitho-action="pause">Pause</button>
        <button type="button" data-peitho-action="resume">Resume</button>
        <button type="button" data-peitho-action="reset">Reset</button>
      </div>
    </aside>
  </section>`;
```

Verification:

```sh
cd packages/peitho-present
npm test -- presenter.test.ts
```

### Task 10 - Update Presenter HTML Shell

Goal: generated `presenter.html` provides a simple two-column presenter layout with black scaled panes and still delegates behavior to `mountPresenterView`.

Files:

- `crates/peitho-core/src/render.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn presenter_index_mounts_presenter_view_with_canvas_panes_and_notes() {
    let html = render_presenter_index();

    assert!(html.contains(r#"<main id="peitho-presenter-root"></main>"#));
    assert!(html.contains(r#"import { mountPresenterView } from './shell.js';"#));
    assert!(html.contains("fetchOk('notes.json')"));
    assert!(html.contains("await mountPresenterView({ root, notes })"));
    assert!(html.contains(".peitho-presenter-pane"));
    assert!(html.contains("overflow: hidden"));
    assert!(html.contains("Failed to load"));
    assert!(!html.contains("fetchOk(slide.src)"));
}
```

Implementation:

```rust
// crates/peitho-core/src/render.rs
pub fn render_presenter_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Presenter</title>
  <style>
    html, body { margin: 0; width: 100%; min-height: 100%; background: #111; color: #f5f5f5; }
    body { font: 14px system-ui, sans-serif; }
    #peitho-presenter-root { min-height: 100vh; }
    .peitho-presenter { display: grid; grid-template-columns: minmax(0, 2fr) minmax(320px, 1fr); gap: 16px; padding: 16px; box-sizing: border-box; min-height: 100vh; }
    .peitho-presenter-pane { position: relative; overflow: hidden; background: #000; min-height: 180px; }
    [data-peitho-presenter="current"] { min-height: calc(100vh - 32px); }
    [data-peitho-presenter="preview"] { aspect-ratio: 16 / 9; }
    [data-peitho-presenter="notes"] { white-space: pre-wrap; line-height: 1.5; margin-top: 16px; }
    [data-peitho-presenter="timer"] { display: block; font-size: 40px; font-variant-numeric: tabular-nums; margin: 16px 0; }
    .peitho-presenter-controls { display: flex; flex-wrap: wrap; gap: 8px; }
  </style>
</head>
<body>
  <main id="peitho-presenter-root"></main>
  <script type="module">
    import { mountPresenterView } from './shell.js';

    function showError(message) {
      const root = document.getElementById('peitho-presenter-root');
      root.textContent = message;
    }

    async function fetchOk(url) {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
      return response;
    }

    async function main() {
      const root = document.getElementById('peitho-presenter-root');
      try {
        const notes = await fetchOk('notes.json').then((response) => response.json());
        await mountPresenterView({ root, notes });
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#
        .to_owned()
}
```

Verification:

```sh
cargo test -p peitho-core render::tests::presenter_index_mounts_presenter_view_with_canvas_panes_and_notes
```

### Task 11 - Replace Distribution Index with One-Slide Canvas Navigation

Goal: `dist/index.html` shows one light-DOM slide at a time in a scaled `1280x720` canvas, without loading `shell.js` or showing presentation controls.

Files:

- `crates/peitho-core/src/render.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn distribution_index_uses_one_slide_canvas_without_shell_bundle() {
    let html = render_distribution_index();

    assert!(html.contains(r#"<link rel="stylesheet" href="peitho.css">"#));
    assert!(html.contains(r#"id="peitho-canvas""#));
    assert!(html.contains("const CANVAS_WIDTH = 1280"));
    assert!(html.contains("const CANVAS_HEIGHT = 720"));
    assert!(html.contains("function resizeCanvas()"));
    assert!(html.contains("function showSlide(index)"));
    assert!(html.contains("document.addEventListener('keydown'"));
    assert!(html.contains("document.addEventListener('click'"));
    assert!(html.contains("fetchOk('manifest.json')"));
    assert!(html.contains("fetchOk(slide.src)"));
    assert!(html.contains("response.ok"));
    assert!(!html.contains("shell.js"));
    assert!(!html.contains("installPresentationControls"));
    assert!(!html.contains("data-slide-key="));
}
```

Implementation:

```rust
// crates/peitho-core/src/render.rs
pub fn render_distribution_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="peitho.css">
  <title>Peitho Deck</title>
  <style>
    html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; }
    #peitho-slides { position: fixed; inset: 0; overflow: hidden; background: #000; }
    #peitho-canvas { position: absolute; left: 0; top: 0; width: 1280px; height: 720px; transform-origin: top left; }
  </style>
</head>
<body>
  <main id="peitho-slides">
    <div id="peitho-canvas"></div>
  </main>
  <script>
    const CANVAS_WIDTH = 1280;
    const CANVAS_HEIGHT = 720;
    let slides = [];
    let currentIndex = 0;

    function showError(message) {
      const root = document.getElementById('peitho-slides');
      root.textContent = message;
    }

    async function fetchOk(url) {
      const response = await fetch(url);
      if (!response.ok) {
        throw new Error(`Failed to load ${url}: ${response.status}`);
      }
      return response;
    }

    function resizeCanvas() {
      const canvas = document.getElementById('peitho-canvas');
      const scale = Math.min(window.innerWidth / CANVAS_WIDTH, window.innerHeight / CANVAS_HEIGHT);
      const width = CANVAS_WIDTH * scale;
      const height = CANVAS_HEIGHT * scale;
      const left = (window.innerWidth - width) / 2;
      const top = (window.innerHeight - height) / 2;
      canvas.style.transform = `translate(${left}px, ${top}px) scale(${scale})`;
    }

    function showSlide(index) {
      if (slides.length === 0) {
        document.getElementById('peitho-canvas').replaceChildren();
        return;
      }
      const next = Math.max(0, Math.min(index, slides.length - 1));
      currentIndex = next;
      const canvas = document.getElementById('peitho-canvas');
      canvas.innerHTML = slides[next].html;
    }

    function navigate(to) {
      if (to === 'next') showSlide(currentIndex + 1);
      if (to === 'prev') showSlide(currentIndex - 1);
      if (to === 'first') showSlide(0);
      if (to === 'last') showSlide(slides.length - 1);
    }

    async function loadDeck() {
      try {
        const manifest = await fetchOk('manifest.json').then((response) => response.json());
        document.title = manifest.title || 'Peitho Deck';
        slides = await Promise.all(
          manifest.slides.map(async (slide) => ({
            key: slide.key,
            html: await fetchOk(slide.src).then((response) => response.text())
          }))
        );
        showSlide(0);
        resizeCanvas();
      } catch (error) {
        showError(error.message);
      }
    }

    document.addEventListener('keydown', (event) => {
      if (event.key === 'ArrowRight' || event.key === 'PageDown' || event.key === ' ') {
        event.preventDefault();
        navigate('next');
      }
      if (event.key === 'ArrowLeft' || event.key === 'PageUp') {
        event.preventDefault();
        navigate('prev');
      }
      if (event.key === 'Home') navigate('first');
      if (event.key === 'End') navigate('last');
    });
    document.addEventListener('click', (event) => {
      navigate(event.clientX < window.innerWidth / 4 ? 'prev' : 'next');
    });
    window.addEventListener('resize', resizeCanvas);
    loadDeck();
  </script>
</body>
</html>"#
        .to_owned()
}
```

Verification:

```sh
cargo test -p peitho-core render::tests::distribution_index_uses_one_slide_canvas_without_shell_bundle
```

### Task 12 - Rewrite Base Theme for Fixed 1280x720 Slides

Goal: `themes/base.css` is authored against the logical canvas with pixel typography and no viewport-height layout.

Files:

- `themes/base.css`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn base_theme_targets_fixed_canvas_size() {
    let css = std::fs::read_to_string(workspace_root().join("themes/base.css")).unwrap();

    assert!(css.contains("width: 1280px;"));
    assert!(css.contains("height: 720px;"));
    assert!(css.contains("font-size: 56px;"));
    assert!(!css.contains("min-height: 100vh"));
    assert!(!css.contains("font-size: 1.4rem"));
}
```

Implementation:

```css
/* themes/base.css */
.peitho-slide {
  width: 1280px;
  height: 720px;
  display: grid;
  grid-template-columns: minmax(0, 1fr) 470px;
  grid-template-rows: auto minmax(0, 1fr);
  column-gap: 56px;
  row-gap: 32px;
  padding: 64px 72px;
  box-sizing: border-box;
  overflow: hidden;
  background: #f7f7f2;
  color: #181818;
  font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

.peitho-slide h1 {
  grid-column: 1 / -1;
  margin: 0;
  font-size: 56px;
  line-height: 1.05;
  font-weight: 760;
}

.slot-title {
  display: inline-block;
}

.body {
  min-width: 0;
  overflow: hidden;
}

.slot-body {
  font-size: 30px;
  line-height: 1.35;
}

.slot-body p {
  margin: 0 0 24px;
}

.slot-body ul {
  margin: 0;
  padding-left: 34px;
}

.code {
  min-width: 0;
  margin: 0;
}

.slot-code {
  display: block;
  margin: 0;
  padding: 24px;
  max-height: 100%;
  box-sizing: border-box;
  overflow: hidden;
  background: #111;
  color: #f7f7f7;
  font-size: 22px;
  line-height: 1.35;
}
```

Verification:

```sh
cargo test -p peitho --test build base_theme_targets_fixed_canvas_size
```

### Task 13 - Update Shell and Keyboard Regression Tests for Canvas Semantics

Goal: keep existing shell tests aligned with manual session start and fixed canvas behavior while preserving prior guarantees.

Files:

- `packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts`
- `packages/peitho-present/test/session.test.ts`

Test changes:

```ts
// packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts
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

it("keeps keyboard as a navigate-event UI component", () => {
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
```

Implementation:

- Remove assertions that expect mount-time `peitho:presentationstart`.
- Keep assertions for visible fetch errors, Shadow DOM CSS injection, handle attributes, invalid navigation logging, and no-op navigation.
- Keep all `afterEach` cleanup arrays so shell instances and window listeners do not leak across tests.

Verification:

```sh
cd packages/peitho-present
npm test -- loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts session.test.ts
```

### Task 14 - Update Present Cache Smoke for M7 Output

Goal: `peitho present --no-serve --no-open` emits the new HTML and bundled symbols without reintroducing static presenter links.

Files:

- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
#[test]
fn repository_example_present_no_serve_smoke() {
    let root = workspace_root();
    let shell = root.join("packages/peitho-present/dist/shell.js");
    assert!(
        shell.exists(),
        "shell bundle not built; run npm run build in packages/peitho-present"
    );

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(&root)
        .args([
            "present",
            "examples/deck.md",
            "--template",
            "templates/title-body-code.html",
            "--base-css",
            "themes/base.css",
            "--overrides-css",
            "themes/overrides.css",
            "--no-serve",
            "--no-open",
        ])
        .assert()
        .success();

    let cache = root.join(".peitho/present-cache");
    let present_html = std::fs::read_to_string(cache.join("present.html")).unwrap();
    let presenter_html = std::fs::read_to_string(cache.join("presenter.html")).unwrap();
    let shell_js = std::fs::read_to_string(cache.join("shell.js")).unwrap();

    assert!(cache.join("slides").is_dir());
    assert!(cache.join("manifest.json").exists());
    assert!(cache.join("notes.json").exists());
    assert!(cache.join("peitho.css").exists());
    assert!(present_html.contains("installPresentationControls"));
    assert!(present_html.contains("installCanvasClickNavigation"));
    assert!(present_html.contains("installFullscreenShortcut"));
    assert!(!present_html.contains("peitho-presenter-link"));
    assert!(presenter_html.contains("mountPresenterView"));
    assert!(presenter_html.contains(".peitho-presenter-pane"));
    assert!(shell_js.contains("CANVAS_WIDTH"));
    assert!(shell_js.contains("installPresentationControls"));
    assert!(shell_js.contains("mountPresenterView"));
}
```

Implementation:

- Merge these assertions into the existing repository-root present smoke test.
- Do not add a second repository-root smoke test that writes `.peitho/present-cache/`.

Verification:

```sh
cargo test -p peitho --test present repository_example_present_no_serve_smoke
```

### Task 15 - Verify Public TS API Exports

Goal: the bundled `shell.js` entry can import every M7 UI function from the public package entry.

Files:

- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/generated.test.ts`

Test:

```ts
// packages/peitho-present/test/generated.test.ts
import { expect, it } from "vitest";
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  calculateCanvasFit,
  installCanvasClickNavigation,
  installCanvasScaler,
  installFullscreenShortcut,
  installPresentationControls,
  mountPresenterView,
  mountPresentShell
} from "../src/index";

it("exports the presentation canvas public API", () => {
  expect(CANVAS_WIDTH).toBe(1280);
  expect(CANVAS_HEIGHT).toBe(720);
  expect(calculateCanvasFit({ width: 1280, height: 720 }).scale).toBe(1);
  expect(typeof installCanvasScaler).toBe("function");
  expect(typeof installPresentationControls).toBe("function");
  expect(typeof installCanvasClickNavigation).toBe("function");
  expect(typeof installFullscreenShortcut).toBe("function");
  expect(typeof mountPresentShell).toBe("function");
  expect(typeof mountPresenterView).toBe("function");
});
```

Implementation:

```ts
// packages/peitho-present/src/index.ts
export {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  calculateCanvasFit,
  installCanvasScaler
} from "./canvas";
export type {
  CanvasFit,
  CanvasScalerOptions,
  CanvasViewport
} from "./canvas";
export {
  installCanvasClickNavigation,
  installFullscreenShortcut,
  installPresentationControls,
  toggleFullscreen
} from "./controls";
export type {
  CanvasClickNavigationOptions,
  FullscreenShortcutOptions,
  PresentationControlsOptions
} from "./controls";
export { installKeyboardNavigation } from "./keyboard";
export { mountPresenterView } from "./presenter";
export { mountPresentShell } from "./shell";
export { installSyncBridge } from "./sync";
export type { PresenterOptions, PresenterView } from "./presenter";
export type {
  NavigateDetail,
  NavigateTarget,
  PresentShell,
  PresentationEndDetail,
  PresentationStartDetail,
  ShellOptions,
  SlideChangeDetail,
  TimerControlDetail
} from "./shell";
export type { SyncChannel, SyncChannelFactory, SyncMessage } from "./sync";
```

Verification:

```sh
cd packages/peitho-present
npm test -- generated.test.ts
npm run build
rg "installPresentationControls" dist/shell.js
rg "CANVAS_WIDTH" dist/shell.js
rg "mountPresenterView" dist/shell.js
```

## Final Verification Gate

Run the full gate after Tasks 1-15:

```sh
cd packages/peitho-present
npm run build
npm test
npm run typecheck
cd ../..
cargo test --workspace
cargo test --workspace
cargo test --workspace
git diff --exit-code bindings/
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p peitho -- present examples/deck.md --template templates/title-body-code.html --base-css themes/base.css --overrides-css themes/overrides.css --no-serve --no-open
cargo run -p peitho -- build examples/deck.md --template templates/title-body-code.html --base-css themes/base.css --overrides-css themes/overrides.css --out dist
```

Expected output markers:

```sh
test -f .peitho/present-cache/present.html
test -f .peitho/present-cache/presenter.html
test -f .peitho/present-cache/shell.js
test -f .peitho/present-cache/peitho.css
test -f .peitho/present-cache/manifest.json
test -f .peitho/present-cache/notes.json
test -d .peitho/present-cache/slides
rg "installPresentationControls" .peitho/present-cache/present.html
rg "installCanvasClickNavigation" .peitho/present-cache/present.html
rg "peitho-presenter-link" .peitho/present-cache/present.html && exit 1 || true
rg "mountPresenterView" .peitho/present-cache/presenter.html
rg "CANVAS_WIDTH" .peitho/present-cache/shell.js
rg "const CANVAS_WIDTH = 1280" dist/index.html
rg "shell.js" dist/index.html && exit 1 || true
rg "width: 1280px;" themes/base.css
rg "min-height: 100vh" themes/base.css && exit 1 || true
```

Final status command:

```sh
git status --short
```
