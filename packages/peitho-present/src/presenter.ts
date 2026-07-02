import type { Notes } from "../../../bindings/Notes";
import { installKeyboardNavigation } from "./keyboard";
import { mountPresentShell, type PresentShell, type SlideChangeDetail } from "./shell";
import { installSyncBridge, type SyncChannelFactory } from "./sync";

export type PresenterOptions = {
  root: HTMLElement;
  notes: Notes;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  now?: () => number;
  syncChannelFactory?: SyncChannelFactory;
};

export type PresenterView = {
  mainShell: PresentShell;
  previewShell: PresentShell;
  tick(): void;
  destroy(): void;
};

function formatElapsed(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60)
    .toString()
    .padStart(2, "0");
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

export async function mountPresenterView(options: PresenterOptions): Promise<PresenterView> {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const now = options.now ?? Date.now;
  const bus = win;
  const previewBus = new EventTarget();
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
          <button type="button" data-peitho-action="pause">Pause</button>
          <button type="button" data-peitho-action="resume">Resume</button>
          <button type="button" data-peitho-action="reset">Reset</button>
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

  const mainShell = await mountPresentShell({
    root: currentRoot,
    fetcher,
    window: win,
    document: doc,
    bus,
    now
  });
  const previewShell = await mountPresentShell({
    root: previewRoot,
    fetcher,
    window: win,
    document: doc,
    bus: previewBus,
    now
  });
  const keyboardCleanup = installKeyboardNavigation(win, bus);
  const syncCleanup = installSyncBridge(win, options.syncChannelFactory, bus);

  function tick(): void {
    timerRoot.textContent = formatElapsed(mainShell.elapsedMs());
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
  for (const action of ["pause", "resume", "reset"] as const) {
    options.root.querySelector(`[data-peitho-action="${action}"]`)?.addEventListener("click", () => {
      bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action } }));
      tick();
    });
  }

  const interval = win.setInterval(tick, 250);

  return {
    mainShell,
    previewShell,
    tick,
    destroy(): void {
      win.clearInterval(interval);
      bus.removeEventListener("peitho:slidechange", onSlideChange);
      keyboardCleanup();
      syncCleanup();
      previewShell.destroy();
      mainShell.destroy();
    }
  };
}
