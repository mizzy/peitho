import type { Manifest } from "../../../bindings/Manifest";
import type { ManifestSlide } from "../../../bindings/ManifestSlide";
import { installCanvasScaler, type CanvasViewport } from "./canvas";
import { installDocumentFontScope } from "./fontscope";
import { nextNonSkippedIndex } from "./skipnav";

export type NavigateTarget =
  | "next"
  | "prev"
  | "first"
  | "last"
  | { key: string }
  | { index: number };

export type NavigateDetail = { to: NavigateTarget };

export type SlideChangeDetail = {
  key: string;
  index: number;
  total: number;
  previousIndex: number | null;
};

export type PresentationStartDetail = { total: number; startedAt: number };
export type PresentationEndDetail = { endedAt: number; elapsedMs: number };
export type TimerControlDetail = { action: "start" | "pause" | "resume" | "reset" };

export type PresentShell = {
  manifest: Manifest | null;
  currentIndex: number;
  navigate(to: NavigateTarget): void;
  elapsedMs(): number;
  isPaused(): boolean;
  startedAt(): number | null;
  destroy(): void;
};

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

type CanvasDimensions = {
  width: number;
  height: number;
};

export type SlideView = {
  meta: ManifestSlide;
  host: HTMLElement;
};

export async function mountPresentShell(options: ShellOptions): Promise<PresentShell> {
  const shell = new PresentShellController(options);
  await shell.load();
  return shell;
}

class PresentShellController implements PresentShell {
  manifest: Manifest | null = null;
  currentIndex = -1;
  private readonly slides: SlideView[] = [];
  private readonly root: HTMLElement;
  private readonly fetcher: typeof fetch;
  private readonly win: Window;
  private readonly doc: Document;
  private readonly log: Pick<Console, "error">;
  private readonly bus: EventTarget;
  private readonly now: () => number;
  private readonly viewport?: () => CanvasViewport;
  private readonly canvasCleanups: Array<() => void> = [];
  private fontScopeCleanup: (() => void) | null = null;
  private startedAtValue: number | null = null;
  private pausedAtValue: number | null = null;
  private pausedTotalMs = 0;
  private ended = false;
  private readonly onNavigate = (event: Event): void => {
    const detail = (event as CustomEvent<NavigateDetail>).detail;
    if (!detail || !("to" in detail)) {
      this.log.error("Invalid peitho:navigate event");
      return;
    }
    this.navigate(detail.to);
  };
  private readonly onTimerControl = (event: Event): void => {
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action === "start") this.startPresentation();
    else if (action === "pause") this.pauseTimer();
    else if (action === "resume") this.resumeTimer();
    else if (action === "reset") this.resetTimer();
    else this.log.error("Invalid peitho:timercontrol event");
  };
  private readonly onPageHide = (): void => this.endPresentation();

  constructor(options: ShellOptions) {
    this.root = options.root;
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.log = options.console ?? console;
    this.bus = options.bus ?? this.win;
    this.now = options.now ?? Date.now;
    this.viewport = options.viewport;
    this.root.classList.add("peitho-shell-viewport");
    const rootPosition = this.win.getComputedStyle(this.root).position;
    if (rootPosition === "static" || rootPosition === "") {
      this.root.style.position = "relative";
    }
    this.root.style.overflow = "hidden";
    this.root.style.background = "#000";
    this.bus.addEventListener("peitho:navigate", this.onNavigate);
    this.bus.addEventListener("peitho:timercontrol", this.onTimerControl);
    this.win.addEventListener("pagehide", this.onPageHide);
  }

  async load(): Promise<void> {
    try {
      const manifest = await this.fetchJson<Manifest>("manifest.json");
      const dimensions = {
        width: manifest.canvasWidth,
        height: manifest.canvasHeight
      };
      const cssAspect = manifest.aspectRatio.replace(":", " / ");
      this.setCanvasRootProperties(dimensions, cssAspect);
      const css = await this.fetchText("peitho.css");
      this.fontScopeCleanup = installDocumentFontScope(this.doc, css);
      const pending: SlideView[] = [];
      for (const slide of manifest.slides) {
        const html = await this.fetchText(slide.src);
        const host = this.createSlideHost(slide, html, css, dimensions);
        pending.push({ meta: slide, host });
      }
      this.manifest = manifest;
      for (const view of pending) {
        this.root.appendChild(view.host);
        this.slides.push(view);
      }
      this.show(nextNonSkippedIndex(pending.map((view) => view.meta), -1, 1) ?? 0);
    } catch (error) {
      this.clearCanvasRootProperties();
      this.root.replaceChildren();
      this.root.textContent = error instanceof Error ? error.message : String(error);
    }
  }

  navigate(to: NavigateTarget): void {
    const index = this.resolveTarget(to);
    if (index === null) return;
    this.show(index);
  }

  elapsedMs(): number {
    if (this.startedAtValue === null) return 0;
    const current = this.now();
    const pausedNow = this.pausedAtValue === null ? 0 : current - this.pausedAtValue;
    return Math.max(0, current - this.startedAtValue - this.pausedTotalMs - pausedNow);
  }

  isPaused(): boolean {
    return this.pausedAtValue !== null;
  }

  startedAt(): number | null {
    return this.startedAtValue;
  }

  destroy(): void {
    this.endPresentation();
    this.fontScopeCleanup?.();
    this.fontScopeCleanup = null;
    while (this.canvasCleanups.length > 0) this.canvasCleanups.pop()?.();
    this.clearCanvasRootProperties();
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:timercontrol", this.onTimerControl);
    this.win.removeEventListener("pagehide", this.onPageHide);
  }

  private async fetchJson<T>(url: string): Promise<T> {
    const response = await this.fetchOk(url);
    return response.json() as Promise<T>;
  }

  private async fetchText(url: string): Promise<string> {
    const response = await this.fetchOk(url);
    return response.text();
  }

  private async fetchOk(url: string): Promise<Response> {
    const response = await this.fetcher(url);
    if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
    return response;
  }

  private setCanvasRootProperties(dimensions: CanvasDimensions, cssAspect: string): void {
    this.root.style.setProperty("--peitho-canvas-width", `${dimensions.width}px`);
    this.root.style.setProperty("--peitho-canvas-height", `${dimensions.height}px`);
    this.root.style.setProperty("--peitho-canvas-aspect", cssAspect);
  }

  private clearCanvasRootProperties(): void {
    this.root.style.removeProperty("--peitho-canvas-width");
    this.root.style.removeProperty("--peitho-canvas-height");
    this.root.style.removeProperty("--peitho-canvas-aspect");
  }

  private createSlideHost(
    slide: ManifestSlide,
    html: string,
    css: string,
    dimensions: CanvasDimensions
  ): HTMLElement {
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
        viewport: this.viewport,
        canvasWidth: dimensions.width,
        canvasHeight: dimensions.height
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

  private resolveTarget(to: NavigateTarget): number | null {
    if (to === "first") return 0;
    if (to === "last") return this.slides.length - 1;
    if (to === "next") return this.resolveSequentialTarget(1);
    if (to === "prev") return this.resolveSequentialTarget(-1);
    if ("index" in to) {
      if (to.index < 0 || to.index >= this.slides.length) {
        this.log.error(`Unknown slide index: ${to.index}`);
        return null;
      }
      return to.index;
    }
    const index = this.slides.findIndex((slide) => slide.meta.key === to.key);
    if (index < 0) {
      this.log.error(`Unknown slide key: ${to.key}`);
      return null;
    }
    return index;
  }

  private resolveSequentialTarget(direction: 1 | -1): number | null {
    return nextNonSkippedIndex(
      this.slides.map((slide) => slide.meta),
      this.currentIndex,
      direction
    );
  }

  private show(index: number): void {
    if (index < 0 || index >= this.slides.length) {
      this.log.error(`Unknown slide target: ${index}`);
      return;
    }
    if (index === this.currentIndex) return;

    this.slides.forEach((slide, slideIndex) => {
      slide.host.hidden = slideIndex !== index;
    });
    const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
    this.currentIndex = index;
    const slide = this.slides[index];
    this.bus.dispatchEvent(
      new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
        detail: {
          key: slide.meta.key,
          index: slide.meta.index,
          total: this.slides.length,
          previousIndex
        }
      })
    );
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

  private endPresentation(): void {
    if (this.ended || this.startedAtValue === null) return;
    const endedAt = this.now();
    const elapsedMs = this.elapsedMs();
    this.ended = true;
    this.bus.dispatchEvent(
      new CustomEvent<PresentationEndDetail>("peitho:presentationend", {
        detail: { endedAt, elapsedMs }
      })
    );
  }

  private pauseTimer(): void {
    if (this.startedAtValue === null || this.pausedAtValue !== null) return;
    this.pausedAtValue = this.now();
  }

  private resumeTimer(): void {
    if (this.pausedAtValue === null) return;
    this.pausedTotalMs += this.now() - this.pausedAtValue;
    this.pausedAtValue = null;
  }

  private resetTimer(): void {
    this.startedAtValue = null;
    this.pausedAtValue = null;
    this.pausedTotalMs = 0;
    this.ended = false;
  }
}
