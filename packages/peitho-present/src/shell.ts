import type { Manifest } from "../../../bindings/Manifest";
import type { ManifestSlide } from "../../../bindings/ManifestSlide";
import { installCanvasScaler, type CanvasViewport } from "./canvas";
import { installDocumentFontScope } from "./fontscope";
import { initialSlideIndex, nextNonSkippedIndex } from "./skipnav";

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
export type TimerStateDetail = { running: boolean; elapsedMs: number };
export type TimerAdoptDetail = TimerStateDetail & { previousElapsedMs: number };
export type TimerControlDetail = { action: "start" | "pause" | "resume" | "reset" };

export type PresentShell = {
  manifest: Manifest | null;
  currentIndex: number;
  navigate(to: NavigateTarget): void;
  elapsedMs(): number;
  isPaused(): boolean;
  startedAt(): number | null;
  adoptTimerState(state: TimerStateDetail): void;
  destroy(): void;
};

export type ShellOptions = {
  root: HTMLElement;
  manifest?: Manifest;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  console?: Pick<Console, "error">;
  bus?: EventTarget;
  now?: () => number;
  viewport?: () => CanvasViewport;
};

export type PointerOverlayOptions = {
  canvas: HTMLCanvasElement;
  pointerColor?: string | null;
  fetcher?: typeof fetch;
  bus?: EventTarget;
  window?: Window;
  now?: () => number;
  console?: Pick<Console, "error">;
};

type CanvasDimensions = {
  width: number;
  height: number;
};

type PointerOverlayState = {
  x: number;
  y: number;
  visible: boolean;
};

type PointerOverlayEvent = { kind: "move"; x: number; y: number } | { kind: "up" };

type PointerTrailPoint = {
  x: number;
  y: number;
  timestamp: number;
};

type PointerPalette = {
  baseColor: string;
  coreColor: string;
  transparentBase: string;
};

type RgbColor = {
  r: number;
  g: number;
  b: number;
};

const DEFAULT_POINTER_BASE_COLOR = "#38bdf8";
const DEFAULT_POINTER_CORE_COLOR = "#e0f2fe";
const POINTER_TRAIL_DURATION_MS = 500;
const POINTER_TRAIL_CAP = 64;
const POINTER_CORE_MIX_TO_WHITE = 0.65;

const CSS_NAMED_COLORS: Record<string, string> = {
  aliceblue: "#f0f8ff",
  antiquewhite: "#faebd7",
  aqua: "#00ffff",
  aquamarine: "#7fffd4",
  azure: "#f0ffff",
  beige: "#f5f5dc",
  bisque: "#ffe4c4",
  black: "#000000",
  blanchedalmond: "#ffebcd",
  blue: "#0000ff",
  blueviolet: "#8a2be2",
  brown: "#a52a2a",
  burlywood: "#deb887",
  cadetblue: "#5f9ea0",
  chartreuse: "#7fff00",
  chocolate: "#d2691e",
  coral: "#ff7f50",
  cornflowerblue: "#6495ed",
  cornsilk: "#fff8dc",
  crimson: "#dc143c",
  cyan: "#00ffff",
  darkblue: "#00008b",
  darkcyan: "#008b8b",
  darkgoldenrod: "#b8860b",
  darkgray: "#a9a9a9",
  darkgreen: "#006400",
  darkgrey: "#a9a9a9",
  darkkhaki: "#bdb76b",
  darkmagenta: "#8b008b",
  darkolivegreen: "#556b2f",
  darkorange: "#ff8c00",
  darkorchid: "#9932cc",
  darkred: "#8b0000",
  darksalmon: "#e9967a",
  darkseagreen: "#8fbc8f",
  darkslateblue: "#483d8b",
  darkslategray: "#2f4f4f",
  darkslategrey: "#2f4f4f",
  darkturquoise: "#00ced1",
  darkviolet: "#9400d3",
  deeppink: "#ff1493",
  deepskyblue: "#00bfff",
  dimgray: "#696969",
  dimgrey: "#696969",
  dodgerblue: "#1e90ff",
  firebrick: "#b22222",
  floralwhite: "#fffaf0",
  forestgreen: "#228b22",
  fuchsia: "#ff00ff",
  gainsboro: "#dcdcdc",
  ghostwhite: "#f8f8ff",
  gold: "#ffd700",
  goldenrod: "#daa520",
  gray: "#808080",
  green: "#008000",
  greenyellow: "#adff2f",
  grey: "#808080",
  honeydew: "#f0fff0",
  hotpink: "#ff69b4",
  indianred: "#cd5c5c",
  indigo: "#4b0082",
  ivory: "#fffff0",
  khaki: "#f0e68c",
  lavender: "#e6e6fa",
  lavenderblush: "#fff0f5",
  lawngreen: "#7cfc00",
  lemonchiffon: "#fffacd",
  lightblue: "#add8e6",
  lightcoral: "#f08080",
  lightcyan: "#e0ffff",
  lightgoldenrodyellow: "#fafad2",
  lightgray: "#d3d3d3",
  lightgreen: "#90ee90",
  lightgrey: "#d3d3d3",
  lightpink: "#ffb6c1",
  lightsalmon: "#ffa07a",
  lightseagreen: "#20b2aa",
  lightskyblue: "#87cefa",
  lightslategray: "#778899",
  lightslategrey: "#778899",
  lightsteelblue: "#b0c4de",
  lightyellow: "#ffffe0",
  lime: "#00ff00",
  limegreen: "#32cd32",
  linen: "#faf0e6",
  magenta: "#ff00ff",
  maroon: "#800000",
  mediumaquamarine: "#66cdaa",
  mediumblue: "#0000cd",
  mediumorchid: "#ba55d3",
  mediumpurple: "#9370db",
  mediumseagreen: "#3cb371",
  mediumslateblue: "#7b68ee",
  mediumspringgreen: "#00fa9a",
  mediumturquoise: "#48d1cc",
  mediumvioletred: "#c71585",
  midnightblue: "#191970",
  mintcream: "#f5fffa",
  mistyrose: "#ffe4e1",
  moccasin: "#ffe4b5",
  navajowhite: "#ffdead",
  navy: "#000080",
  oldlace: "#fdf5e6",
  olive: "#808000",
  olivedrab: "#6b8e23",
  orange: "#ffa500",
  orangered: "#ff4500",
  orchid: "#da70d6",
  palegoldenrod: "#eee8aa",
  palegreen: "#98fb98",
  paleturquoise: "#afeeee",
  palevioletred: "#db7093",
  papayawhip: "#ffefd5",
  peachpuff: "#ffdab9",
  peru: "#cd853f",
  pink: "#ffc0cb",
  plum: "#dda0dd",
  powderblue: "#b0e0e6",
  purple: "#800080",
  rebeccapurple: "#663399",
  red: "#ff0000",
  rosybrown: "#bc8f8f",
  royalblue: "#4169e1",
  saddlebrown: "#8b4513",
  salmon: "#fa8072",
  sandybrown: "#f4a460",
  seagreen: "#2e8b57",
  seashell: "#fff5ee",
  sienna: "#a0522d",
  silver: "#c0c0c0",
  skyblue: "#87ceeb",
  slateblue: "#6a5acd",
  slategray: "#708090",
  slategrey: "#708090",
  snow: "#fffafa",
  springgreen: "#00ff7f",
  steelblue: "#4682b4",
  tan: "#d2b48c",
  teal: "#008080",
  thistle: "#d8bfd8",
  tomato: "#ff6347",
  turquoise: "#40e0d0",
  violet: "#ee82ee",
  wheat: "#f5deb3",
  white: "#ffffff",
  whitesmoke: "#f5f5f5",
  yellow: "#ffff00",
  yellowgreen: "#9acd32"
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

export function installPointerOverlay(options: PointerOverlayOptions): () => void {
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const now = options.now ?? Date.now;
  const log = options.console ?? console;
  const canvas = options.canvas;
  const ctx = canvas2dContext(canvas);
  const palette = pointerPalette(options.pointerColor);
  const state: PointerOverlayState = { x: 0, y: 0, visible: false };
  const trail: PointerTrailPoint[] = [];
  let closed = false;
  let seq = 0;
  let session: string | null = null;
  let frame: number | null = null;
  let retryTimer: number | null = null;

  const requestFrame = (callback: FrameRequestCallback): number => {
    if (typeof win.requestAnimationFrame === "function") {
      return win.requestAnimationFrame(callback);
    }
    return win.setTimeout(() => callback(now()), 16);
  };
  const cancelFrame = (handle: number): void => {
    if (typeof win.cancelAnimationFrame === "function") {
      win.cancelAnimationFrame(handle);
      return;
    }
    win.clearTimeout(handle);
  };

  const resizeCanvas = (): void => {
    const rect = canvas.getBoundingClientRect();
    const fallbackWidth = win.innerWidth || 1;
    const fallbackHeight = win.innerHeight || 1;
    const cssWidth = rect.width > 0 ? rect.width : fallbackWidth;
    const cssHeight = rect.height > 0 ? rect.height : fallbackHeight;
    const scale = win.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.round(cssWidth * scale));
    canvas.height = Math.max(1, Math.round(cssHeight * scale));
    draw();
  };

  const clearCanvas = (): void => {
    if (ctx == null) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
  };

  const requestDraw = (): void => {
    if (frame !== null) return;
    frame = requestFrame(() => {
      frame = null;
      draw();
      if (!closed && (state.visible || trail.length > 0)) {
        requestDraw();
      }
    });
  };

  const resetState = (): void => {
    state.visible = false;
    trail.length = 0;
    clearCanvas();
  };

  const setSession = (nextSession: string): boolean => {
    if (session !== null && session !== nextSession) {
      resetState();
      session = nextSession;
      return true;
    }
    session = nextSession;
    return false;
  };

  const applyEvent = (event: PointerOverlayEvent, options: { fadeUp?: boolean } = {}): void => {
    if (event.kind === "move") {
      state.x = event.x;
      state.y = event.y;
      state.visible = true;
      pushTrailPoint({ x: event.x, y: event.y, timestamp: now() });
      requestDraw();
      return;
    }
    if (options.fadeUp === false) {
      resetState();
      return;
    }
    state.visible = false;
    requestDraw();
  };

  const delay = (): Promise<void> =>
    new Promise((resolve) => {
      retryTimer = win.setTimeout(() => {
        retryTimer = null;
        resolve();
      }, 1000);
    });

  const handshake = async (): Promise<boolean> => {
    try {
      const response = await fetcher("/pointer");
      if (closed) return false;
      if (!response.ok) {
        log.error(`Failed to start pointer polling: ${response.status}`);
        await delay();
        return false;
      }
      const body = (await response.json()) as unknown;
      if (!isPointerHandshakeResponse(body)) {
        log.error("Invalid peitho pointer handshake");
        await delay();
        return false;
      }
      seq = body.seq;
      setSession(body.session);
      return true;
    } catch (error: unknown) {
      if (!closed) {
        log.error(`Failed to start pointer polling: ${String(error)}`);
        await delay();
      }
      return false;
    }
  };

  const poll = async (): Promise<void> => {
    let needsHandshake = true;
    while (!closed) {
      while (!closed && needsHandshake && !(await handshake())) {
        continue;
      }
      if (closed) return;
      needsHandshake = false;
      try {
        const response = await fetcher(`/pointer?seq=${seq}`);
        if (closed) return;
        if (response.status === 204) continue;
        if (!response.ok) {
          log.error(`Failed to poll pointer message: ${response.status}`);
          await delay();
          continue;
        }
        const body = pointerPollResponse((await response.json()) as unknown);
        if (body == null) {
          log.error("Invalid peitho pointer message");
          await delay();
          continue;
        }
        seq = body.seq;
        const sessionChanged = setSession(body.session);
        applyEvent(body.event, { fadeUp: !(sessionChanged && body.event.kind === "up") });
      } catch (error: unknown) {
        if (!closed) {
          log.error(`Failed to poll pointer message: ${String(error)}`);
          needsHandshake = true;
          await delay();
        }
      }
    }
  };

  const onNavigate = (): void => resetState();

  if (ctx != null) {
    resizeCanvas();
    win.addEventListener("resize", resizeCanvas);
    bus.addEventListener("peitho:navigate", onNavigate);
    void poll();
  }

  return () => {
    closed = true;
    bus.removeEventListener("peitho:navigate", onNavigate);
    win.removeEventListener("resize", resizeCanvas);
    if (frame !== null) {
      cancelFrame(frame);
      frame = null;
    }
    if (retryTimer !== null) {
      win.clearTimeout(retryTimer);
      retryTimer = null;
    }
    clearCanvas();
  };

  function draw(): void {
    if (ctx == null) return;
    const context = ctx;
    clearCanvas();
    const nowMs = now();
    const radius = 0.012 * Math.min(canvas.width, canvas.height);
    pruneTrail(nowMs);
    const headIndex = state.visible ? trail.length - 1 : -1;
    for (let index = trail.length - 1; index >= 0; index -= 1) {
      if (index === headIndex) continue;
      const point = trail[index];
      const alpha = trailOpacity(point, nowMs);
      if (alpha <= 0) continue;
      drawPointerPoint(context, point, alpha, radius * (0.6 + 0.4 * alpha));
    }
    if (state.visible) {
      drawPointerPoint(context, { x: state.x, y: state.y, timestamp: nowMs }, 1, radius);
    }
  }

  function pushTrailPoint(point: PointerTrailPoint): void {
    trail.push(point);
    if (trail.length > POINTER_TRAIL_CAP) {
      trail.splice(0, trail.length - POINTER_TRAIL_CAP);
    }
  }

  function pruneTrail(nowMs: number): void {
    while (trail.length > 0 && trailOpacity(trail[0], nowMs) <= 0) {
      trail.shift();
    }
  }

  function drawPointerPoint(
    context: CanvasRenderingContext2D,
    point: PointerTrailPoint,
    alpha: number,
    radius: number
  ): void {
    const x = point.x * canvas.width;
    const y = point.y * canvas.height;
    const gradient = context.createRadialGradient(x, y, 0, x, y, radius);
    gradient.addColorStop(0, palette.coreColor);
    gradient.addColorStop(0.25, palette.baseColor);
    gradient.addColorStop(1, palette.transparentBase);
    context.save();
    context.globalAlpha = alpha;
    context.fillStyle = gradient;
    context.beginPath();
    context.arc(x, y, radius, 0, Math.PI * 2);
    context.fill();
    context.restore();
  }
}

class PresentShellController implements PresentShell {
  manifest: Manifest | null = null;
  currentIndex = -1;
  private readonly slides: SlideView[] = [];
  private readonly root: HTMLElement;
  private readonly fetcher: typeof fetch;
  private readonly injectedManifest?: Manifest;
  private readonly win: Window;
  private readonly doc: Document;
  private readonly log: Pick<Console, "error">;
  private readonly bus: EventTarget;
  private readonly now: () => number;
  private readonly viewport?: () => CanvasViewport;
  private readonly canvasCleanups: Array<() => void> = [];
  private fontScopeCleanup: (() => void) | null = null;
  private pointerCleanup: (() => void) | null = null;
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
    this.injectedManifest = options.manifest;
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
      const manifest = this.injectedManifest ?? (await this.fetchJson<Manifest>("manifest.json"));
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
      this.show(initialSlideIndex(pending.map((view) => view.meta)) ?? 0);
      this.mountPointerOverlay();
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

  adoptTimerState(state: TimerStateDetail): void {
    const elapsedMs = Math.max(0, state.elapsedMs);
    const previousElapsedMs = this.elapsedMs();
    if (!state.running && elapsedMs === 0) {
      this.startedAtValue = null;
      this.pausedAtValue = null;
      this.pausedTotalMs = 0;
      this.ended = false;
      this.dispatchTimerAdopt(elapsedMs, state.running, previousElapsedMs);
      return;
    }
    const now = this.now();
    this.startedAtValue = now - elapsedMs;
    this.pausedAtValue = state.running ? null : now;
    this.pausedTotalMs = 0;
    this.ended = false;
    this.dispatchTimerAdopt(elapsedMs, state.running, previousElapsedMs);
  }

  destroy(): void {
    this.endPresentation();
    this.pointerCleanup?.();
    this.pointerCleanup = null;
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

  private mountPointerOverlay(): void {
    if (this.viewport != null) return;
    const canvas = this.doc.createElement("canvas");
    canvas.dataset.peithoPointerOverlay = "true";
    canvas.style.position = "absolute";
    canvas.style.inset = "0";
    canvas.style.zIndex = "4";
    canvas.style.pointerEvents = "none";
    canvas.style.width = "100%";
    canvas.style.height = "100%";
    this.root.appendChild(canvas);
    this.pointerCleanup = installPointerOverlay({
      canvas,
      fetcher: this.fetcher,
      bus: this.bus,
      window: this.win,
      now: this.now,
      console: this.log,
      pointerColor: this.manifest?.pointerColor ?? null
    });
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
    this.dispatchTimerChange();
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
    this.dispatchTimerChange();
  }

  private resumeTimer(): void {
    if (this.pausedAtValue === null) return;
    this.pausedTotalMs += this.now() - this.pausedAtValue;
    this.pausedAtValue = null;
    this.dispatchTimerChange();
  }

  private resetTimer(): void {
    this.startedAtValue = null;
    this.pausedAtValue = null;
    this.pausedTotalMs = 0;
    this.ended = false;
    this.dispatchTimerChange();
  }

  private dispatchTimerChange(): void {
    this.bus.dispatchEvent(
      new CustomEvent<TimerStateDetail>("peitho:timerchange", {
        detail: {
          running: this.startedAtValue !== null && this.pausedAtValue === null,
          elapsedMs: this.elapsedMs()
        }
      })
    );
  }

  private dispatchTimerAdopt(
    elapsedMs: number,
    running: boolean,
    previousElapsedMs: number
  ): void {
    this.bus.dispatchEvent(
      new CustomEvent<TimerAdoptDetail>("peitho:timeradopt", {
        detail: { running, elapsedMs, previousElapsedMs }
      })
    );
  }
}

function trailOpacity(point: PointerTrailPoint, nowMs: number): number {
  return Math.max(0, Math.min(1, 1 - (nowMs - point.timestamp) / POINTER_TRAIL_DURATION_MS));
}

function pointerPalette(pointerColor: string | null | undefined): PointerPalette {
  const requestedColor = pointerColor?.trim() || DEFAULT_POINTER_BASE_COLOR;
  const parsed = parsePointerColor(requestedColor);
  const baseColor = parsed == null ? DEFAULT_POINTER_BASE_COLOR : requestedColor;
  const rgb = parsed ?? parsePointerColor(DEFAULT_POINTER_BASE_COLOR)!;
  const coreColor =
    baseColor.toLowerCase() === DEFAULT_POINTER_BASE_COLOR
      ? DEFAULT_POINTER_CORE_COLOR
      : mixToWhite(baseColor, POINTER_CORE_MIX_TO_WHITE);
  return {
    baseColor,
    coreColor,
    transparentBase: transparentRgb(rgb)
  };
}

export function mixToWhite(color: string, amount: number): string {
  const rgb = parsePointerColor(color);
  if (rgb == null) {
    throw new Error(`Unsupported pointer color: ${color}`);
  }
  const mix = Math.max(0, Math.min(1, amount));
  return rgbToHex({
    r: Math.round(rgb.r * (1 - mix) + 255 * mix),
    g: Math.round(rgb.g * (1 - mix) + 255 * mix),
    b: Math.round(rgb.b * (1 - mix) + 255 * mix)
  });
}

function transparentRgb(color: RgbColor): string {
  return `rgba(${color.r}, ${color.g}, ${color.b}, 0)`;
}

function rgbToHex(color: RgbColor): string {
  const channel = (value: number): string => value.toString(16).padStart(2, "0");
  return `#${channel(color.r)}${channel(color.g)}${channel(color.b)}`;
}

function parsePointerColor(color: string): RgbColor | null {
  const value = color.trim().toLowerCase();
  const hex = value.startsWith("#") ? value : CSS_NAMED_COLORS[value];
  if (hex == null) return null;
  return parseHexPointerColor(hex);
}

function parseHexPointerColor(color: string): RgbColor | null {
  const hex = color.slice(1);
  if (![3, 4, 6, 8].includes(hex.length) || !/^[0-9a-f]+$/i.test(hex)) return null;
  if (hex.length === 3 || hex.length === 4) {
    return {
      r: Number.parseInt(`${hex[0]}${hex[0]}`, 16),
      g: Number.parseInt(`${hex[1]}${hex[1]}`, 16),
      b: Number.parseInt(`${hex[2]}${hex[2]}`, 16)
    };
  }
  return {
    r: Number.parseInt(hex.slice(0, 2), 16),
    g: Number.parseInt(hex.slice(2, 4), 16),
    b: Number.parseInt(hex.slice(4, 6), 16)
  };
}

function canvas2dContext(canvas: HTMLCanvasElement): CanvasRenderingContext2D | null {
  try {
    return canvas.getContext("2d");
  } catch (_error: unknown) {
    return null;
  }
}

function isPointerHandshakeResponse(value: unknown): value is { seq: number; session: string } {
  return (
    hasExactKeys(value, ["seq", "session"]) &&
    typeof value.seq === "number" &&
    Number.isFinite(value.seq) &&
    typeof value.session === "string"
  );
}

function pointerPollResponse(
  value: unknown
): { seq: number; event: PointerOverlayEvent; session: string } | null {
  if (
    !hasExactKeys(value, ["seq", "event", "session"]) ||
    typeof value.seq !== "number" ||
    !Number.isFinite(value.seq) ||
    typeof value.session !== "string"
  ) {
    return null;
  }
  const event = pointerOverlayEvent(value.event);
  if (event == null) return null;
  return { seq: value.seq, event, session: value.session };
}

function pointerOverlayEvent(value: unknown): PointerOverlayEvent | null {
  if (!isRecord(value)) return null;
  if (hasExactKeys(value, ["up"])) {
    return value.up === true ? { kind: "up" } : null;
  }
  const keys = Object.keys(value);
  if (keys.length !== 1 || !Object.hasOwn(value, "move")) {
    return null;
  }
  const move = (value as { move?: unknown }).move;
  if (!hasExactKeys(move, ["x", "y"])) {
    return null;
  }
  if (!isUnitCoordinate(move.x) || !isUnitCoordinate(move.y)) {
    return null;
  }
  return { kind: "move", x: move.x, y: move.y };
}

function hasExactKeys<T extends string>(
  value: unknown,
  keys: readonly T[]
): value is Record<T, unknown> {
  if (!isRecord(value)) return false;
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isUnitCoordinate(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1;
}
