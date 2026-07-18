import type { Manifest } from "../../../bindings/Manifest";
import type { Notes } from "../../../bindings/Notes";
import { sectionIndexForSlide } from "./sections";
import {
  mountPresentShell as defaultMountPresentShell,
  type PresentShell,
  type ShellOptions,
  type TimerControlDetail
} from "./shell";
import { initialSlideIndex, nextNonSkippedIndex } from "./skipnav";
import {
  isCloseSyncMessage,
  isGenerationSyncMessage,
  isIndexSyncMessage,
  isSessionChangedSyncMessage,
  isSyncedSyncMessage,
  isSwappedSyncMessage,
  isTimerReplaySyncMessage,
  isTimerSyncMessage,
  serverSyncChannelFactory,
  type SyncChannelFactory
} from "./sync";
import { clamp01, formatMinuteSeconds, isOverrun, isValidDurationMs } from "./timeTracker";

export { mountPresentShell } from "./shell";
export { serverSyncChannelFactory } from "./sync";

type RemoteSlide = {
  key: string;
  skip: boolean;
  title: string;
};

type RemoteTimerAnchor = {
  running: boolean;
  elapsedMs: number;
  receivedAtMs: number;
};

type TimerVisualState = "stopped" | "running" | "paused";

type RemoteViewState =
  | { kind: "loading" }
  | { kind: "active"; synced: true }
  | { kind: "ended" };

const isReadOnly = (state: RemoteViewState): state is { kind: "ended" } =>
  state.kind === "ended";
const canInteract = (state: RemoteViewState): state is { kind: "active"; synced: true } =>
  state.kind === "active";

type RemoteRowElement = HTMLElement & { readonly __peithoDimmable: true };

type RemoteRow =
  | { kind: "dimmable"; element: RemoteRowElement }
  | { kind: "actions"; element: HTMLElement };

export type RemotePaceState =
  | { kind: "ahead" | "behind" | "onpace" | "overrun"; label: string }
  | { kind: "paused"; label: "Paused" };

export type RemoteView = {
  manifest: Manifest | null;
  currentIndex: number | null;
  destroy(): void;
};

export type RemoteViewOptions = {
  root: HTMLElement;
  manifestUrl?: string;
  notesUrl?: string;
  fetcher?: typeof fetch;
  channelFactory?: SyncChannelFactory;
  syncChannelFactory?: SyncChannelFactory;
  mountPresentShell?: (options: ShellOptions) => Promise<PresentShell>;
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  console?: Pick<Console, "error">;
  now?: () => number;
  reload?: () => void;
};

export type RemoteControlsOptions = {
  root: HTMLElement;
  document?: Document;
  bus?: EventTarget;
};

export type RemoteSyncBridgeOptions = {
  slides: ReadonlyArray<RemoteSlide>;
  channelFactory?: SyncChannelFactory;
  bus?: EventTarget;
  now?: () => number;
  getCurrentIndex(): number | null;
  setCurrentIndex(index: number): void;
  getTimerState(): RemoteTimerAnchor | null;
  setTimerState(state: RemoteTimerAnchor): void;
  setSynced(): void;
  setEnded(): void;
  onSessionChange(): void;
  console?: Pick<Console, "error">;
};

export async function mountRemoteView(options: RemoteViewOptions): Promise<RemoteView> {
  const view = new RemoteController(options);
  await view.load();
  return view;
}

export function installRemoteControls(options: RemoteControlsOptions): () => void {
  const doc = options.document ?? document;
  const bus = options.bus ?? window;
  const root = options.root;
  root.classList.remove("peitho-remote-error");
  root.replaceChildren();

  const container = doc.createElement("section");
  container.className = "peitho-remote";
  container.dataset.peithoEnded = "false";

  const preview = createDimmableRow(doc, "div", "peitho-remote-preview");
  preview.dataset.peithoRemote = "preview";

  const titlebar = createDimmableRow(doc, "div", "peitho-remote-titlebar");
  const title = doc.createElement("div");
  title.className = "peitho-remote-title";
  title.dataset.peithoRemote = "title";
  title.textContent = "Loading";
  const counter = doc.createElement("div");
  counter.className = "peitho-remote-counter";
  counter.dataset.peithoRemote = "counter";
  counter.textContent = "– / –";
  titlebar.append(title, counter);

  const chase = createDimmableRow(doc, "div", "peitho-remote-chase");
  chase.dataset.peithoRemote = "chase";
  chase.dataset.peithoChase = "slide";
  const chaseTrack = doc.createElement("div");
  chaseTrack.className = "peitho-remote-chase-track";
  chaseTrack.dataset.peithoRemote = "chase-track";
  const chaseFill = doc.createElement("div");
  chaseFill.className = "peitho-remote-chase-fill";
  chaseFill.dataset.peithoRemote = "chase-fill";
  chaseTrack.append(chaseFill);
  const rabbit = doc.createElement("span");
  rabbit.className = "peitho-remote-chase-marker";
  rabbit.dataset.peithoRemote = "marker-rabbit";
  rabbit.setAttribute("aria-label", "slide progress");
  rabbit.textContent = "🐰";
  const turtle = doc.createElement("span");
  turtle.className = "peitho-remote-chase-marker";
  turtle.dataset.peithoRemote = "marker-turtle";
  turtle.setAttribute("aria-label", "time progress");
  turtle.textContent = "🐢";
  chase.append(chaseTrack, rabbit, turtle);

  const pace = createDimmableRow(doc, "div", "peitho-remote-pace");
  const timerButton = doc.createElement("button");
  timerButton.type = "button";
  timerButton.className = "peitho-remote-timer-button";
  timerButton.dataset.peithoAction = "timer";
  timerButton.dataset.peithoRunning = "false";
  timerButton.dataset.peithoTimerAction = "start";
  timerButton.disabled = true;
  timerButton.setAttribute("aria-label", "Start timer");
  const timerIcon = doc.createElement("span");
  timerIcon.className = "peitho-remote-timer-icon";
  timerIcon.dataset.peithoIcon = "play";
  timerButton.append(timerIcon);
  const resetButton = doc.createElement("button");
  resetButton.type = "button";
  resetButton.className = "peitho-remote-reset-button";
  resetButton.dataset.peithoAction = "timer-reset";
  resetButton.disabled = true;
  resetButton.setAttribute("aria-label", "Reset timer");
  resetButton.textContent = "↺";
  const elapsedRow = doc.createElement("div");
  elapsedRow.className = "peitho-remote-elapsed-row";
  elapsedRow.dataset.peithoRemote = "elapsed-row";
  const elapsed = doc.createElement("span");
  elapsed.className = "peitho-remote-elapsed";
  elapsed.dataset.peithoRemote = "elapsed";
  elapsed.textContent = "0:00";
  const separator = doc.createElement("span");
  separator.className = "peitho-remote-time-separator";
  separator.dataset.peithoRemote = "time-separator";
  separator.textContent = "/";
  separator.hidden = true;
  const planned = doc.createElement("span");
  planned.className = "peitho-remote-planned";
  planned.dataset.peithoRemote = "planned";
  planned.hidden = true;
  elapsedRow.append(elapsed, separator, planned);
  const delta = doc.createElement("span");
  delta.className = "peitho-remote-pace-delta";
  delta.dataset.peithoRemote = "pace-delta";
  delta.hidden = true;
  pace.append(timerButton, resetButton, elapsedRow, delta);

  const notesPanel = createDimmableRow(doc, "section", "peitho-remote-notes");
  const notesCaption = doc.createElement("div");
  notesCaption.className = "peitho-remote-notes-caption";
  notesCaption.textContent = "NOTES";
  const notesBody = doc.createElement("div");
  notesBody.className = "peitho-remote-notes-body";
  notesBody.dataset.peithoRemote = "notes";
  notesPanel.append(notesCaption, notesBody);

  const actions = doc.createElement("div");
  actions.className = "peitho-remote-actions";
  const prev = remoteButton(doc, "prev", "Previous");
  const next = remoteButton(doc, "next", "Next");
  actions.append(prev, next);

  const onPrev = (): void => dispatchNavigate(bus, "prev");
  const onNext = (): void => dispatchNavigate(bus, "next");
  const onTimer = (): void => {
    const action = timerButton.dataset.peithoTimerAction;
    if (action === "start" || action === "pause" || action === "resume") {
      dispatchTimerControl(bus, action);
    }
  };
  const onReset = (): void => dispatchTimerControl(bus, "reset");
  prev.addEventListener("click", onPrev);
  next.addEventListener("click", onNext);
  timerButton.addEventListener("click", onTimer);
  resetButton.addEventListener("click", onReset);

  /**
   * This is the remote's vertical composition contract. Adding a row here is a
   * design change: decide whether it dims on Ended, and whether it reserves
   * vertical space even when it has no content.
   */
  const rows: RemoteRow[] = [
    { kind: "dimmable", element: preview },
    { kind: "dimmable", element: titlebar },
    { kind: "dimmable", element: chase },
    { kind: "dimmable", element: pace },
    { kind: "dimmable", element: notesPanel },
    { kind: "actions", element: actions }
  ];
  container.append(...rows.map((row) => row.element));
  root.append(container);

  return () => {
    prev.removeEventListener("click", onPrev);
    next.removeEventListener("click", onNext);
    timerButton.removeEventListener("click", onTimer);
    resetButton.removeEventListener("click", onReset);
    container.remove();
  };
}

export function createDimmableRow(
  doc: Document,
  tag: string,
  ...classNames: string[]
): RemoteRowElement {
  const el = doc.createElement(tag);
  el.classList.add("peitho-remote-dim-on-end", ...classNames);
  return el as RemoteRowElement;
}

export function installRemoteSyncBridge(options: RemoteSyncBridgeOptions): () => void {
  const bus = options.bus ?? window;
  const log = options.console ?? console;
  const now = options.now ?? Date.now;
  const channel = (options.channelFactory ?? serverSyncChannelFactory())("peitho-sync");
  let synced = false;

  const onNavigate = (event: Event): void => {
    if (!synced) return;
    const to = (event as CustomEvent<{ to?: unknown }>).detail?.to;
    if (to !== "next" && to !== "prev") return;
    const target = resolveRemoteTarget(options.slides, options.getCurrentIndex(), to);
    if (target === null) return;
    channel.postMessage({ index: target });
    options.setCurrentIndex(target);
  };

  const onTimerControl = (event: Event): void => {
    if (!synced) return;
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action !== "start" && action !== "pause" && action !== "resume" && action !== "reset") {
      log.error("Invalid peitho:timercontrol event");
      return;
    }
    const next = nextTimerStateForAction(action, options.getTimerState(), now());
    if (next === null) return;
    channel.postMessage({
      timer: { running: next.running, elapsedMs: Math.round(next.elapsedMs) }
    });
    options.setTimerState(next);
  };

  channel.onmessage = (event: { data: unknown }): void => {
    const data = event.data;
    if (isSyncedSyncMessage(data)) {
      synced = true;
      options.setSynced();
      return;
    }
    if (isCloseSyncMessage(data)) {
      options.setEnded();
      return;
    }
    if (isIndexSyncMessage(data)) {
      options.setCurrentIndex(data.index);
      return;
    }
    if (isTimerReplaySyncMessage(data)) {
      const serverAdvance = data.timer.running ? Math.max(0, data.nowMs - data.timer.atMs) : 0;
      options.setTimerState({
        running: data.timer.running,
        elapsedMs: data.timer.elapsedMs + serverAdvance,
        receivedAtMs: now()
      });
      return;
    }
    if (isSessionChangedSyncMessage(data)) {
      options.onSessionChange();
      return;
    }
    if (
      isSwappedSyncMessage(data) ||
      isGenerationSyncMessage(data) ||
      isTimerSyncMessage(data)
    ) {
      return;
    }
    log.error("Invalid peitho remote sync message");
  };

  bus.addEventListener("peitho:navigate", onNavigate);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  return () => {
    bus.removeEventListener("peitho:navigate", onNavigate);
    bus.removeEventListener("peitho:timercontrol", onTimerControl);
    channel.onmessage = null;
    channel.close();
  };
}

class RemoteController implements RemoteView {
  manifest: Manifest | null = null;
  currentIndex: number | null = null;
  private readonly root: HTMLElement;
  private readonly manifestUrl: string;
  private readonly notesUrl: string;
  private readonly fetcher: typeof fetch;
  private readonly channelFactory?: SyncChannelFactory;
  private readonly mountPresentShell: (options: ShellOptions) => Promise<PresentShell>;
  private readonly win: Window;
  private readonly doc: Document;
  private readonly bus: EventTarget;
  private readonly previewBus = new EventTarget();
  private readonly log: Pick<Console, "error">;
  private readonly now: () => number;
  private readonly reload: () => void;
  private state: RemoteViewState = { kind: "loading" };
  private notes: Notes = { version: 1, notes: {} };
  private renderedNotesValue: string | null = null;
  private slides: RemoteSlide[] = [];
  private timerState: RemoteTimerAnchor | null = null;
  private controlsCleanup: (() => void) | null = null;
  private syncCleanup: (() => void) | null = null;
  private previewShell: PresentShell | null = null;
  private timerInterval: number | null = null;

  constructor(options: RemoteViewOptions) {
    this.root = options.root;
    this.manifestUrl = options.manifestUrl ?? "manifest.json";
    this.notesUrl = options.notesUrl ?? "notes.json";
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.channelFactory = options.syncChannelFactory ?? options.channelFactory;
    this.mountPresentShell = options.mountPresentShell ?? defaultMountPresentShell;
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.bus = options.bus ?? this.win;
    this.log = options.console ?? console;
    this.now = options.now ?? Date.now;
    this.reload = options.reload ?? (() => this.win.location.reload());
  }

  async load(): Promise<void> {
    try {
      const manifest = await this.fetchJson<Manifest>(this.manifestUrl);
      this.manifest = manifest;
      this.notes = await this.fetchNotes();
      this.slides = manifest.slides.map((slide) => ({
        key: slide.key,
        skip: slide.skip === true,
        title: slide.text.title
      }));
      this.currentIndex = initialSlideIndex(this.slides);
      this.controlsCleanup = installRemoteControls({
        root: this.root,
        document: this.doc,
        bus: this.bus
      });
      const previewRoot = this.root.querySelector<HTMLElement>('[data-peitho-remote="preview"]');
      if (previewRoot != null) {
        this.previewShell = await this.mountPresentShell({
          root: previewRoot,
          fetcher: this.fetcher,
          window: this.win,
          document: this.doc,
          bus: this.previewBus,
          manifest,
          now: this.now,
          viewport: paneViewport(previewRoot)
        });
      }
      this.render();
      this.syncCleanup = installRemoteSyncBridge({
        slides: this.slides,
        channelFactory: this.channelFactory,
        bus: this.bus,
        now: this.now,
        getCurrentIndex: () => this.currentIndex,
        setCurrentIndex: (index) => this.setCurrentIndex(index),
        getTimerState: () => this.timerState,
        setTimerState: (state) => this.setTimerState(state),
        setSynced: () => this.setSynced(),
        setEnded: () => this.setEnded(),
        onSessionChange: () => this.reload(),
        console: this.log
      });
    } catch (error) {
      this.showError(error instanceof Error ? error.message : String(error));
    }
  }

  destroy(): void {
    this.clearTimerInterval();
    this.syncCleanup?.();
    this.syncCleanup = null;
    this.previewShell?.destroy();
    this.previewShell = null;
    this.controlsCleanup?.();
    this.controlsCleanup = null;
  }

  private async fetchJson<T>(url: string): Promise<T> {
    const response = await this.fetcher(url);
    if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
    return response.json() as Promise<T>;
  }

  private async fetchNotes(): Promise<Notes> {
    try {
      return await this.fetchJson<Notes>(this.notesUrl);
    } catch (error) {
      this.log.error(
        `Failed to load ${this.notesUrl}: ${error instanceof Error ? error.message : String(error)}`
      );
      return { version: 1, notes: {} };
    }
  }

  private setCurrentIndex(index: number): void {
    this.currentIndex = clampIndex(index, this.slides.length);
    this.render();
  }

  private setTimerState(state: RemoteTimerAnchor): void {
    this.timerState = {
      running: state.running,
      elapsedMs: Math.max(0, state.elapsedMs),
      receivedAtMs: state.receivedAtMs
    };
    this.render();
  }

  private setSynced(): void {
    if (this.state.kind !== "loading") return;
    this.state = { kind: "active", synced: true };
    this.render();
  }

  private setEnded(): void {
    this.state = { kind: "ended" };
    this.clearTimerInterval();
    this.render();
  }

  private render(): void {
    const manifest = this.manifest;
    if (manifest == null) return;
    const container = this.root.querySelector<HTMLElement>(".peitho-remote");
    if (container == null) return;
    container.dataset.peithoEnded = isReadOnly(this.state) ? "true" : "false";
    const currentIndex = this.currentIndex;
    const slide = currentIndex == null ? null : manifest.slides[currentIndex];
    const total = this.slides.length;

    setText(this.root, "title", slideTitle(slide?.text.title));
    setText(this.root, "counter", currentIndex == null ? `– / ${total}` : `${currentIndex + 1} / ${total}`);

    this.renderChase(manifest, currentIndex);
    this.renderPaceStatic(manifest);
    this.renderTimeDependentChrome(manifest, currentIndex);
    this.renderSection(manifest, currentIndex);
    this.renderNotes(slide?.key);
    this.renderButtons(currentIndex);
    this.syncPreview(currentIndex);
    this.updateTimerInterval();
  }

  private renderChase(manifest: Manifest, currentIndex: number | null): void {
    const rabbit = this.root.querySelector<HTMLElement>('[data-peitho-remote="marker-rabbit"]');
    if (rabbit == null) return;
    const plannedDurationMs = validPlannedDurationMs(manifest);
    const planned = plannedDurationMs != null;
    rabbit.hidden = !planned;
    if (planned) setChaseMarker(rabbit, slideFraction(manifest, currentIndex));
  }

  private renderPaceStatic(manifest: Manifest): void {
    const elapsedRow = this.root.querySelector<HTMLElement>('[data-peitho-remote="elapsed-row"]');
    if (elapsedRow == null) return;
    const separator = elapsedRow.querySelector<HTMLElement>('[data-peitho-remote="time-separator"]');
    const planned = elapsedRow.querySelector<HTMLElement>('[data-peitho-remote="planned"]');
    const plannedDurationMs = validPlannedDurationMs(manifest);
    if (separator != null) separator.hidden = plannedDurationMs == null;
    if (planned != null) {
      planned.hidden = plannedDurationMs == null;
      planned.textContent = plannedDurationMs == null ? "" : formatMinuteSeconds(plannedDurationMs);
    }
  }

  private renderTimeDependentChrome(manifest: Manifest, currentIndex: number | null): void {
    const timerButton = this.root.querySelector<HTMLButtonElement>('[data-peitho-action="timer"]');
    const resetButton = this.root.querySelector<HTMLButtonElement>(
      '[data-peitho-action="timer-reset"]'
    );
    const elapsed = this.root.querySelector<HTMLElement>('[data-peitho-remote="elapsed"]');
    if (timerButton == null || resetButton == null || elapsed == null) return;

    const elapsedMs = this.currentElapsedMs();
    const state = timerVisualState(this.timerState, elapsedMs);
    timerButton.disabled = !canInteract(this.state);
    resetButton.disabled = !canInteract(this.state) || state === "stopped";
    timerButton.dataset.peithoRunning = state === "running" ? "true" : "false";
    timerButton.dataset.peithoTimerAction = playpauseActionFor(state);
    timerButton.setAttribute("aria-label", timerAriaLabel(state));
    const icon = timerButton.querySelector<HTMLElement>(".peitho-remote-timer-icon");
    if (icon != null) icon.dataset.peithoIcon = state === "running" ? "pause" : "play";

    elapsed.textContent = formatMinuteSeconds(elapsedMs);

    this.updateChaseTime(manifest, currentIndex, elapsedMs, state);
  }

  private updateChaseTime(
    manifest: Manifest,
    currentIndex: number | null,
    elapsedMs: number,
    state: TimerVisualState
  ): void {
    const chase = this.root.querySelector<HTMLElement>('[data-peitho-remote="chase"]');
    const fill = this.root.querySelector<HTMLElement>('[data-peitho-remote="chase-fill"]');
    const rabbit = this.root.querySelector<HTMLElement>('[data-peitho-remote="marker-rabbit"]');
    const turtle = this.root.querySelector<HTMLElement>('[data-peitho-remote="marker-turtle"]');
    const delta = this.root.querySelector<HTMLElement>('[data-peitho-remote="pace-delta"]');
    if (chase == null || fill == null || rabbit == null || turtle == null || delta == null) return;

    const plannedDurationMs = validPlannedDurationMs(manifest);
    if (plannedDurationMs == null) {
      chase.dataset.peithoChase = "slide";
      chase.classList.remove("peitho-remote-chase-overrun");
      rabbit.hidden = true;
      turtle.hidden = true;
      delta.hidden = true;
      delta.textContent = "";
      delete delta.dataset.peithoPace;
      setChaseFill(fill, slideFraction(manifest, currentIndex));
      return;
    }

    chase.dataset.peithoChase = "time";
    rabbit.hidden = false;
    turtle.hidden = false;
    const overrun = isOverrun(elapsedMs, plannedDurationMs);
    chase.classList.toggle("peitho-remote-chase-overrun", overrun);
    const turtleFraction = clamp01(elapsedMs / plannedDurationMs);
    setChaseMarker(turtle, turtleFraction);
    setChaseFill(fill, turtleFraction);

    if (currentIndex == null) {
      delta.hidden = true;
      delta.textContent = "";
      delete delta.dataset.peithoPace;
      return;
    }
    const paceState = remotePaceState(manifest, currentIndex, elapsedMs, state === "running");
    if (paceState == null) {
      delta.hidden = true;
      delta.textContent = "";
      delete delta.dataset.peithoPace;
      return;
    }
    delta.hidden = false;
    delta.dataset.peithoPace = paceState.kind;
    delta.textContent = paceState.label;
  }

  private renderSection(manifest: Manifest, currentIndex: number | null): void {
    const existing = this.root.querySelector<HTMLElement>('[data-peitho-remote="section"]');
    if (currentIndex == null || manifest.sections.length === 0) {
      existing?.remove();
      return;
    }
    const sectionIndex = sectionIndexForSlide(manifest.sections, currentIndex);
    if (sectionIndex < 0) {
      existing?.remove();
      return;
    }
    const section = manifest.sections[sectionIndex];
    const sectionSlideCount = section.endIndex - section.startIndex + 1;
    const sectionOffset = currentIndex - section.startIndex + 1;
    const sectionLine = existing ?? createDimmableRow(this.doc, "div", "peitho-remote-section");
    sectionLine.dataset.peithoRemote = "section";
    const name = this.doc.createElement("b");
    name.textContent = section.name;
    sectionLine.replaceChildren(
      name,
      this.doc.createTextNode(` · slide ${sectionOffset} / ${sectionSlideCount} in section`)
    );
    if (existing == null) {
      const notes = this.root.querySelector<HTMLElement>(".peitho-remote-notes");
      notes?.before(sectionLine);
    }
  }

  private renderNotes(slideKey: string | undefined): void {
    const notes = this.root.querySelector<HTMLElement>('[data-peitho-remote="notes"]');
    if (notes == null) return;
    const value = slideKey == null ? null : this.notes.notes[slideKey];
    if (value == null || value.length === 0) {
      this.setNotesText(notes, "No notes for this slide");
      notes.dataset.peithoEmpty = "true";
      return;
    }
    this.setNotesText(notes, value);
    notes.dataset.peithoEmpty = "false";
  }

  private setNotesText(notes: HTMLElement, value: string): void {
    if (this.renderedNotesValue === value) return;
    notes.textContent = value;
    this.renderedNotesValue = value;
  }

  private renderButtons(currentIndex: number | null): void {
    const prev = this.root.querySelector<HTMLButtonElement>('[data-peitho-action="prev"]');
    const next = this.root.querySelector<HTMLButtonElement>('[data-peitho-action="next"]');
    if (prev == null || next == null) return;
    prev.disabled =
      !canInteract(this.state) || resolveRemoteTarget(this.slides, currentIndex, "prev") === null;
    next.disabled =
      !canInteract(this.state) || resolveRemoteTarget(this.slides, currentIndex, "next") === null;
  }

  private syncPreview(currentIndex: number | null): void {
    if (currentIndex == null) return;
    this.previewBus.dispatchEvent(
      new CustomEvent("peitho:navigate", { detail: { to: { index: currentIndex } } })
    );
  }

  private currentElapsedMs(): number {
    return currentTimerElapsedMs(this.timerState, this.now());
  }

  private updateTimerInterval(): void {
    if (isReadOnly(this.state) || this.timerState?.running !== true) {
      this.clearTimerInterval();
      return;
    }
    if (this.timerInterval != null) return;
    this.timerInterval = this.win.setInterval(() => {
      const manifest = this.manifest;
      if (manifest == null) return;
      this.renderTimeDependentChrome(manifest, this.currentIndex);
    }, 1000);
  }

  private clearTimerInterval(): void {
    if (this.timerInterval == null) return;
    this.win.clearInterval(this.timerInterval);
    this.timerInterval = null;
  }

  private showError(message: string): void {
    this.destroy();
    this.root.replaceChildren();
    this.root.className = "peitho-remote-error";
    this.root.textContent = message;
  }
}

function remoteButton(doc: Document, action: "prev" | "next", label: string): HTMLButtonElement {
  const button = doc.createElement("button");
  button.type = "button";
  button.disabled = true;
  button.dataset.peithoAction = action;
  button.dataset.peithoDirection = action;
  const arrow = doc.createElement("span");
  arrow.className = "peitho-remote-action-arrow";
  arrow.setAttribute("aria-hidden", "true");
  arrow.textContent = action === "prev" ? "‹" : "›";
  const labelSpan = doc.createElement("span");
  labelSpan.className = "peitho-remote-action-label";
  labelSpan.textContent = label;
  if (action === "prev") {
    button.append(arrow, labelSpan);
  } else {
    button.append(labelSpan, arrow);
  }
  return button;
}

function dispatchNavigate(bus: EventTarget, to: "prev" | "next"): void {
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
}

function dispatchTimerControl(bus: EventTarget, action: TimerControlDetail["action"]): void {
  bus.dispatchEvent(new CustomEvent<TimerControlDetail>("peitho:timercontrol", { detail: { action } }));
}

function resolveRemoteTarget(
  slides: ReadonlyArray<RemoteSlide>,
  currentIndex: number | null,
  to: "prev" | "next"
): number | null {
  const base = currentIndex ?? initialSlideIndex(slides);
  if (base === null) return null;
  return nextNonSkippedIndex(slides, base, to === "next" ? 1 : -1);
}

function clampIndex(index: number, total: number): number | null {
  if (total === 0) return null;
  return Math.max(0, Math.min(Math.trunc(index), total - 1));
}

function slideFraction(manifest: Manifest, currentIndex: number | null): number {
  if (currentIndex == null) return 0;
  if (manifest.slideCount <= 1) return 1;
  return clamp01(currentIndex / (manifest.slideCount - 1));
}

function chasePercent(ratio: number): number {
  return Math.round(clamp01(ratio) * 10_000) / 100;
}

function setChaseMarker(element: HTMLElement, ratio: number): void {
  const percent = chasePercent(ratio);
  element.style.left = `${percent}%`;
  element.style.transform = `translateX(${-percent}%)`;
}

function setChaseFill(element: HTMLElement, ratio: number): void {
  element.style.width = `${chasePercent(ratio)}%`;
}

function setText(root: HTMLElement, key: string, value: string): void {
  const element = root.querySelector<HTMLElement>(`[data-peitho-remote="${key}"]`);
  if (element != null) element.textContent = value;
}

function slideTitle(title: string | undefined): string {
  return title == null || title.length === 0 ? "Untitled slide" : title;
}

function validPlannedDurationMs(manifest: Manifest): number | null {
  const plannedDurationMs = manifest.plannedDurationMs;
  return plannedDurationMs != null && isValidDurationMs(plannedDurationMs)
    ? plannedDurationMs
    : null;
}

export function expectedElapsedAtSlide(manifest: Manifest, index: number): number | null {
  const plannedDurationMs = validPlannedDurationMs(manifest);
  if (plannedDurationMs == null) return null;
  const slideCount = Math.max(1, manifest.slideCount);
  const requestedIndex = Number.isFinite(index) ? Math.trunc(index) : 0;
  if (requestedIndex <= 0) return 0;
  if (requestedIndex >= slideCount) return plannedDurationMs;
  if (manifest.sections.length === 0) {
    return (plannedDurationMs * requestedIndex) / slideCount;
  }
  const sectionIndex = sectionIndexForSlide(manifest.sections, requestedIndex);
  if (sectionIndex < 0) return (plannedDurationMs * requestedIndex) / slideCount;
  let elapsed = 0;
  for (let i = 0; i < sectionIndex; i += 1) {
    elapsed += manifest.sections[i].plannedDurationMs;
  }
  const section = manifest.sections[sectionIndex];
  const sectionSlideCount = section.endIndex - section.startIndex + 1;
  return (
    elapsed +
    section.plannedDurationMs * ((requestedIndex - section.startIndex) / sectionSlideCount)
  );
}

export function remotePaceState(
  manifest: Manifest,
  index: number,
  elapsedMs: number,
  running: boolean
): RemotePaceState | null {
  const expectedStart = expectedElapsedAtSlide(manifest, index);
  const expectedEnd = expectedElapsedAtSlide(manifest, index + 1);
  if (expectedStart == null || expectedEnd == null) return null;
  if (!running) {
    return elapsedMs > 0 ? { kind: "paused", label: "Paused" } : null;
  }
  const plannedDurationMs = validPlannedDurationMs(manifest);
  if (plannedDurationMs != null && isOverrun(elapsedMs, plannedDurationMs)) {
    return {
      kind: "overrun",
      label: `+${formatMinuteSeconds(elapsedMs - plannedDurationMs)} over`
    };
  }
  if (elapsedMs < expectedStart) {
    return {
      kind: "ahead",
      label: `${formatMinuteSeconds(expectedStart - elapsedMs)} ahead`
    };
  }
  if (elapsedMs <= expectedEnd) {
    return { kind: "onpace", label: "on pace" };
  }
  return {
    kind: "behind",
    label: `${formatMinuteSeconds(elapsedMs - expectedEnd)} behind`
  };
}

function currentTimerElapsedMs(timer: RemoteTimerAnchor | null, now: number): number {
  if (timer == null) return 0;
  return Math.max(0, timer.elapsedMs + (timer.running ? now - timer.receivedAtMs : 0));
}

function timerVisualState(timer: RemoteTimerAnchor | null, elapsedMs: number): TimerVisualState {
  if (timer == null || (!timer.running && elapsedMs === 0)) return "stopped";
  return timer.running ? "running" : "paused";
}

function playpauseActionFor(state: TimerVisualState): TimerControlDetail["action"] {
  if (state === "running") return "pause";
  if (state === "paused") return "resume";
  return "start";
}

function timerAriaLabel(state: TimerVisualState): string {
  if (state === "running") return "Pause timer";
  if (state === "paused") return "Resume timer";
  return "Start timer";
}

function nextTimerStateForAction(
  action: TimerControlDetail["action"],
  current: RemoteTimerAnchor | null,
  now: number
): RemoteTimerAnchor | null {
  const elapsedMs = currentTimerElapsedMs(current, now);
  const state = timerVisualState(current, elapsedMs);
  if (action === "start") {
    if (state !== "stopped") return null;
    return { running: true, elapsedMs: 0, receivedAtMs: now };
  }
  if (action === "pause") {
    if (state !== "running") return null;
    return { running: false, elapsedMs, receivedAtMs: now };
  }
  if (action === "resume") {
    if (state !== "paused") return null;
    return { running: true, elapsedMs, receivedAtMs: now };
  }
  return { running: false, elapsedMs: 0, receivedAtMs: now };
}

function paneViewport(pane: HTMLElement): () => { width: number; height: number } {
  return () => ({
    width: pane.clientWidth,
    height: pane.clientHeight
  });
}
