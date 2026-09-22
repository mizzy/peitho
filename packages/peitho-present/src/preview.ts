import type { Manifest } from "../../../bindings/Manifest";
import type { ManifestSlide } from "../../../bindings/ManifestSlide";
import type { Notes } from "../../../bindings/Notes";
import type { SlideSources } from "../../../bindings/SlideSources";
import { calculateCanvasFit, type CanvasViewport } from "./canvas";
import { createClickNavigationGuard } from "./clickNavigationGuard";
import { installDocumentFontScope } from "./fontscope";
import { deckText, waitForFontsReady } from "./fontsReady";
import { hasChordModifier, isComposingKey } from "./keyboard";
import { postJson, readErrorResponse } from "./previewHttp";
import {
  openPreviewSourceEdit,
  type PreviewSourceEdit,
  type PreviewSourceEditCommitResult
} from "./previewSourceEdit";
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

export type PreviewEditorSelectionRange = {
  range: Range;
  select(range: Range): void;
};

export type PreviewSelectionRangeProvider = (
  editor: HTMLElement
) => PreviewEditorSelectionRange | null;

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
  selectionRangeProvider?: PreviewSelectionRangeProvider;
};

type PreviewSlideView = {
  meta: ManifestSlide;
  sourceKey: string;
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

type PanelStatusSource = "notes" | "slide-edit" | "slide-source";

type ActiveSlideEdit = {
  key: string;
  start: number;
  end: number;
  old: string;
  target: HTMLElement;
  editor: HTMLElement;
  originalNodes: Node[];
  originalContenteditable: string | null;
  originalStyle: string | null;
  editableStyle: string | null;
  trailingNewlineSentinel: boolean;
  commitPromise: Promise<boolean> | null;
  removeListeners(): void;
};

type ActiveEdit =
  | { kind: "inline"; edit: ActiveSlideEdit }
  | {
      kind: "source";
      edit: PreviewSourceEdit;
      view: PreviewSlideView;
      key: string;
      body: string;
      commitPromise: Promise<boolean> | null;
    };

type DiscardedDraftData =
  | {
      kind: "inline";
      key: string;
      target: HTMLElement;
      start: number;
      end: number;
      old: string;
      text: string;
    }
  | {
      kind: "source";
      view: PreviewSlideView;
      key: string;
      body: string;
      text: string;
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
/**
 * The save keys are not discoverable from the editor itself, and they differ from
 * the inline editor's (where plain Enter saves). Cmd and Ctrl are both accepted,
 * so both are named. A save error replaces this hint.
 */
const SOURCE_EDIT_HINT =
  "Cmd/Ctrl+Enter or click away saves · Enter inserts a newline · Esc cancels";
const RESTORE_DRAFT_HINT = "Draft discarded · Press u to restore";
const EDIT_AFFORDANCE_HINT = "Click text to edit · e for Markdown · notes below";
const EDIT_AFFORDANCE_WITHOUT_SOURCE_HINT = "Click text to edit · notes below";
const INLINE_EDIT_OUTLINE = "2px solid #38bdf8";
const NESTED_LIST_ITEM_BLOCKS = new Set([
  "BLOCKQUOTE",
  "DIV",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "OL",
  "P",
  "PRE",
  "TABLE",
  "UL"
]);

function isStringRecord(value: unknown): value is Record<string, string> {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.values(value).every((entry) => typeof entry === "string")
  );
}

function isSlideSources(value: unknown): value is SlideSources {
  if (typeof value !== "object" || value === null) return false;
  const sources = value as Partial<SlideSources>;
  return (
    typeof sources.version === "number" &&
    isStringRecord(sources.sources) &&
    isStringRecord(sources.unavailable)
  );
}

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

function parseEditableSourceRange(value: string): { start: number; end: number } | null {
  const match = /^(\d+)-(\d+)$/.exec(value);
  if (match === null) return null;
  const start = Number(match[1]);
  const end = Number(match[2]);
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start >= end) return null;
  return { start, end };
}

function isNestedListItemBlock(node: Node): boolean {
  return node instanceof Element && NESTED_LIST_ITEM_BLOCKS.has(node.tagName);
}

function placeCaretAtEnd(win: Window, editor: HTMLElement): void {
  const selection = win.getSelection();
  if (selection === null) return;
  const range = editor.ownerDocument.createRange();
  range.selectNodeContents(editor);
  range.collapse(false);
  selection.removeAllRanges();
  selection.addRange(range);
}

function selectionBelongsToEditor(range: Range, editor: HTMLElement): boolean {
  const container = range.commonAncestorContainer;
  return container === editor || editor.contains(container);
}

function selectionRange(
  selection: Selection | null,
  editor: HTMLElement
): PreviewEditorSelectionRange | null {
  if (selection === null || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!selectionBelongsToEditor(range, editor)) return null;
  return {
    range,
    select(nextRange: Range): void {
      selection.removeAllRanges();
      selection.addRange(nextRange);
    }
  };
}

function rangeFromStaticRange(editor: HTMLElement, source: StaticRange): Range | null {
  try {
    const range = editor.ownerDocument.createRange();
    range.setStart(source.startContainer, source.startOffset);
    range.setEnd(source.endContainer, source.endOffset);
    return selectionBelongsToEditor(range, editor) ? range : null;
  } catch {
    return null;
  }
}

function defaultSelectionRangeProvider(editor: HTMLElement): PreviewEditorSelectionRange | null {
  const documentSelection = editor.ownerDocument.getSelection();
  const root = editor.getRootNode();
  if (
    documentSelection !== null &&
    root instanceof ShadowRoot &&
    typeof documentSelection.getComposedRanges === "function"
  ) {
    try {
      for (const source of documentSelection.getComposedRanges({ shadowRoots: [root] })) {
        const range = rangeFromStaticRange(editor, source);
        if (range !== null) {
          return {
            range,
            select(nextRange: Range): void {
              documentSelection.removeAllRanges();
              documentSelection.addRange(nextRange);
            }
          };
        }
      }
    } catch {
      // Older engines can expose the method without accepting the standard options object.
    }
  }

  if (root instanceof ShadowRoot) {
    const rootSelection = (
      root as ShadowRoot & { getSelection?: () => Selection | null }
    ).getSelection?.();
    const selected = selectionRange(rootSelection ?? null, editor);
    if (selected !== null) return selected;
  }

  return selectionRange(documentSelection, editor);
}

function rangeEndsAtTextEnd(range: Range, editor: HTMLElement): boolean {
  try {
    const trailing = editor.ownerDocument.createRange();
    trailing.selectNodeContents(editor);
    trailing.setStart(range.endContainer, range.endOffset);
    return trailing.toString() === "";
  } catch {
    return false;
  }
}

function insertSourceNewline(
  win: Window,
  editor: HTMLElement,
  rangeProvider: PreviewSelectionRangeProvider
): boolean {
  const selected = rangeProvider(editor);
  if (selected === null || !selectionBelongsToEditor(selected.range, editor)) {
    editor.append("\n");
    placeCaretAtEnd(win, editor);
    return false;
  }

  const range = selected.range;
  range.deleteContents();
  range.collapse(true);
  const addSentinel =
    rangeEndsAtTextEnd(range, editor) && !(editor.textContent ?? "").endsWith("\n");
  const newline = editor.ownerDocument.createTextNode(addSentinel ? "\n\n" : "\n");
  range.insertNode(newline);
  if (addSentinel) range.setStart(newline, 1);
  else range.setStartAfter(newline);
  range.collapse(true);
  selected.select(range);
  return addSentinel;
}

function isEditableTarget(event: KeyboardEvent): boolean {
  const target = event.composedPath()[0];
  if (
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLInputElement ||
    target instanceof HTMLSelectElement
  ) {
    return true;
  }
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const editable = target.closest<HTMLElement>("[contenteditable]");
  return editable !== null && editable.getAttribute("contenteditable") !== "false";
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
    if (event.key === "u") {
      const request = new CustomEvent("peitho:restorerequest", { cancelable: true });
      bus.dispatchEvent(request);
      if (request.defaultPrevented) {
        event.preventDefault();
        return;
      }
    }
    if (event.key === "e" && !event.shiftKey) {
      event.preventDefault();
      bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
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
  private destroyed = false;
  private readonly root: HTMLElement;
  private readonly fetcher: typeof fetch;
  private readonly win: Window;
  private readonly doc: Document;
  private readonly log: Pick<Console, "error" | "warn">;
  private readonly bus: EventTarget;
  private readonly storage?: Storage;
  private readonly syncUrl: string;
  private readonly viewport?: () => CanvasViewport;
  private readonly selectionRangeProvider: PreviewSelectionRangeProvider;
  private readonly restoredState: PreviewState | null;
  private readonly slides: PreviewSlideView[] = [];
  private notes: Notes = { version: 1, notes: {} };
  private sources: SlideSources = { version: 1, sources: {}, unavailable: {} };
  private readonly notesPanel: HTMLElement;
  private readonly notesTextarea: HTMLTextAreaElement;
  private readonly notesStatus: HTMLSpanElement;
  private readonly panelStatuses = new Map<PanelStatusSource, string>();
  private readonly notesPositionText: HTMLSpanElement;
  private readonly sourceEditHint: HTMLSpanElement;
  private readonly buildErrorBanner: HTMLElement;
  private activeEdit: ActiveEdit | null = null;
  private discardedDraft: DiscardedDraftData | null = null;
  private notesTextareaKey: string | null = null;
  private swallowEnterRepeat = false;
  private flushChain: Promise<boolean> = Promise.resolve(true);
  private flushesInFlight = 0;
  private transitionSequence = 0;
  private pendingTransitionSettlements = 0;
  private deferredReload: (() => void) | null = null;
  private readonly strip: HTMLElement;
  private readonly tileListenerCleanups: Array<() => void> = [];
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
  private readonly onSourceEditRequest = (): void => this.tryStartSourceEdit();
  private readonly onRestoreRequest = (event: Event): void => this.restoreDiscardedDraft(event);
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
    const active = this.activeEdit;
    if (active?.kind === "inline") {
      const edit = active.edit;
      const text = this.slideEditText(edit);
      // Repeat an in-flight normal commit with keepalive because unload may abort the first fetch.
      if (text !== edit.old) {
        void this.sendSlideEdit(edit, text, true).catch(() => undefined);
      }
    } else if (active?.kind === "source") {
      active.edit.saveForPageHide();
    }
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
    this.selectionRangeProvider =
      options.selectionRangeProvider ?? defaultSelectionRangeProvider;
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
    this.sourceEditHint = this.notesPanel.querySelector<HTMLSpanElement>(
      '[data-peitho-preview="source-hint"]'
    )!;
    this.buildErrorBanner = this.createBuildErrorBanner();
    this.strip = this.createStrip();
    this.notesTextarea.addEventListener("keydown", this.onNotesKeyDown);
    this.notesTextarea.addEventListener("blur", this.onNotesBlur);
    this.bus.addEventListener("peitho:navigate", this.onNavigate);
    this.bus.addEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.bus.addEventListener("peitho:sourceeditrequest", this.onSourceEditRequest);
    this.bus.addEventListener("peitho:restorerequest", this.onRestoreRequest);
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
      const loadedSources: unknown = await this.fetchJson<unknown>("sources.json");
      if (!isSlideSources(loadedSources)) throw new Error("Invalid sources.json");
      this.sources = loadedSources;
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
    if (this.isEditOpen() || this.pendingTransitionSettlements > 0) {
      this.deferredReload = reload;
      return;
    }
    this.performGenerationReload(reload);
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
      const response = await postJson(this.fetcher, "/notes", { key, text }, keepalive);
      if (response.ok) {
        if (text === "") delete this.notes.notes[key];
        else this.notes.notes[key] = text;
        this.setNotesStatus("");
        return true;
      }

      this.setNotesStatus(await readErrorResponse(response));
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
    this.destroyed = true;
    this.advanceTransitionSequence();
    this.notesTextarea.removeEventListener("keydown", this.onNotesKeyDown);
    this.notesTextarea.removeEventListener("blur", this.onNotesBlur);
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.bus.removeEventListener("peitho:sourceeditrequest", this.onSourceEditRequest);
    this.bus.removeEventListener("peitho:restorerequest", this.onRestoreRequest);
    this.win.removeEventListener("resize", this.onResize);
    this.win.removeEventListener("pagehide", this.onPageHide);
    while (this.tileListenerCleanups.length > 0) this.tileListenerCleanups.pop()?.();
    const active = this.activeEdit;
    if (active !== null) {
      if (active.kind === "inline") {
        active.edit.removeListeners();
        this.restoreSlideEdit(active.edit);
      } else {
        active.edit.destroy();
      }
    }
    this.replaceActiveEdit(null);
    if (active?.kind === "source") {
      this.setSlideSourceStatus("");
      this.applyLayout();
    }
    this.deferredReload = null;
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
    this.tileListenerCleanups.push(() => clickGuard.destroy());
    const onTileClick = (event: MouseEvent): void => {
      if (clickGuard.shouldIgnoreClick(event)) return;
      if (this.tryStartSlideEdit(slide, host, event)) return;
      this.commitTransition(slide.index, "single");
    };
    tile.addEventListener("click", onTileClick);
    this.tileListenerCleanups.push(() => tile.removeEventListener("click", onTileClick));

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
    const onThumbClick = (): void => this.setIndex(slide.index);
    thumb.addEventListener("click", onThumbClick);
    this.tileListenerCleanups.push(() => thumb.removeEventListener("click", onThumbClick));
    const thumbHost = this.createSlideHost(slide, html, css, "peitho-preview-thumb-slide");
    thumbHost.style.pointerEvents = "none";
    thumb.appendChild(thumbHost);
    thumb.appendChild(this.createSlideNumber(slide));
    return { meta: slide, sourceKey: slide.key, tile, host, thumb, thumbHost, tileNumber };
  }

  private tryStartSourceEdit(): void {
    if (this.mode === "grid" || !this.canStartEdit()) {
      return;
    }
    const view = this.slides[this.currentIndex];
    if (view === undefined) return;
    const unavailable = this.sources.unavailable[view.sourceKey];
    if (unavailable !== undefined) {
      this.setSlideSourceStatus(unavailable);
      return;
    }
    const body = this.sources.sources[view.sourceKey];
    if (body === undefined) {
      this.setSlideSourceStatus("this slide cannot be edited from preview");
      return;
    }

    this.startSourceEdit(view, view.sourceKey, body, body);
  }

  private startSourceEdit(
    view: PreviewSlideView,
    key: string,
    body: string,
    text: string
  ): void {
    let edit!: PreviewSourceEdit;
    edit = openPreviewSourceEdit({
      document: this.doc,
      fetcher: this.fetcher,
      tile: view.tile,
      key,
      body,
      onCommitRequest: () => this.commitActiveEditAndRelease(),
      onCancelRequest: () => this.cancelSourceEdit(edit)
    });
    edit.textarea.value = text;
    edit.textarea.setSelectionRange(0, 0);
    this.replaceActiveEdit({ kind: "source", edit, view, key, body, commitPromise: null });
    // After activeEdit is set, so the derived hint in setPanelStatus can see it.
    this.setSlideSourceStatus("");
    this.applyLayout();
  }

  private finishSourceEditCommit(
    edit: PreviewSourceEdit,
    result: PreviewSourceEditCommitResult
  ): boolean {
    const active = this.activeEdit;
    if (active?.kind !== "source" || active.edit !== edit) return false;
    if (result.status === "closed") return true;
    if (result.status === "failed") {
      this.setSlideSourceStatus(result.message);
      return false;
    }
    if (result.status === "saved") {
      delete this.sources.sources[result.previousKey];
      this.sources.sources[result.key] = result.body;
      active.view.sourceKey = result.key;
    }
    this.replaceActiveEdit(null);
    this.setSlideSourceStatus("");
    if (this.pendingTransitionSettlements === 0) this.applyLayout();
    return true;
  }

  private cancelSourceEdit(edit: PreviewSourceEdit): void {
    const active = this.activeEdit;
    if (active?.kind !== "source" || active.edit !== edit) return;
    const text = edit.textarea.value;
    const discarded: DiscardedDraftData | undefined =
      text === active.body
        ? undefined
        : {
            kind: "source",
            view: active.view,
            key: active.key,
            body: active.body,
            text
          };
    edit.cancel();
    this.replaceActiveEdit(null, discarded);
    this.setSlideSourceStatus("");
    this.applyLayout();
    this.releaseDeferredReload();
  }

  private tryStartSlideEdit(
    slide: ManifestSlide,
    host: HTMLElement,
    event: MouseEvent
  ): boolean {
    if (!this.canStartEdit()) return true;
    if (this.mode !== "single" || slide.index !== this.currentIndex) return false;
    const shadow = host.shadowRoot;
    if (shadow === null) return false;
    const path = event.composedPath();
    const boundary = path.indexOf(shadow);
    if (boundary <= 0) return false;

    let target: HTMLElement | null = null;
    for (let index = 0; index < boundary; index += 1) {
      const candidate = path[index];
      if (
        candidate instanceof HTMLElement &&
        candidate.hasAttribute("data-peitho-src") &&
        candidate.hasAttribute("data-peitho-md")
      ) {
        target = candidate;
        break;
      }
    }
    if (target === null || target.getRootNode() !== shadow) return false;

    const encodedRange = target.getAttribute("data-peitho-src");
    const old = target.getAttribute("data-peitho-md");
    if (encodedRange === null || old === null) return false;
    const sourceRange = parseEditableSourceRange(encodedRange);
    if (sourceRange === null) return false;

    return this.startSlideEdit(slide.key, target, sourceRange.start, sourceRange.end, old, old);
  }

  private startSlideEdit(
    key: string,
    target: HTMLElement,
    start: number,
    end: number,
    old: string,
    text: string
  ): boolean {
    let editor = target;
    let originalNodes: Node[];
    if (target.tagName === "LI") {
      const children = Array.from(target.childNodes);
      const nestedBlockIndex = children.findIndex(isNestedListItemBlock);
      const inlineEnd = nestedBlockIndex < 0 ? children.length : nestedBlockIndex;
      originalNodes = children.slice(0, inlineEnd);
      const insertionPoint = children[inlineEnd] ?? null;
      editor = this.doc.createElement("span");
      for (const node of originalNodes) target.removeChild(node);
      target.insertBefore(editor, insertionPoint);
    } else {
      originalNodes = Array.from(target.childNodes);
      target.replaceChildren();
    }

    const originalContenteditable = editor.getAttribute("contenteditable");
    const originalStyle = editor.getAttribute("style");
    editor.textContent = text;
    editor.setAttribute("contenteditable", "plaintext-only");
    editor.style.outline = INLINE_EDIT_OUTLINE;
    editor.style.outlineOffset = "2px";
    const editableStyle = editor.getAttribute("style");

    let edit!: ActiveSlideEdit;
    const onKeyDown = (keyboardEvent: KeyboardEvent): void => {
      this.handleSlideEditKeyDown(edit, keyboardEvent);
    };
    const onBlur = (): void => {
      this.commitActiveEditAndRelease();
    };
    edit = {
      key,
      start,
      end,
      old,
      target,
      editor,
      originalNodes,
      originalContenteditable,
      originalStyle,
      editableStyle,
      trailingNewlineSentinel: false,
      commitPromise: null,
      removeListeners: () => {
        editor.removeEventListener("keydown", onKeyDown);
        editor.removeEventListener("blur", onBlur);
      }
    };
    editor.addEventListener("keydown", onKeyDown);
    editor.addEventListener("blur", onBlur);
    this.replaceActiveEdit({ kind: "inline", edit });
    this.setSlideEditStatus("");
    editor.focus({ preventScroll: true });
    placeCaretAtEnd(this.win, editor);
    return true;
  }

  private handleSlideEditKeyDown(edit: ActiveSlideEdit, event: KeyboardEvent): void {
    if (this.activeEdit?.kind !== "inline" || this.activeEdit.edit !== edit) return;
    if (edit.commitPromise !== null) {
      if (event.key === "Escape" || event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
      }
      return;
    }
    if (hasChordModifier(event) || isComposingKey(event)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      this.cancelSlideEdit(edit);
      return;
    }
    if (event.key !== "Enter") return;
    event.preventDefault();
    event.stopPropagation();
    if (event.shiftKey) {
      edit.trailingNewlineSentinel =
        insertSourceNewline(this.win, edit.editor, this.selectionRangeProvider) ||
        edit.trailingNewlineSentinel;
      return;
    }
    this.commitActiveEditAndRelease();
  }

  private cancelSlideEdit(edit: ActiveSlideEdit): void {
    const text = this.slideEditText(edit);
    const discarded: DiscardedDraftData | undefined =
      text === edit.old
        ? undefined
        : {
            kind: "inline",
            key: edit.key,
            target: edit.target,
            start: edit.start,
            end: edit.end,
            old: edit.old,
            text
          };
    if (!this.closeSlideEdit(edit, discarded)) return;
    this.releaseDeferredReload();
  }

  private closeSlideEdit(
    edit: ActiveSlideEdit,
    discarded?: DiscardedDraftData
  ): boolean {
    if (this.activeEdit?.kind !== "inline" || this.activeEdit.edit !== edit) return false;
    edit.removeListeners();
    this.replaceActiveEdit(null, discarded);
    this.restoreSlideEdit(edit);
    this.setSlideEditStatus("");
    return true;
  }

  private commitActiveEditAndRelease(): void {
    void this.commitActiveEdit().then((committed) => {
      if (committed && this.pendingTransitionSettlements === 0) {
        this.releaseDeferredReload();
      }
    });
  }

  private commitActiveEdit(): Promise<boolean> {
    const active = this.activeEdit;
    if (active === null) return Promise.resolve(true);
    if (active.kind === "source") {
      if (active.commitPromise !== null) return active.commitPromise;
      const commit = active.edit
        .commit()
        .then((result) => this.finishSourceEditCommit(active.edit, result))
        .finally(() => {
          active.commitPromise = null;
        });
      active.commitPromise = commit;
      return commit;
    }
    const edit = active.edit;
    if (edit.commitPromise !== null) return edit.commitPromise;
    const newText = this.slideEditText(edit);
    if (newText === edit.old) {
      this.closeSlideEdit(edit);
      return Promise.resolve(true);
    }

    const commit = this.postSlideEdit(edit, newText).finally(() => {
      edit.commitPromise = null;
    });
    edit.commitPromise = commit;
    this.lockSlideEdit(edit);
    return commit;
  }

  private async postSlideEdit(edit: ActiveSlideEdit, newText: string): Promise<boolean> {
    try {
      const response = await this.sendSlideEdit(edit, newText, false);
      if (response.ok) {
        if (this.activeEdit?.kind === "inline" && this.activeEdit.edit === edit) {
          this.finishSlideEdit(edit, newText);
        }
        return true;
      }

      const error = await readErrorResponse(response, "slide edit");
      if (this.activeEdit?.kind === "inline" && this.activeEdit.edit === edit) {
        this.setSlideEditStatus(error);
        this.unlockSlideEdit(edit);
      }
    } catch (error) {
      if (this.activeEdit?.kind === "inline" && this.activeEdit.edit === edit) {
        this.setSlideEditStatus(`failed to save slide edit: ${String(error)}`);
        this.unlockSlideEdit(edit);
      }
    }
    return false;
  }

  private finishSlideEdit(edit: ActiveSlideEdit, newText: string): void {
    edit.removeListeners();
    this.replaceActiveEdit(null);
    edit.editor.textContent = newText;
    this.restoreSlideEditorAttributes(edit);
    edit.target.removeAttribute("data-peitho-src");
    edit.target.removeAttribute("data-peitho-md");
    this.setSlideEditStatus("");
  }

  private sendSlideEdit(
    edit: ActiveSlideEdit,
    newText: string,
    keepalive: boolean
  ): Promise<Response> {
    return postJson(
      this.fetcher,
      "/slide-edit",
      {
        key: edit.key,
        start: edit.start,
        end: edit.end,
        old: edit.old,
        new: newText
      },
      keepalive
    );
  }

  private releaseDeferredReload(): void {
    const reload = this.deferredReload;
    if (reload === null) return;
    this.deferredReload = null;
    this.saveState();
    this.performGenerationReload(reload);
  }

  private performGenerationReload(reload: () => void): void {
    this.advanceTransitionSequence();
    this.renderPanelStatus();
    reload();
  }

  private restoreSlideEdit(edit: ActiveSlideEdit): void {
    if (edit.editor !== edit.target) {
      edit.editor.replaceWith(...edit.originalNodes);
      return;
    }
    edit.target.replaceChildren(...edit.originalNodes);
    this.restoreSlideEditorAttributes(edit);
  }

  private slideEditText(edit: ActiveSlideEdit): string {
    const text = edit.editor.textContent ?? "";
    if (edit.trailingNewlineSentinel && text.endsWith("\n")) return text.slice(0, -1);
    return text;
  }

  private lockSlideEdit(edit: ActiveSlideEdit): void {
    edit.editor.setAttribute("contenteditable", "false");
    edit.editor.style.opacity = "0.65";
  }

  private unlockSlideEdit(edit: ActiveSlideEdit): void {
    edit.editor.setAttribute("contenteditable", "plaintext-only");
    if (edit.editableStyle === null) edit.editor.removeAttribute("style");
    else edit.editor.setAttribute("style", edit.editableStyle);
    edit.editor.focus({ preventScroll: true });
  }

  private restoreSlideEditorAttributes(edit: ActiveSlideEdit): void {
    if (edit.originalContenteditable === null) edit.editor.removeAttribute("contenteditable");
    else edit.editor.setAttribute("contenteditable", edit.originalContenteditable);
    if (edit.originalStyle === null) edit.editor.removeAttribute("style");
    else edit.editor.setAttribute("style", edit.originalStyle);
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
    const hint = this.doc.createElement("span");
    hint.dataset.peithoPreview = "source-hint";
    hint.hidden = true;
    hint.style.marginLeft = "auto";
    hint.style.color = "#9ca3af";
    hint.style.paddingLeft = "12px";
    hint.style.flexShrink = "0";
    positionRow.appendChild(hint);
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

  private setNotesStatus(message: string): void {
    this.setPanelStatus("notes", message);
  }

  private setSlideEditStatus(message: string): void {
    this.setPanelStatus("slide-edit", message);
  }

  private setSlideSourceStatus(message: string): void {
    this.setPanelStatus("slide-source", message);
  }

  /**
   * Save channels clear only their own status so one successful write cannot hide an
   * unrelated failure. Any remaining failure keeps the entire panel visibly alerting.
   */
  private setPanelStatus(source: PanelStatusSource, message: string): void {
    if (message === "") this.panelStatuses.delete(source);
    else this.panelStatuses.set(source, message);
    this.renderPanelStatus();
  }

  private renderPanelStatus(): void {
    const combined = (["notes", "slide-edit", "slide-source"] as const)
      .map((statusSource) => this.panelStatuses.get(statusSource))
      .filter((status): status is string => status !== undefined)
      .join("\n");
    this.notesStatus.textContent = combined;
    // The restore hint and request handler share restorableDraft(), so a hidden
    // offer cannot remain keyboard-active.
    const restorable = this.restorableDraft();
    const currentSlide = this.slides[this.currentIndex];
    const editAffordance =
      currentSlide !== undefined &&
      this.sources.unavailable[currentSlide.sourceKey] !== undefined
        ? EDIT_AFFORDANCE_WITHOUT_SOURCE_HINT
        : EDIT_AFFORDANCE_HINT;
    const hint =
      this.destroyed
        ? ""
        : restorable !== null
          ? RESTORE_DRAFT_HINT
          : this.activeEdit?.kind === "source" && combined === ""
            ? SOURCE_EDIT_HINT
            : this.activeEdit === null && combined === ""
              ? editAffordance
              : "";
    this.sourceEditHint.textContent = hint;
    this.sourceEditHint.hidden = hint === "";
    const failed = combined !== "";
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
    // Source availability is slide-specific; keep the hint in its sole derivation point
    // while rendering it whenever the panel's current slide is rendered.
    this.renderPanelStatus();
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

  private isEditOpen(): boolean {
    return this.activeEdit !== null;
  }

  private canStartEdit(): boolean {
    return !this.isEditOpen() && this.pendingTransitionSettlements === 0;
  }

  private replaceActiveEdit(
    activeEdit: ActiveEdit | null,
    discarded?: DiscardedDraftData
  ): void {
    if (activeEdit !== null) this.advanceTransitionSequence();
    this.activeEdit = activeEdit;
    if (discarded !== undefined) {
      this.discardedDraft = discarded;
    }
    this.renderPanelStatus();
  }

  /** Eagerly release retained text and DOM references at the sole invalidation point. */
  private advanceTransitionSequence(): number {
    this.transitionSequence += 1;
    this.discardedDraft = null;
    return this.transitionSequence;
  }

  private restorableDraft(): DiscardedDraftData | null {
    if (
      this.activeEdit !== null ||
      !this.canStartEdit() ||
      this.panelStatuses.size > 0
    ) {
      return null;
    }
    return this.discardedDraft;
  }

  private restoreDiscardedDraft(event: Event): void {
    const discarded = this.restorableDraft();
    if (discarded === null) return;
    if (discarded.kind === "source") {
      this.startSourceEdit(
        discarded.view,
        discarded.key,
        discarded.body,
        discarded.text
      );
    } else {
      this.startSlideEdit(
        discarded.key,
        discarded.target,
        discarded.start,
        discarded.end,
        discarded.old,
        discarded.text
      );
    }
    event.preventDefault();
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
    if (this.isEditOpen()) {
      this.commitTransition(this.currentIndex, "single", () => this.focusNotes());
      return;
    }
    this.focusNotes();
  }

  private focusNotes(): void {
    const length = this.notesTextarea.value.length;
    this.notesTextarea.setSelectionRange(length, length);
    this.notesTextarea.focus();
    this.swallowEnterRepeat = true;
  }

  private setIndex(index: number): void {
    this.commitTransition(index, this.mode);
  }

  private commitTransition(
    index: number,
    mode: PreviewMode,
    afterCommit?: () => void
  ): void {
    index = this.clampIndex(index);
    if (
      afterCommit === undefined &&
      index === this.currentIndex &&
      index === this.selectedIndex &&
      mode === this.mode
    ) {
      return;
    }
    const sequence = this.advanceTransitionSequence();
    this.renderPanelStatus();
    const needsFlush =
      (mode === "grid" && this.mode === "single") ||
      (mode === "single" && this.slides[index]?.meta.key !== this.notesTextareaKey);
    const commit = (): void => {
      const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
      const previousMode = this.mode;
      this.currentIndex = index;
      this.selectedIndex = index;
      this.mode = mode;
      if (previousIndex !== index || previousMode !== mode) this.setSlideSourceStatus("");
      if (mode === "grid" && this.doc.activeElement === this.notesTextarea) {
        this.notesTextarea.blur();
      }
      this.applyLayout();
      if (previousIndex !== index) this.dispatchSlideChange(previousIndex);
      this.saveState();
      afterCommit?.();
    };

    if (!this.isEditOpen() && (!needsFlush || this.notesSettled())) {
      commit();
      this.releaseDeferredReload();
      return;
    }

    this.pendingTransitionSettlements += 1;
    void (async () => {
      try {
        const settled = await this.settleForTransition(sequence);
        if (sequence !== this.transitionSequence) return;
        if (!settled) {
          if (!this.isEditOpen()) this.releaseDeferredReload();
          return;
        }
        commit();
        this.releaseDeferredReload();
      } finally {
        this.pendingTransitionSettlements -= 1;
      }
    })();
  }

  private async settleForTransition(sequence: number): Promise<boolean> {
    if (!(await this.commitActiveEdit())) return false;
    if (sequence !== this.transitionSequence) return false;
    while (!this.notesSettled()) {
      if (!(await this.flushNotes())) return false;
      if (sequence !== this.transitionSequence) return false;
    }
    return true;
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
      const sourceEdit =
        active && this.activeEdit?.kind === "source" && this.activeEdit.view === slide
          ? this.activeEdit.edit
          : null;
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
      slide.host.hidden = !active || sourceEdit !== null;
      slide.tileNumber.hidden = true;
      if (sourceEdit === null) {
        this.applyHostFrame(slide.host, fit.left, fit.top, fit.scale);
      } else {
        sourceEdit.setFrame({
          left: fit.left,
          top: fit.top,
          width: this.dimensions.width * fit.scale,
          height: this.dimensions.height * fit.scale
        });
      }

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
