import type { Manifest } from "../../../bindings/Manifest";
import type { ManifestSlide } from "../../../bindings/ManifestSlide";
import { calculateCanvasFit, type CanvasViewport } from "./canvas";
import { createClickNavigationGuard } from "./clickNavigationGuard";
import { installDocumentFontScope } from "./fontscope";
import { waitForFontsReady } from "./fontsReady";
import { hasChordModifier } from "./keyboard";
import type { NavigateTarget, SlideChangeDetail } from "./shell";
import { initialSlideIndex, nextNonSkippedIndex } from "./skipnav";
import {
  isGenerationSyncMessage,
  isSessionChangedSyncMessage,
  serverSyncChannelFactory,
  type SyncChannelFactory
} from "./sync";

export type PreviewMode = "single" | "grid";
export type OverviewRequestAction = "toggle" | "enter" | "exit" | "activate";
export type OverviewRequestDetail = { action: OverviewRequestAction };
export type PreviewNavigateTarget = NavigateTarget | "up" | "down";
type PreviewNavigateDetail = { to: PreviewNavigateTarget };

export type PreviewShell = {
  manifest: Manifest | null;
  currentIndex: number;
  selectedIndex: number;
  mode: PreviewMode;
  generation: number;
  navigate(to: PreviewNavigateTarget): void;
  saveState(): void;
  destroy(): void;
};

export type PreviewShellOptions = {
  root: HTMLElement;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  console?: Pick<Console, "error"> & Partial<Pick<Console, "warn">>;
  bus?: EventTarget;
  storage?: Storage;
  syncUrl?: string;
  viewport?: () => CanvasViewport;
};

type PreviewSlideView = {
  meta: ManifestSlide;
  tile: HTMLElement;
  host: HTMLElement;
};

type CanvasDimensions = {
  width: number;
  height: number;
};

type PreviewState = {
  mode: PreviewMode;
  index: number;
};

const PREVIEW_STATE_KEY = "peitho:preview-state";
const GRID_TILE_WIDTH = 320;
const GRID_GAP = 18;
const GRID_PADDING = 24;

export function previewGridColumnCount(rootWidth: number): number {
  const columns = Math.floor(
    (rootWidth - GRID_PADDING * 2 + GRID_GAP) / (GRID_TILE_WIDTH + GRID_GAP)
  );
  return Math.max(1, columns);
}

const previewNavigationKeyMap = new Map<string, PreviewNavigateTarget>([
  ["ArrowRight", "next"],
  ["PageDown", "next"],
  ["ArrowLeft", "prev"],
  ["PageUp", "prev"],
  ["ArrowUp", "up"],
  ["ArrowDown", "down"],
  ["Home", "first"],
  ["End", "last"]
]);
const verticalPreviewNavigationTargets = new Set<PreviewNavigateTarget>(["up", "down"]);

export function installPreviewKeyboard(
  win: Window = window,
  bus: EventTarget = win
): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    if (hasChordModifier(event)) return;
    if (event.key === "o") {
      event.preventDefault();
      dispatchOverviewRequest(bus, "toggle");
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      dispatchOverviewRequest(bus, "toggle");
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      dispatchOverviewRequest(bus, "activate");
      return;
    }
    const to = previewNavigationKeyMap.get(event.key);
    if (!to) return;
    const request = new CustomEvent<PreviewNavigateDetail>("peitho:navigate", {
      cancelable: true,
      detail: { to }
    });
    bus.dispatchEvent(request);
    if (!verticalPreviewNavigationTargets.has(to) || request.defaultPrevented) {
      event.preventDefault();
    }
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}

function dispatchOverviewRequest(bus: EventTarget, action: OverviewRequestAction): void {
  bus.dispatchEvent(
    new CustomEvent<OverviewRequestDetail>("peitho:overviewrequest", {
      detail: { action }
    })
  );
}

export async function mountPreviewShell(options: PreviewShellOptions): Promise<PreviewShell> {
  const shell = new PreviewShellController(options);
  await shell.load();
  return shell;
}

export function installPreviewReload(
  shell: Pick<PreviewShell, "generation" | "saveState">,
  channelFactory: SyncChannelFactory = serverSyncChannelFactory(),
  reload: () => void = () => window.location.reload()
): () => void {
  const channel = channelFactory("peitho-sync");
  channel.onmessage = (event: { data: unknown }): void => {
    if (isSessionChangedSyncMessage(event.data)) return;
    if (!isGenerationSyncMessage(event.data)) return;
    if (event.data.generation === shell.generation) return;
    shell.saveState();
    reload();
  };
  return () => {
    channel.onmessage = null;
    channel.close();
  };
}

class PreviewShellController implements PreviewShell {
  manifest: Manifest | null = null;
  currentIndex = -1;
  selectedIndex = -1;
  mode: PreviewMode = "single";
  generation = 0;
  private readonly root: HTMLElement;
  private readonly fetcher: typeof fetch;
  private readonly win: Window;
  private readonly doc: Document;
  private readonly log: Pick<Console, "error" | "warn">;
  private readonly bus: EventTarget;
  private readonly storage?: Storage;
  private readonly syncUrl: string;
  private readonly viewport?: () => CanvasViewport;
  private readonly restoredState: PreviewState | null;
  private readonly slides: PreviewSlideView[] = [];
  private readonly tileClickGuardCleanups: Array<() => void> = [];
  private fontScopeCleanup: (() => void) | null = null;
  private dimensions: CanvasDimensions = { width: 1280, height: 720 };
  private readonly onNavigate = (event: Event): void => {
    if (!this.isLoaded()) return;
    const detail = (event as CustomEvent<PreviewNavigateDetail>).detail;
    if (!detail || !("to" in detail)) {
      this.log.error("Invalid peitho:navigate event");
      return;
    }
    if (this.navigateToTarget(detail.to)) {
      event.preventDefault();
    }
  };
  private readonly onOverviewRequest = (event: Event): void => {
    if (!this.isLoaded()) return;
    const action = (event as CustomEvent<OverviewRequestDetail>).detail?.action;
    if (action === "toggle") this.toggleOverview();
    else if (action === "enter") this.enterGrid();
    else if (action === "exit") this.exitGrid();
    else if (action === "activate") this.activateSelection();
    else this.log.error("Invalid peitho:overviewrequest event");
  };
  private readonly onResize = (): void => this.applyLayout();

  constructor(options: PreviewShellOptions) {
    this.root = options.root;
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    const log = options.console ?? console;
    this.log = { error: log.error, warn: log.warn ?? console.warn.bind(console) };
    this.bus = options.bus ?? this.win;
    this.storage = options.storage ?? this.win.sessionStorage;
    this.syncUrl = options.syncUrl ?? "/sync";
    this.viewport = options.viewport;
    this.restoredState = this.readState();
    this.root.classList.add("peitho-preview-root");
    const rootPosition = this.win.getComputedStyle(this.root).position;
    if (rootPosition === "static" || rootPosition === "") {
      this.root.style.position = "relative";
    }
    this.root.style.background = "#000";
    this.bus.addEventListener("peitho:navigate", this.onNavigate);
    this.bus.addEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.win.addEventListener("resize", this.onResize);
  }

  async load(): Promise<void> {
    try {
      this.generation = await this.fetchGeneration();
      const manifest = await this.fetchJson<Manifest>("manifest.json");
      this.dimensions = {
        width: manifest.canvasWidth,
        height: manifest.canvasHeight
      };
      const cssAspect = manifest.aspectRatio.replace(":", " / ");
      this.setCanvasRootProperties(this.dimensions, cssAspect);
      const css = await this.fetchText("peitho.css");
      this.fontScopeCleanup = installDocumentFontScope(this.doc, css);
      await waitForFontsReady(this.doc, this.win, { log: this.log });
      const pending = await Promise.all(
        manifest.slides.map(async (slide) => {
          const html = await this.fetchText(slide.src);
          return this.createSlideView(slide, html, css);
        })
      );
      this.manifest = manifest;
      this.root.replaceChildren();
      for (const view of pending) {
        this.root.appendChild(view.tile);
        this.slides.push(view);
      }
      const restored = this.restoredState;
      const restoredIndex =
        restored === null
          ? this.clampIndex(initialSlideIndex(pending.map((view) => view.meta)) ?? 0)
          : this.clampIndex(restored.index);
      this.currentIndex = restoredIndex;
      this.selectedIndex = restoredIndex;
      this.mode = restored?.mode ?? "single";
      this.applyLayout();
      this.dispatchSlideChange(null);
    } catch (error) {
      this.clearCanvasRootProperties();
      this.root.replaceChildren();
      this.root.textContent = error instanceof Error ? error.message : String(error);
    }
  }

  navigate(to: PreviewNavigateTarget): void {
    if (!this.isLoaded()) return;
    this.navigateToTarget(to);
  }

  private navigateToTarget(to: PreviewNavigateTarget): boolean {
    const index = this.resolveTarget(to);
    if (index === null) return false;
    this.setIndex(index);
    return true;
  }

  saveState(): void {
    const index = this.clampIndex(this.selectedIndex >= 0 ? this.selectedIndex : this.currentIndex);
    try {
      this.storage?.setItem(PREVIEW_STATE_KEY, JSON.stringify({ mode: this.mode, index }));
    } catch (error) {
      this.log.error(`Failed to save preview state: ${String(error)}`);
    }
  }

  destroy(): void {
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.win.removeEventListener("resize", this.onResize);
    while (this.tileClickGuardCleanups.length > 0) this.tileClickGuardCleanups.pop()?.();
    this.fontScopeCleanup?.();
    this.fontScopeCleanup = null;
    this.clearCanvasRootProperties();
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

  private async fetchGeneration(): Promise<number> {
    const response = await this.fetchOk(this.syncUrl);
    const body = (await response.json()) as { generation?: unknown };
    if (typeof body.generation !== "number") {
      throw new Error("Invalid peitho sync generation");
    }
    return body.generation;
  }

  private createSlideView(slide: ManifestSlide, html: string, css: string): PreviewSlideView {
    const tile = this.doc.createElement("div");
    tile.classList.add("peitho-preview-tile");
    tile.dataset.slideKey = slide.key;
    tile.dataset.slideIndex = String(slide.index);
    const clickGuard = createClickNavigationGuard({ target: tile, window: this.win });
    this.tileClickGuardCleanups.push(() => clickGuard.destroy());
    tile.addEventListener("click", (event) => {
      if (clickGuard.shouldIgnoreClick(event)) return;
      this.setIndex(slide.index);
      this.exitGrid();
    });

    const host = this.doc.createElement("section");
    host.classList.add("peitho-preview-slide");
    host.dataset.slideKey = slide.key;
    host.dataset.slideIndex = String(slide.index);
    host.dataset.peithoCanvas = "slide";
    const shadow = host.attachShadow({ mode: "open" });
    const style = this.doc.createElement("style");
    style.textContent = css;
    shadow.appendChild(style);
    const template = this.doc.createElement("template");
    template.innerHTML = html;
    shadow.appendChild(template.content.cloneNode(true));
    tile.appendChild(host);
    return { meta: slide, tile, host };
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

  private isLoaded(): boolean {
    return this.manifest !== null;
  }

  private toggleOverview(): void {
    if (this.mode === "grid") this.exitGrid();
    else this.enterGrid();
  }

  private enterGrid(): void {
    if (this.mode === "grid") return;
    this.mode = "grid";
    this.selectedIndex = this.clampIndex(this.currentIndex);
    this.applyLayout();
    this.saveState();
  }

  private exitGrid(): void {
    if (this.mode === "single") return;
    this.mode = "single";
    this.selectedIndex = this.clampIndex(this.selectedIndex);
    this.currentIndex = this.selectedIndex;
    this.applyLayout();
    this.saveState();
  }

  private activateSelection(): void {
    if (this.mode === "grid") this.exitGrid();
    else this.enterGrid();
  }

  private setIndex(index: number): void {
    const next = this.clampIndex(index);
    if (next === this.currentIndex && next === this.selectedIndex) return;
    const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
    this.currentIndex = next;
    this.selectedIndex = next;
    this.applyLayout();
    this.dispatchSlideChange(previousIndex);
    this.saveState();
  }

  private resolveTarget(to: PreviewNavigateTarget): number | null {
    if (to === "first") return 0;
    if (to === "last") return this.slides.length - 1;
    if (to === "next") {
      if (this.mode === "grid") return Math.min(this.selectedIndex + 1, this.slides.length - 1);
      return this.resolveSequentialTarget(1);
    }
    if (to === "prev") {
      if (this.mode === "grid") return Math.max(this.selectedIndex - 1, 0);
      return this.resolveSequentialTarget(-1);
    }
    if (to === "up" || to === "down") return this.resolveGridVerticalTarget(to);
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
      this.selectedIndex,
      direction
    );
  }

  private resolveGridVerticalTarget(direction: "up" | "down"): number | null {
    if (this.mode !== "grid") return null;
    const columns = previewGridColumnCount(this.gridRootWidth());
    const selected = this.clampIndex(this.selectedIndex);
    const next = selected + (direction === "up" ? -columns : columns);
    if (next < 0 || next > this.slides.length - 1) return null;
    return next;
  }

  private gridRootWidth(): number {
    if (this.root.clientWidth > 0) return this.root.clientWidth;
    return this.viewport?.().width ?? this.win.innerWidth;
  }

  private clampIndex(index: number): number {
    if (this.slides.length === 0) return 0;
    return Math.min(Math.max(index, 0), this.slides.length - 1);
  }

  private applyLayout(): void {
    this.root.dataset.peithoPreviewMode = this.mode;
    if (this.mode === "grid") this.applyGridLayout();
    else this.applySingleLayout();
  }

  private applySingleLayout(): void {
    const viewport = this.viewport?.() ?? {
      width: this.win.innerWidth,
      height: this.win.innerHeight
    };
    const fit = calculateCanvasFit(viewport, this.dimensions.width, this.dimensions.height);
    this.root.style.display = "block";
    this.root.style.overflow = "hidden";
    this.root.style.padding = "0";
    this.root.style.gap = "0";
    this.root.style.gridTemplateColumns = "";
    this.root.style.removeProperty("scroll-padding-top");
    this.root.style.removeProperty("scroll-padding-bottom");

    this.slides.forEach((slide, index) => {
      const active = index === this.currentIndex;
      slide.tile.hidden = !active;
      slide.tile.classList.toggle("is-selected", active);
      slide.tile.style.position = "absolute";
      slide.tile.style.left = "0";
      slide.tile.style.top = "0";
      slide.tile.style.width = "100%";
      slide.tile.style.height = "100%";
      slide.tile.style.overflow = "hidden";
      slide.tile.style.border = "0";
      slide.tile.style.borderRadius = "0";
      slide.tile.style.outlineWidth = "";
      slide.tile.style.outlineStyle = "";
      slide.tile.style.outlineColor = "";
      slide.tile.style.outlineOffset = "";
      slide.tile.style.background = "transparent";
      slide.host.hidden = !active;
      this.applyHostFrame(slide.host, fit.left, fit.top, fit.scale);
    });
  }

  private applyGridLayout(): void {
    const scale = GRID_TILE_WIDTH / this.dimensions.width;
    const tileHeight = this.dimensions.height * scale;
    this.root.style.display = "grid";
    this.root.style.gridTemplateColumns =
      `repeat(auto-fit, minmax(${GRID_TILE_WIDTH}px, ${GRID_TILE_WIDTH}px))`;
    this.root.style.gap = `${GRID_GAP}px`;
    this.root.style.alignContent = "start";
    this.root.style.justifyContent = "center";
    this.root.style.overflow = "auto";
    this.root.style.padding = `${GRID_PADDING}px`;
    this.root.style.setProperty("scroll-padding-top", `${GRID_PADDING}px`);
    this.root.style.setProperty("scroll-padding-bottom", `${GRID_PADDING}px`);
    this.root.style.boxSizing = "border-box";

    this.slides.forEach((slide, index) => {
      const selected = index === this.selectedIndex;
      slide.tile.hidden = false;
      slide.tile.classList.toggle("is-selected", selected);
      slide.tile.setAttribute("aria-selected", String(selected));
      slide.tile.style.position = "relative";
      slide.tile.style.left = "";
      slide.tile.style.top = "";
      slide.tile.style.width = `${GRID_TILE_WIDTH}px`;
      slide.tile.style.height = `${tileHeight}px`;
      slide.tile.style.overflow = "hidden";
      slide.tile.style.border = "1px solid rgba(255,255,255,0.24)";
      slide.tile.style.borderRadius = "6px";
      slide.tile.style.outlineWidth = selected ? "3px" : "";
      slide.tile.style.outlineStyle = selected ? "solid" : "";
      slide.tile.style.outlineColor = selected ? "#7dd3fc" : "";
      slide.tile.style.outlineOffset = selected ? "1px" : "";
      slide.tile.style.background = "#000";
      slide.tile.style.cursor = "pointer";
      slide.tile.style.boxSizing = "content-box";
      slide.host.hidden = false;
      this.applyHostFrame(slide.host, 0, 0, scale);
    });
    this.scrollSelectedTileIntoView();
  }

  private scrollSelectedTileIntoView(): void {
    this.slides[this.selectedIndex]?.tile.scrollIntoView?.({ block: "nearest" });
  }

  private applyHostFrame(host: HTMLElement, left: number, top: number, scale: number): void {
    host.style.position = "absolute";
    host.style.left = "0";
    host.style.top = "0";
    host.style.width = `${this.dimensions.width}px`;
    host.style.height = `${this.dimensions.height}px`;
    host.style.transformOrigin = "top left";
    host.style.transform = `translate(${left}px, ${top}px) scale(${scale})`;
  }

  private dispatchSlideChange(previousIndex: number | null): void {
    const slide = this.slides[this.currentIndex];
    if (!slide) return;
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

  private readState(): PreviewState | null {
    let raw: string | null = null;
    try {
      raw = this.storage?.getItem(PREVIEW_STATE_KEY) ?? null;
    } catch (error) {
      this.log.error(`Failed to read preview state: ${String(error)}`);
      return null;
    }
    if (raw == null) return null;
    try {
      const parsed = JSON.parse(raw) as Partial<PreviewState>;
      if (
        (parsed.mode === "single" || parsed.mode === "grid") &&
        typeof parsed.index === "number"
      ) {
        return { mode: parsed.mode, index: parsed.index };
      }
    } catch (_error) {
      return null;
    }
    return null;
  }
}
