import type { Notes } from "../../../bindings/Notes";
import { installKeyboardNavigation } from "./keyboard";
import { mountPresentShell, type PresentShell, type SlideChangeDetail } from "./shell";
import { installSyncBridge, type SyncChannelFactory } from "./sync";
import { installTimeTracker, isOverrun } from "./timeTracker";

export type PresenterOptions = {
  root: HTMLElement;
  notes: Notes;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  now?: () => number;
  syncChannelFactory?: SyncChannelFactory;
  console?: Pick<Console, "error">;
};

export type PresenterView = {
  mainShell: PresentShell;
  previewShell: PresentShell;
  tick(): void;
  destroy(): void;
};

function formatSeconds(totalSeconds: number): string {
  const minutes = Math.floor(totalSeconds / 60)
    .toString()
    .padStart(2, "0");
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

function formatElapsed(ms: number): string {
  return formatSeconds(Math.floor(ms / 1000));
}

function formatOverrun(ms: number): string {
  return formatSeconds(Math.ceil(ms / 1000));
}

function formatPresenterTimer(
  elapsedMs: number,
  plannedDurationMs: number | null | undefined
): string {
  if (plannedDurationMs == null) return formatElapsed(elapsedMs);
  const base = `${formatElapsed(elapsedMs)} / ${formatElapsed(plannedDurationMs)}`;
  if (!isOverrun(elapsedMs, plannedDurationMs)) return base;
  return `${base} +${formatOverrun(elapsedMs - plannedDurationMs)}`;
}

function paneViewport(pane: HTMLElement): () => { width: number; height: number } {
  return () => ({
    width: pane.clientWidth,
    height: pane.clientHeight
  });
}

export async function mountPresenterView(options: PresenterOptions): Promise<PresenterView> {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const now = options.now ?? Date.now;
  const log = options.console ?? console;
  const bus = win;
  const previewBus = new EventTarget();
  options.root.innerHTML = `
    <section class="peitho-presenter">
      <div class="peitho-presenter-pane" data-peitho-presenter="current"></div>
      <aside>
        <div class="peitho-presenter-preview-slot">
          <div class="peitho-presenter-pane" data-peitho-presenter="preview"></div>
          <p data-peitho-presenter="preview-end" hidden>End of deck</p>
        </div>
        <section data-peitho-presenter="notes"></section>
        <output data-peitho-presenter="timer">00:00</output>
        <div class="peitho-presenter-controls">
          <button type="button" data-peitho-action="prev">Prev</button>
          <button type="button" data-peitho-action="next">Next</button>
          <button type="button" data-peitho-action="start">Start</button>
          <button type="button" data-peitho-action="pause">Pause</button>
          <button type="button" data-peitho-action="resume">Resume</button>
          <button type="button" data-peitho-action="reset">Reset</button>
          <button type="button" data-peitho-action="close">Close</button>
        </div>
      </aside>
    </section>`;

  const currentRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="current"]')!;
  const previewRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="preview"]')!;
  const previewEnd = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="preview-end"]'
  )!;
  const notesRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="notes"]')!;
  const timerRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="timer"]')!;
  const asideRoot = options.root.querySelector<HTMLElement>("aside")!;

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
  const keyboardCleanup = installKeyboardNavigation(win, bus);
  const syncCleanup = installSyncBridge(win, options.syncChannelFactory, bus);
  const rawPlannedDurationMs = mainShell.manifest?.plannedDurationMs ?? null;
  const plannedDurationMs =
    rawPlannedDurationMs != null &&
    Number.isFinite(rawPlannedDurationMs) &&
    rawPlannedDurationMs > 0
      ? rawPlannedDurationMs
      : null;
  if (rawPlannedDurationMs != null && plannedDurationMs == null) {
    log.error("Invalid plannedDurationMs in manifest.json");
  }
  const trackerCleanup =
    plannedDurationMs == null
      ? () => undefined
      : installTimeTracker({
          root: asideRoot,
          shell: mainShell,
          plannedDurationMs,
          bus,
          window: win,
          document: doc,
          variant: "presenter"
        });

  function tick(): void {
    const elapsedMs = mainShell.elapsedMs();
    timerRoot.textContent = formatPresenterTimer(elapsedMs, plannedDurationMs);
    timerRoot.toggleAttribute(
      "data-peitho-overrun",
      plannedDurationMs != null && isOverrun(elapsedMs, plannedDurationMs)
    );
  }

  function updateFromSlide(detail: SlideChangeDetail): void {
    notesRoot.textContent = options.notes.notes[detail.key] ?? "No notes for this slide.";
    const nextIndex = detail.index + 1;
    if (nextIndex < detail.total) {
      previewRoot.hidden = false;
      previewEnd.hidden = true;
      previewBus.dispatchEvent(
        new CustomEvent("peitho:navigate", { detail: { to: { index: nextIndex } } })
      );
    } else {
      previewRoot.hidden = true;
      previewEnd.hidden = false;
    }
    tick();
  }

  const onSlideChange = (event: Event): void => {
    updateFromSlide((event as CustomEvent<SlideChangeDetail>).detail);
  };
  bus.addEventListener("peitho:slidechange", onSlideChange);

  const firstSlide = mainShell.manifest?.slides[mainShell.currentIndex];
  if (firstSlide) {
    updateFromSlide({
      key: firstSlide.key,
      index: firstSlide.index,
      total: mainShell.manifest?.slideCount ?? 0,
      previousIndex: null
    });
  }

  options.root.querySelector('[data-peitho-action="prev"]')?.addEventListener("click", () => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
  });
  options.root.querySelector('[data-peitho-action="next"]')?.addEventListener("click", () => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  });
  for (const action of ["start", "pause", "resume", "reset"] as const) {
    options.root.querySelector(`[data-peitho-action="${action}"]`)?.addEventListener("click", () => {
      bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action } }));
      tick();
    });
  }
  options.root.querySelector('[data-peitho-action="close"]')?.addEventListener("click", () => {
    bus.dispatchEvent(new CustomEvent("peitho:closerequest"));
  });

  const interval = win.setInterval(tick, 250);

  return {
    mainShell,
    previewShell,
    tick,
    destroy(): void {
      win.clearInterval(interval);
      trackerCleanup();
      bus.removeEventListener("peitho:slidechange", onSlideChange);
      keyboardCleanup();
      syncCleanup();
      previewShell.destroy();
      mainShell.destroy();
    }
  };
}
