import type { Manifest } from "../../../bindings/Manifest";
import { initialSlideIndex, nextNonSkippedIndex } from "./skipnav";
import {
  isCloseSyncMessage,
  isGenerationSyncMessage,
  isIndexSyncMessage,
  isSwappedSyncMessage,
  serverSyncChannelFactory,
  type SyncChannelFactory
} from "./sync";

type RemoteSlide = { skip: boolean };

export type RemoteView = {
  manifest: Manifest | null;
  currentIndex: number | null;
  destroy(): void;
};

export type RemoteViewOptions = {
  root: HTMLElement;
  manifestUrl?: string;
  fetcher?: typeof fetch;
  channelFactory?: SyncChannelFactory;
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  console?: Pick<Console, "error">;
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
  getCurrentIndex(): number | null;
  setCurrentIndex(index: number): void;
  setEnded(): void;
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

  const counter = doc.createElement("div");
  counter.className = "peitho-remote-counter";
  counter.dataset.peithoRemote = "counter";
  counter.textContent = "– / –";

  const status = doc.createElement("div");
  status.className = "peitho-remote-status";
  status.dataset.peithoRemote = "status";

  const actions = doc.createElement("div");
  actions.className = "peitho-remote-actions";

  const prev = remoteButton(doc, "prev", "Previous");
  const next = remoteButton(doc, "next", "Next");

  const onPrev = (): void => dispatchNavigate(bus, "prev");
  const onNext = (): void => dispatchNavigate(bus, "next");
  prev.addEventListener("click", onPrev);
  next.addEventListener("click", onNext);

  actions.append(prev, next);
  container.append(counter, status, actions);
  root.append(container);

  return () => {
    prev.removeEventListener("click", onPrev);
    next.removeEventListener("click", onNext);
    container.remove();
  };
}

export function installRemoteSyncBridge(options: RemoteSyncBridgeOptions): () => void {
  const bus = options.bus ?? window;
  const log = options.console ?? console;
  const channel = (options.channelFactory ?? serverSyncChannelFactory())("peitho-sync");

  const onNavigate = (event: Event): void => {
    const to = (event as CustomEvent<{ to?: unknown }>).detail?.to;
    if (to !== "next" && to !== "prev") return;
    const target = resolveRemoteTarget(options.slides, options.getCurrentIndex(), to);
    if (target === null) return;
    channel.postMessage({ index: target });
    options.setCurrentIndex(target);
  };

  channel.onmessage = (event: { data: unknown }): void => {
    const data = event.data;
    if (isCloseSyncMessage(data)) {
      options.setEnded();
      return;
    }
    if (isIndexSyncMessage(data)) {
      options.setCurrentIndex(data.index);
      return;
    }
    if (isSwappedSyncMessage(data) || isGenerationSyncMessage(data)) return;
    log.error("Invalid peitho remote sync message");
  };

  bus.addEventListener("peitho:navigate", onNavigate);
  return () => {
    bus.removeEventListener("peitho:navigate", onNavigate);
    channel.onmessage = null;
    channel.close();
  };
}

class RemoteController implements RemoteView {
  manifest: Manifest | null = null;
  currentIndex: number | null = null;
  private readonly root: HTMLElement;
  private readonly manifestUrl: string;
  private readonly fetcher: typeof fetch;
  private readonly channelFactory?: SyncChannelFactory;
  private readonly win: Window;
  private readonly doc: Document;
  private readonly bus: EventTarget;
  private readonly log: Pick<Console, "error">;
  private slides: RemoteSlide[] = [];
  private controlsCleanup: (() => void) | null = null;
  private syncCleanup: (() => void) | null = null;

  constructor(options: RemoteViewOptions) {
    this.root = options.root;
    this.manifestUrl = options.manifestUrl ?? "manifest.json";
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.channelFactory = options.channelFactory;
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.bus = options.bus ?? this.win;
    this.log = options.console ?? console;
  }

  async load(): Promise<void> {
    try {
      const manifest = await this.fetchJson<Manifest>(this.manifestUrl);
      this.manifest = manifest;
      this.slides = manifest.slides.map((slide) => ({ skip: slide.skip === true }));
      this.currentIndex = initialSlideIndex(this.slides);
      this.controlsCleanup = installRemoteControls({
        root: this.root,
        document: this.doc,
        bus: this.bus
      });
      this.renderCounter();
      this.syncCleanup = installRemoteSyncBridge({
        slides: this.slides,
        channelFactory: this.channelFactory,
        bus: this.bus,
        getCurrentIndex: () => this.currentIndex,
        setCurrentIndex: (index) => this.setCurrentIndex(index),
        setEnded: () => this.setEnded(),
        console: this.log
      });
    } catch (error) {
      this.showError(error instanceof Error ? error.message : String(error));
    }
  }

  destroy(): void {
    this.syncCleanup?.();
    this.syncCleanup = null;
    this.controlsCleanup?.();
    this.controlsCleanup = null;
  }

  private async fetchJson<T>(url: string): Promise<T> {
    const response = await this.fetcher(url);
    if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
    return response.json() as Promise<T>;
  }

  private setCurrentIndex(index: number): void {
    this.currentIndex = clampIndex(index, this.slides.length);
    this.renderCounter();
  }

  private setEnded(): void {
    const cleanup = this.syncCleanup;
    this.syncCleanup = null;
    cleanup?.();
    for (const button of this.root.querySelectorAll<HTMLButtonElement>("[data-peitho-action]")) {
      button.disabled = true;
    }
    const status = this.root.querySelector<HTMLElement>('[data-peitho-remote="status"]');
    if (status != null) status.textContent = "Ended";
  }

  private renderCounter(): void {
    const counter = this.root.querySelector<HTMLElement>('[data-peitho-remote="counter"]');
    if (counter == null) return;
    const total = this.slides.length;
    counter.textContent = this.currentIndex === null ? `– / ${total}` : `${this.currentIndex + 1} / ${total}`;
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
  button.dataset.peithoAction = action;
  button.textContent = label;
  return button;
}

function dispatchNavigate(bus: EventTarget, to: "prev" | "next"): void {
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
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
