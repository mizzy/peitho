import type { Manifest } from "../../../bindings/Manifest";
import type { ManifestSlide } from "../../../bindings/ManifestSlide";
import type { Notes } from "../../../bindings/Notes";
import { calculateCanvasFit, type CanvasViewport } from "./canvas";
import { createClickNavigationGuard } from "./clickNavigationGuard";
import { installDocumentFontScope } from "./fontscope";
import { deckText, waitForFontsReady } from "./fontsReady";
import { hasChordModifier } from "./keyboard";
import type { NavigateTarget, SlideChangeDetail } from "./shell";
import { initialSlideIndex, nextNonSkippedIndex } from "./skipnav";
import {
  isBuildErrorSyncMessage,
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
  setBuildError(error: string | null): void;
  requestGenerationReload(generation: number, reload: () => void): void;
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
  thumb: HTMLElement;
  thumbHost: HTMLElement;
  tileNumber: HTMLElement;
};

type CanvasDimensions = {
  width: number;
  height: number;
};

type InitialSyncState = {
  generation: number;
  buildError: string | null;
};

type PreviewDraft = {
  key: string;
  text?: string;
  selectionStart: number;
  selectionEnd: number;
  focused: boolean;
};

type PreviewState = {
  mode: PreviewMode;
  index: number;
  draft?: PreviewDraft;
};

const PREVIEW_STATE_KEY = "peitho:preview-state";
const DEFAULT_PREVIEW_MODE: PreviewMode = "grid";
const GRID_TILE_WIDTH = 320;
const GRID_GAP = 18;
const GRID_PADDING = 24;
export const PREVIEW_NOTES_HEIGHT = 160;
export const PREVIEW_STRIP_WIDTH = 200;
const STRIP_PADDING = 12;
const STRIP_GAP = 10;
const NO_NOTES_PLACEHOLDER = "No notes for this slide.";

function isPreviewDraft(value: unknown): value is PreviewDraft {
  if (typeof value !== "object" || value === null) return false;
  const draft = value as Partial<PreviewDraft>;
  return (
    typeof draft.key === "string" &&
    (draft.text === undefined || typeof draft.text === "string") &&
    typeof draft.selectionStart === "number" &&
    Number.isFinite(draft.selectionStart) &&
    draft.selectionStart >= 0 &&
    typeof draft.selectionEnd === "number" &&
    Number.isFinite(draft.selectionEnd) &&
    draft.selectionEnd >= 0 &&
    typeof draft.focused === "boolean"
  );
}

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

function isComposingKey(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229;
}

function isEditableTarget(event: KeyboardEvent): boolean {
  const target = event.composedPath()[0];
  return (
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLInputElement ||
    target instanceof HTMLSelectElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

export function installPreviewKeyboard(
  win: Window = window,
  bus: EventTarget = win
): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    if (hasChordModifier(event) || isComposingKey(event)) return;
    const editable = isEditableTarget(event);
    if (editable && (event.shiftKey || (event.key !== "PageUp" && event.key !== "PageDown"))) {
      return;
    }
    if (event.key === "o") {
      event.preventDefault();
      dispatchOverviewRequest(bus, "toggle");
      return;
    }
    if (event.key === "Escape") {
      if (event.repeat) return;
      event.preventDefault();
      dispatchOverviewRequest(bus, "enter");
      return;
    }
    if (event.key === "Enter") {
      if (event.repeat) return;
      const target = event.composedPath()[0];
      if (target instanceof Element && target.closest("a") !== null) return;
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
    if (request.defaultPrevented || (!editable && !verticalPreviewNavigationTargets.has(to))) {
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
  shell: Pick<PreviewShell, "setBuildError" | "requestGenerationReload">,
  channelFactory: SyncChannelFactory = serverSyncChannelFactory(),
  reload: () => void = () => window.location.reload()
): () => void {
  const channel = channelFactory("peitho-sync");
  channel.onmessage = (event: { data: unknown }): void => {
    if (isSessionChangedSyncMessage(event.data)) return;
    if (isBuildErrorSyncMessage(event.data)) {
      shell.setBuildError(event.data.buildError);
    }
    if (isGenerationSyncMessage(event.data)) {
      shell.requestGenerationReload(event.data.generation, reload);
    }
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
  mode: PreviewMode = DEFAULT_PREVIEW_MODE;
  generation = 0;
  private firstLayoutDone = false;
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
  private notes: Notes = { version: 1, notes: {} };
  private readonly notesPanel: HTMLElement;
  private readonly notesTextarea: HTMLTextAreaElement;
  private readonly notesStatus: HTMLSpanElement;
  private readonly notesPositionText: HTMLSpanElement;
  private readonly buildErrorBanner: HTMLElement;
  private notesTextareaKey: string | null = null;
  private swallowEnterRepeat = false;
  private flushChain: Promise<boolean> = Promise.resolve(true);
  private flushesInFlight = 0;
  private transitionSequence = 0;
  private readonly strip: HTMLElement;
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
  private readonly onNotesKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "Enter" && event.repeat && this.swallowEnterRepeat) {
      event.preventDefault();
      return;
    }
    if (!event.repeat) this.swallowEnterRepeat = false;
    if (isComposingKey(event)) return;
    if (hasChordModifier(event) || event.key !== "Escape") return;
    event.preventDefault();
    this.notesTextarea.blur();
  };
  private readonly onNotesBlur = (): void => {
    this.swallowEnterRepeat = false;
    void this.flushNotes();
  };
  private readonly onPageHide = (): void => {
    this.saveState();
    const key = this.notesTextareaKey;
    const text = this.notesTextarea.value;
    void this.doFlush(key, text, true);
  };

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
    this.notesPanel = this.createNotesPanel();
    this.notesTextarea = this.notesPanel.querySelector<HTMLTextAreaElement>(
      '[data-peitho-preview="note"]'
    )!;
    this.notesStatus = this.notesPanel.querySelector<HTMLSpanElement>(
      '[data-peitho-preview="status"]'
    )!;
    this.notesPositionText = this.notesPanel.querySelector<HTMLSpanElement>(
      '[data-peitho-preview="position"]'
    )!;
    this.buildErrorBanner = this.createBuildErrorBanner();
    this.strip = this.createStrip();
    this.notesTextarea.addEventListener("keydown", this.onNotesKeyDown);
    this.notesTextarea.addEventListener("blur", this.onNotesBlur);
    this.bus.addEventListener("peitho:navigate", this.onNavigate);
    this.bus.addEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.win.addEventListener("resize", this.onResize);
    this.win.addEventListener("pagehide", this.onPageHide);
  }

  async load(): Promise<void> {
    try {
      const initialSyncState = await this.fetchInitialSyncState();
      this.generation = initialSyncState.generation;
      this.setBuildError(initialSyncState.buildError);
      const manifest = await this.fetchJson<Manifest>("manifest.json");
      this.notes = await this.fetchJson<Notes>("notes.json");
      this.dimensions = {
        width: manifest.canvasWidth,
        height: manifest.canvasHeight
      };
      const cssAspect = manifest.aspectRatio.replace(":", " / ");
      this.setCanvasRootProperties(this.dimensions, cssAspect);
      const css = await this.fetchText("peitho.css");
      this.fontScopeCleanup = installDocumentFontScope(this.doc, css);
      // Fetch the slide HTML before waiting on fonts: the deck's own text is what selects
      // which `unicode-range` subsets are worth fetching (see waitForFontsReady).
      const sources = await Promise.all(
        manifest.slides.map(async (slide) => ({ slide, html: await this.fetchText(slide.src) }))
      );
      await waitForFontsReady(this.doc, this.win, {
        log: this.log,
        text: deckText(sources.map((source) => source.html))
      });
      const pending = sources.map(({ slide, html }) => this.createSlideView(slide, html, css));
      this.manifest = manifest;
      this.doc.title = manifest.title;
      this.root.replaceChildren();
      this.strip.replaceChildren();
      for (const view of pending) {
        this.root.appendChild(view.tile);
        this.strip.appendChild(view.thumb);
        this.slides.push(view);
      }
      this.root.appendChild(this.notesPanel);
      this.root.appendChild(this.strip);
      this.root.appendChild(this.buildErrorBanner);
      const restored = this.restoredState;
      const restoredIndex =
        restored === null
          ? this.clampIndex(initialSlideIndex(pending.map((view) => view.meta)) ?? 0)
          : this.clampIndex(restored.index);
      this.currentIndex = restoredIndex;
      this.selectedIndex = restoredIndex;
      this.mode = restored?.mode ?? DEFAULT_PREVIEW_MODE;
      this.applyLayout();
      if (this.notesTextareaKey === null) this.renderNotes();
      this.restoreDraft(restored?.draft);
      this.markReady();
      this.dispatchSlideChange(null);
    } catch (error) {
      this.clearCanvasRootProperties();
      this.root.replaceChildren();
      this.root.textContent = error instanceof Error ? error.message : String(error);
      this.markReady();
    }
  }

  /**
   * Paint the black ground only now that the root has content (or an error message).
   * Until this runs the new document paints nothing, so a reload holds the previous
   * frame instead of flashing black for the duration of the mount.
   */
  private markReady(): void {
    this.root.style.background = "#000";
    this.doc.documentElement.dataset.peithoReady = "";
  }

  navigate(to: PreviewNavigateTarget): void {
    if (!this.isLoaded()) return;
    this.navigateToTarget(to);
  }

  setBuildError(error: string | null): void {
    this.buildErrorBanner.textContent = error ?? "";
    this.buildErrorBanner.hidden = error === null;
  }

  requestGenerationReload(generation: number, reload: () => void): void {
    if (generation === this.generation) return;
    this.saveState();
    reload();
  }

  private flushNotes(): Promise<boolean> {
    const key = this.notesTextareaKey;
    const text = this.notesTextarea.value;
    this.flushesInFlight += 1;
    // The two-arm then keeps the chain alive if doFlush ever rejects.
    this.flushChain = this.flushChain
      .then(
        () => this.doFlush(key, text, false),
        () => this.doFlush(key, text, false)
      )
      .finally(() => {
        this.flushesInFlight -= 1;
      });
    return this.flushChain;
  }

  private async doFlush(key: string | null, text: string, keepalive: boolean): Promise<boolean> {
    if (key === null) return true;
    if (!this.isDirty(key, text)) {
      this.setNotesStatus("");
      return true;
    }

    try {
      const requestBody = JSON.stringify({ key, text });
      if (keepalive && new TextEncoder().encode(requestBody).length > 60_000) keepalive = false; // Chrome rejects in-flight keepalive bodies over 64 KiB.
      const response = await this.fetcher("/notes", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: requestBody,
        keepalive
      });
      if (response.ok) {
        if (text === "") delete this.notes.notes[key];
        else this.notes.notes[key] = text;
        this.setNotesStatus("");
        return true;
      }

      const body = await response.text();
      let message = body;
      try {
        const error = (JSON.parse(body) as { error?: unknown }).error;
        if (typeof error === "string") message = error;
      } catch {
        // A non-JSON response is already the server's displayable error text.
      }
      this.setNotesStatus(message);
    } catch (error) {
      this.setNotesStatus(error instanceof Error ? error.message : String(error));
    }
    return false;
  }

  private navigateToTarget(to: PreviewNavigateTarget): boolean {
    const index = this.resolveTarget(to);
    if (index === null) return false;
    this.setIndex(index);
    return true;
  }

  saveState(): void {
    if (!this.isLoaded()) return;
    const state: PreviewState = { mode: this.mode, index: this.stateIndex() };
    const focused = this.doc.activeElement === this.notesTextarea;
    const dirty = !this.notesSettled();
    if (this.notesTextareaKey !== null && (dirty || focused)) {
      const draft: PreviewDraft = {
        key: this.notesTextareaKey,
        selectionStart: this.notesTextarea.selectionStart,
        selectionEnd: this.notesTextarea.selectionEnd,
        focused
      };
      if (dirty) draft.text = this.notesTextarea.value;
      state.draft = draft;
    }
    this.writeState(state);
  }

  destroy(): void {
    this.transitionSequence += 1;
    this.notesTextarea.removeEventListener("keydown", this.onNotesKeyDown);
    this.notesTextarea.removeEventListener("blur", this.onNotesBlur);
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.win.removeEventListener("resize", this.onResize);
    this.win.removeEventListener("pagehide", this.onPageHide);
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

  private async fetchInitialSyncState(): Promise<InitialSyncState> {
    const response = await this.fetchOk(this.syncUrl);
    const body: unknown = await response.json();
    if (!isGenerationSyncMessage(body)) {
      throw new Error("Invalid peitho sync generation");
    }
    if (!isBuildErrorSyncMessage(body)) {
      throw new Error("Invalid peitho sync build error");
    }
    return { generation: body.generation, buildError: body.buildError };
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
      this.commitTransition(slide.index, "single");
    });

    const host = this.createSlideHost(slide, html, css, "peitho-preview-slide");
    tile.appendChild(host);
    const tileNumber = this.createSlideNumber(slide);
    tile.appendChild(tileNumber);

    const thumb = this.doc.createElement("div");
    thumb.classList.add("peitho-preview-thumb");
    thumb.dataset.slideKey = slide.key;
    thumb.dataset.slideIndex = String(slide.index);
    thumb.setAttribute("role", "button");
    thumb.setAttribute("aria-label", `Slide ${slide.index + 1}`);
    thumb.addEventListener("click", () => this.setIndex(slide.index));
    const thumbHost = this.createSlideHost(slide, html, css, "peitho-preview-thumb-slide");
    thumbHost.style.pointerEvents = "none";
    thumb.appendChild(thumbHost);
    thumb.appendChild(this.createSlideNumber(slide));
    return { meta: slide, tile, host, thumb, thumbHost, tileNumber };
  }

  private createSlideNumber(slide: ManifestSlide): HTMLElement {
    const badge = this.doc.createElement("span");
    badge.classList.add("peitho-preview-number");
    badge.textContent = String(slide.index + 1);
    const style = badge.style;
    style.position = "absolute";
    style.left = "6px";
    style.bottom = "6px";
    style.zIndex = "1";
    style.padding = "1px 7px";
    style.borderRadius = "4px";
    style.background = "rgba(0,0,0,0.65)";
    style.color = "#fff";
    style.font = "600 12px/1.5 system-ui, sans-serif";
    style.fontVariantNumeric = "tabular-nums";
    style.pointerEvents = "none";
    return badge;
  }

  private createSlideHost(
    slide: ManifestSlide,
    html: string,
    css: string,
    className: string
  ): HTMLElement {
    const host = this.doc.createElement("section");
    host.classList.add(className);
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
    return host;
  }

  private createStrip(): HTMLElement {
    const strip = this.doc.createElement("nav");
    strip.classList.add("peitho-preview-strip");
    strip.dataset.peithoPreview = "strip";
    strip.setAttribute("aria-label", "Slides");
    strip.hidden = true;
    const style = strip.style;
    style.position = "absolute";
    style.left = "0";
    style.top = "0";
    style.bottom = "0";
    style.width = `${PREVIEW_STRIP_WIDTH}px`;
    style.boxSizing = "border-box";
    style.overflowY = "auto";
    style.padding = `${STRIP_PADDING}px`;
    style.scrollPadding = `${STRIP_PADDING}px`;
    style.flexDirection = "column";
    style.gap = `${STRIP_GAP}px`;
    style.borderRight = "1px solid rgba(255,255,255,0.16)";
    style.background = "#15181e";
    return strip;
  }

  private createBuildErrorBanner(): HTMLElement {
    const banner = this.doc.createElement("div");
    banner.dataset.peithoPreview = "build-error";
    banner.setAttribute("role", "alert");
    banner.hidden = true;
    const style = banner.style;
    style.position = "fixed";
    style.left = "0";
    style.right = "0";
    style.top = "0";
    style.maxHeight = "40vh";
    style.boxSizing = "border-box";
    style.overflowY = "auto";
    style.zIndex = "2147483647";
    style.padding = "12px 18px";
    style.borderBottom = "2px solid #ef4444";
    style.background = "#450a0a";
    style.color = "#fee2e2";
    style.font = "14px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";
    style.whiteSpace = "pre-wrap";
    style.overflowWrap = "anywhere";
    return banner;
  }

  private createNotesPanel(): HTMLElement {
    const panel = this.doc.createElement("aside");
    panel.classList.add("peitho-preview-notes");
    panel.dataset.peithoPreview = "notes";
    panel.setAttribute("aria-label", "Speaker notes");
    panel.hidden = true;
    const style = panel.style;
    style.position = "absolute";
    style.left = `${PREVIEW_STRIP_WIDTH}px`;
    style.right = "0";
    style.bottom = "0";
    style.height = `${PREVIEW_NOTES_HEIGHT}px`;
    style.boxSizing = "border-box";
    style.overflow = "auto";
    style.padding = "14px 24px";
    style.borderTop = "1px solid rgba(255,255,255,0.16)";
    style.background = "#15181e";
    style.color = "#e5e7eb";
    style.font = "18px/1.5 system-ui, sans-serif";
    style.flexDirection = "column";
    const positionRow = this.doc.createElement("div");
    positionRow.style.display = "flex";
    positionRow.style.font = "600 13px/1.5 system-ui, sans-serif";
    positionRow.style.color = "#9ca3af";
    positionRow.style.fontVariantNumeric = "tabular-nums";
    positionRow.style.marginBottom = "4px";
    const positionText = this.doc.createElement("span");
    positionText.dataset.peithoPreview = "position";
    positionText.style.flexShrink = "0";
    positionRow.appendChild(positionText);
    const status = this.doc.createElement("span");
    status.dataset.peithoPreview = "status";
    status.setAttribute("role", "alert");
    status.style.marginLeft = "auto";
    status.style.color = "#f87171";
    status.style.whiteSpace = "pre-wrap";
    status.style.overflowWrap = "anywhere";
    positionRow.appendChild(status);
    panel.appendChild(positionRow);
    const textarea = this.doc.createElement("textarea");
    textarea.dataset.peithoPreview = "note";
    textarea.setAttribute("aria-label", "Speaker notes");
    textarea.placeholder = NO_NOTES_PLACEHOLDER;
    textarea.style.background = "transparent";
    textarea.style.color = "inherit";
    textarea.style.font = "inherit";
    textarea.style.border = "none";
    textarea.style.resize = "none";
    textarea.style.flex = "1";
    textarea.style.minHeight = "0";
    textarea.style.width = "100%";
    textarea.style.padding = "0";
    textarea.style.outlineOffset = "4px";
    panel.appendChild(textarea);
    return panel;
  }

  /**
   * The only writer of the notes status. A save failure is the one thing in preview the
   * author must not miss (the server may be gone), so a non-empty status turns the whole
   * notes panel into the alert: red chip plus a red panel border.
   */
  private setNotesStatus(message: string): void {
    this.notesStatus.textContent = message;
    const failed = message !== "";
    this.notesStatus.style.background = failed ? "#7f1d1d" : "";
    this.notesStatus.style.color = failed ? "#fee2e2" : "#f87171";
    this.notesStatus.style.padding = failed ? "2px 10px" : "";
    this.notesStatus.style.borderRadius = failed ? "999px" : "";
    this.notesPanel.style.borderTop = failed
      ? "3px solid #ef4444"
      : "1px solid rgba(255,255,255,0.16)";
    this.notesPanel.style.background = failed ? "#241416" : "#15181e";
  }

  private renderNotes(): void {
    const slide = this.slides[this.currentIndex];
    const key = slide?.meta.key ?? null;
    this.notesPositionText.textContent = `${this.currentIndex + 1} / ${this.slides.length}`;
    if (this.notesTextareaKey !== key) {
      this.notesTextareaKey = key;
      this.notesTextarea.value = key === null ? "" : (this.notes.notes[key] ?? "");
    }
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
    this.commitTransition(this.currentIndex, "grid");
  }

  private exitGrid(): void {
    this.commitTransition(this.selectedIndex, "single");
  }

  private activateSelection(): void {
    if (this.mode === "grid") {
      this.exitGrid();
      return;
    }
    const length = this.notesTextarea.value.length;
    this.notesTextarea.setSelectionRange(length, length);
    this.notesTextarea.focus();
    this.swallowEnterRepeat = true;
  }

  private setIndex(index: number): void {
    this.commitTransition(index, this.mode);
  }

  private commitTransition(index: number, mode: PreviewMode): void {
    index = this.clampIndex(index);
    if (index === this.currentIndex && index === this.selectedIndex && mode === this.mode) return;
    const sequence = ++this.transitionSequence;
    const needsFlush =
      (mode === "grid" && this.mode === "single") ||
      (mode === "single" && this.slides[index]?.meta.key !== this.notesTextareaKey);
    const commit = (): void => {
      const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
      this.currentIndex = index;
      this.selectedIndex = index;
      this.mode = mode;
      if (mode === "grid" && this.doc.activeElement === this.notesTextarea) {
        this.notesTextarea.blur();
      }
      this.applyLayout();
      if (previousIndex !== index) this.dispatchSlideChange(previousIndex);
      this.saveState();
    };

    if (!needsFlush || this.notesSettled()) {
      commit();
      return;
    }

    void (async () => {
      while (true) {
        const flushed = await this.flushNotes();
        if (sequence !== this.transitionSequence || !flushed) return;
        if (this.notesSettled()) {
          commit();
          return;
        }
      }
    })();
  }

  private notesAreDirty(): boolean {
    return this.isDirty(this.notesTextareaKey, this.notesTextarea.value);
  }

  private notesSettled(): boolean {
    return this.flushesInFlight === 0 && !this.notesAreDirty();
  }

  private isDirty(key: string | null, text: string): boolean {
    return key !== null && text !== (this.notes.notes[key] ?? "");
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
    if (to === "up" || to === "down") {
      if (this.mode === "grid") return this.resolveGridVerticalTarget(to);
      return this.resolveSequentialTarget(to === "up" ? -1 : 1);
    }
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
    this.firstLayoutDone = true;
  }

  /**
   * `nearest` is right for stepping between neighbours (it holds the scroll still when the
   * target is already visible), but wrong for the first layout after a load: the strip starts
   * at scrollTop 0, so the minimal scroll pins the current slide to the bottom edge no matter
   * where the pre-reload scroll had it. Centre it once, then hand over to `nearest`.
   */
  private scrollIntoViewOnLayout(element: HTMLElement | undefined): void {
    element?.scrollIntoView?.({ block: this.firstLayoutDone ? "nearest" : "center" });
  }

  private applySingleLayout(): void {
    const viewport = this.viewport?.() ?? {
      width: this.win.innerWidth,
      height: this.win.innerHeight
    };
    const fit = calculateCanvasFit(
      {
        width: Math.max(0, viewport.width - PREVIEW_STRIP_WIDTH),
        height: Math.max(0, viewport.height - PREVIEW_NOTES_HEIGHT)
      },
      this.dimensions.width,
      this.dimensions.height
    );
    const thumbWidth = PREVIEW_STRIP_WIDTH - STRIP_PADDING * 2 - 2;
    const thumbScale = thumbWidth / this.dimensions.width;
    const thumbHeight = this.dimensions.height * thumbScale;
    this.notesPanel.hidden = false;
    this.notesPanel.style.display = "flex";
    this.strip.hidden = false;
    // An inline display would override the hidden attribute, so it is set only while shown.
    this.strip.style.display = "flex";
    this.renderNotes();
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
      slide.tile.style.left = `${PREVIEW_STRIP_WIDTH}px`;
      slide.tile.style.top = "0";
      slide.tile.style.width = `calc(100% - ${PREVIEW_STRIP_WIDTH}px)`;
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
      slide.tileNumber.hidden = true;
      this.applyHostFrame(slide.host, fit.left, fit.top, fit.scale);

      slide.thumb.classList.toggle("is-selected", active);
      slide.thumb.setAttribute("aria-current", active ? "true" : "false");
      slide.thumb.style.position = "relative";
      slide.thumb.style.flexShrink = "0";
      slide.thumb.style.width = `${thumbWidth}px`;
      slide.thumb.style.height = `${thumbHeight}px`;
      slide.thumb.style.overflow = "hidden";
      slide.thumb.style.border = "1px solid rgba(255,255,255,0.24)";
      slide.thumb.style.borderRadius = "4px";
      slide.thumb.style.outline = active ? "3px solid #7dd3fc" : "";
      slide.thumb.style.outlineOffset = active ? "1px" : "";
      slide.thumb.style.background = "#000";
      slide.thumb.style.cursor = "pointer";
      this.applyHostFrame(slide.thumbHost, 0, 0, thumbScale);
    });
    this.scrollIntoViewOnLayout(this.slides[this.currentIndex]?.thumb);
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
    this.notesPanel.hidden = true;
    this.notesPanel.style.display = "";
    this.strip.hidden = true;
    this.strip.style.display = "";

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
      slide.tileNumber.hidden = false;
      slide.tile.style.boxSizing = "content-box";
      slide.host.hidden = false;
      this.applyHostFrame(slide.host, 0, 0, scale);
    });
    this.scrollSelectedTileIntoView();
  }

  private scrollSelectedTileIntoView(): void {
    this.scrollIntoViewOnLayout(this.slides[this.selectedIndex]?.tile);
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
      const parsed = JSON.parse(raw) as {
        mode?: unknown;
        index?: unknown;
        draft?: unknown;
      };
      if (
        (parsed.mode === "single" || parsed.mode === "grid") &&
        typeof parsed.index === "number"
      ) {
        const state: PreviewState = { mode: parsed.mode, index: parsed.index };
        if (isPreviewDraft(parsed.draft)) state.draft = parsed.draft;
        return state;
      }
    } catch (_error) {
      return null;
    }
    return null;
  }

  private restoreDraft(draft: PreviewDraft | undefined): void {
    if (draft === undefined) return;
    const key = this.slides[this.currentIndex]?.meta.key;
    if (draft.key === key) {
      if (draft.text !== undefined) this.notesTextarea.value = draft.text;
      const selectionStart = Math.min(draft.selectionStart, this.notesTextarea.value.length);
      const selectionEnd = Math.min(draft.selectionEnd, this.notesTextarea.value.length);
      this.notesTextarea.setSelectionRange(selectionStart, selectionEnd);
      if (draft.focused && this.mode === "single") this.notesTextarea.focus();
    }
    this.writeState({ mode: this.mode, index: this.stateIndex() });
  }

  private stateIndex(): number {
    return this.clampIndex(this.selectedIndex >= 0 ? this.selectedIndex : this.currentIndex);
  }

  private writeState(state: PreviewState): void {
    try {
      this.storage?.setItem(PREVIEW_STATE_KEY, JSON.stringify(state));
    } catch (error) {
      this.log.error(`Failed to save preview state: ${String(error)}`);
    }
  }
}
