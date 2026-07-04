import type { Notes } from "../../../bindings/Notes";
import { installAgenda } from "./agenda";
import { installPresenterKeyboard } from "./keyboard";
import { sectionIndexForSlide } from "./sections";
import {
  mountPresentShell,
  type PresentShell,
  type SlideChangeDetail,
  type TimerControlDetail
} from "./shell";
import { installSyncBridge, type SyncChannelFactory } from "./sync";
import { installTimeTracker, isOverrun, isValidDurationMs } from "./timeTracker";

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

type TimerState = "stopped" | "running" | "paused";

const STATE_LABELS: Record<TimerState, string> = {
  stopped: "Stopped",
  running: "Running",
  paused: "Paused"
};

const PLAY_LABELS: Record<TimerState, string> = {
  stopped: "Start",
  running: "Pause",
  paused: "Resume"
};

function deriveTimerState(shell: PresentShell): TimerState {
  if (shell.startedAt() === null) return "stopped";
  return shell.isPaused() ? "paused" : "running";
}

function playpauseActionFor(state: TimerState): TimerControlDetail["action"] {
  if (state === "stopped") return "start";
  if (state === "paused") return "resume";
  return "pause";
}

function formatSlideNumber(value: number): string {
  return value.toString().padStart(2, "0");
}

function renderPresenterTimer(
  doc: Document,
  root: HTMLElement,
  elapsedMs: number,
  plannedDurationMs: number | null
): void {
  if (plannedDurationMs == null) {
    root.textContent = formatElapsed(elapsedMs);
    return;
  }

  root.replaceChildren(doc.createTextNode(formatElapsed(elapsedMs)));
  const planned = doc.createElement("span");
  planned.className = "planned";
  planned.textContent = ` / ${formatElapsed(plannedDurationMs)}`;
  root.appendChild(planned);

  if (!isOverrun(elapsedMs, plannedDurationMs)) return;
  const overrun = doc.createElement("span");
  overrun.className = "overrun";
  overrun.textContent = ` +${formatOverrun(elapsedMs - plannedDurationMs)}`;
  root.appendChild(overrun);
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
    <section class="peitho-presenter app" data-screen-label="Presenter view">
      <section class="left" aria-label="Current slide and notes">
        <div class="stage">
          <header class="colhead">
            <div class="status-line">
              <span data-peitho-presenter="position">Slide 00 of 00</span>
              <span class="sep" data-peitho-presenter="section-sep" hidden></span>
              <span data-peitho-presenter="section" hidden></span>
            </div>
            <div class="deck-title" data-peitho-presenter="title">Peitho Deck</div>
          </header>

          <div class="slide-frame">
            <div
              class="peitho-presenter-pane slide-pane"
              data-peitho-presenter="current"
              role="img"
              aria-label="Current slide preview"
            ></div>
          </div>

          <div class="kbdbar">
            <div class="pos" data-peitho-presenter="position-short">00 / 00</div>
            <div>
              <span class="grp"><span class="kbd">←</span><span class="kbd">→</span> navigate</span>
              <span class="grp"><span class="kbd">Space</span> start / pause</span>
              <span class="grp"><span class="kbd">Esc</span> close</span>
            </div>
          </div>

          <section class="notes" aria-label="Speaker notes">
            <div class="notes-head">
              <span>Notes</span>
              <span class="badge" data-peitho-presenter="notes-slide">Slide 00</span>
            </div>
            <div class="notes-body" data-peitho-presenter="notes"></div>
          </section>
        </div>
      </section>

      <aside class="right">
        <section class="card" aria-label="Next slide">
          <div class="card-head">
            <span>Next</span>
            <span class="badge mono" data-peitho-presenter="next-position">00 / 00</span>
          </div>
          <div class="next-wrap">
            <div class="next-preview">
              <div class="peitho-presenter-pane" data-peitho-presenter="preview"></div>
              <p data-peitho-presenter="preview-end" hidden>End of deck</p>
            </div>
          </div>
        </section>

        <section class="card clock" data-peitho-presenter="clock" data-peitho-state="stopped" aria-label="Timer">
          <div class="clock-row">
            <output class="timer mono" data-peitho-presenter="timer">00:00</output>
            <span class="state-pill" data-peitho-presenter="state-pill" data-peitho-state="stopped">
              <span class="state-dot"></span>
              <span data-peitho-presenter="state-label">Stopped</span>
            </span>
          </div>

          <div class="tracker-wrap" data-peitho-presenter="tracker-slot"></div>
          <div data-peitho-presenter="agenda-slot"></div>

          <div class="controls">
            <button class="btn play primary" type="button" data-peitho-action="playpause"><span data-peitho-presenter="play-label">Start</span> <span class="k">Space</span></button>
            <button class="btn" type="button" data-peitho-action="prev">Prev <span class="k">←</span></button>
            <button class="btn" type="button" data-peitho-action="next">Next <span class="k">→</span></button>
            <button class="btn" type="button" data-peitho-action="reset">Reset</button>
            <button class="btn danger" type="button" data-peitho-action="close">Close <span class="k">Esc</span></button>
          </div>
        </section>
      </aside>
    </section>`;

  const currentRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="current"]')!;
  const previewRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="preview"]')!;
  const previewEnd = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="preview-end"]'
  )!;
  const notesRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="notes"]')!;
  const timerRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="timer"]')!;
  const clockRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="clock"]')!;
  const statePill = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="state-pill"]'
  )!;
  const stateLabel = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="state-label"]'
  )!;
  const playLabel = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="play-label"]'
  )!;
  const playButton = options.root.querySelector<HTMLButtonElement>(
    '[data-peitho-action="playpause"]'
  )!;
  const trackerSlot = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="tracker-slot"]'
  )!;
  const agendaSlot = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="agenda-slot"]'
  )!;
  const deckTitle = options.root.querySelector<HTMLElement>('[data-peitho-presenter="title"]')!;
  const positionLong = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="position"]'
  )!;
  const sectionLabel = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="section"]'
  )!;
  const sectionSep = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="section-sep"]'
  )!;
  const positionShort = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="position-short"]'
  )!;
  const notesSlide = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="notes-slide"]'
  )!;
  const nextPosition = options.root.querySelector<HTMLElement>(
    '[data-peitho-presenter="next-position"]'
  )!;

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
  const keyboardCleanup = installPresenterKeyboard(win, bus, dispatchPlaypause);
  const syncCleanup = installSyncBridge(win, options.syncChannelFactory, bus);
  const rawPlannedDurationMs = mainShell.manifest?.plannedDurationMs ?? null;
  const plannedDurationMs =
    rawPlannedDurationMs != null && isValidDurationMs(rawPlannedDurationMs)
      ? rawPlannedDurationMs
      : null;
  if (rawPlannedDurationMs != null && plannedDurationMs == null) {
    log.error("Invalid plannedDurationMs in manifest.json");
  }
  const trackerCleanup =
    plannedDurationMs == null
      ? () => undefined
      : installTimeTracker({
          root: trackerSlot,
          shell: mainShell,
          plannedDurationMs,
          bus,
          window: win,
          document: doc,
          variant: "presenter"
        });
  const sections = mainShell.manifest?.sections ?? [];
  const agendaCleanup = installAgenda({
    root: agendaSlot,
    shell: mainShell,
    sections,
    bus,
    window: win,
    document: doc,
    log
  });
  const rippleTimeouts = new Set<number>();

  function setTimerStateChrome(state: TimerState): void {
    clockRoot.dataset.peithoState = state;
    statePill.dataset.peithoState = state;
    stateLabel.textContent = STATE_LABELS[state];
    playLabel.textContent = PLAY_LABELS[state];
    playButton.setAttribute("aria-label", PLAY_LABELS[state]);
  }

  function tick(): void {
    const elapsedMs = mainShell.elapsedMs();
    renderPresenterTimer(doc, timerRoot, elapsedMs, plannedDurationMs);
    timerRoot.toggleAttribute(
      "data-peitho-overrun",
      plannedDurationMs != null && isOverrun(elapsedMs, plannedDurationMs)
    );
    setTimerStateChrome(deriveTimerState(mainShell));
  }

  function updateFromSlide(detail: SlideChangeDetail): void {
    const slideNotes = options.notes.notes[detail.key];
    notesRoot.textContent = slideNotes ?? "No notes for this slide.";
    notesRoot.classList.toggle("is-empty", slideNotes == null);
    const slideNumber = detail.index + 1;
    const slide = formatSlideNumber(slideNumber);
    const total = formatSlideNumber(detail.total);
    deckTitle.textContent = mainShell.manifest?.title ?? "Peitho Deck";
    positionLong.textContent = `Slide ${slide} of ${total}`;
    positionShort.textContent = `${slide} / ${total}`;
    notesSlide.textContent = `Slide ${slide}`;
    const currentSectionIndex = sectionIndexForSlide(sections, detail.index);
    if (currentSectionIndex >= 0) {
      sectionLabel.textContent = `Section — “${sections[currentSectionIndex].name}”`;
      sectionLabel.hidden = false;
      sectionSep.hidden = false;
    } else {
      sectionLabel.textContent = "";
      sectionLabel.hidden = true;
      sectionSep.hidden = true;
    }
    const nextIndex = detail.index + 1;
    if (nextIndex < detail.total) {
      previewRoot.hidden = false;
      previewEnd.hidden = true;
      nextPosition.textContent = `${formatSlideNumber(nextIndex + 1)} / ${total}`;
      previewBus.dispatchEvent(
        new CustomEvent("peitho:navigate", { detail: { to: { index: nextIndex } } })
      );
    } else {
      previewRoot.hidden = true;
      previewEnd.hidden = false;
      nextPosition.textContent = "End";
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

  const buttonCleanups: Array<() => void> = [];
  const addButtonListener = (
    action: string,
    listener: (event: MouseEvent) => void
  ): void => {
    const button = options.root.querySelector<HTMLButtonElement>(`[data-peitho-action="${action}"]`);
    if (!button) return;
    button.addEventListener("click", listener);
    buttonCleanups.push(() => button.removeEventListener("click", listener));
  };
  function dispatchTimerControl(action: TimerControlDetail["action"]): void {
    bus.dispatchEvent(
      new CustomEvent<TimerControlDetail>("peitho:timercontrol", { detail: { action } })
    );
    tick();
  }

  function dispatchPlaypause(): void {
    dispatchTimerControl(playpauseActionFor(deriveTimerState(mainShell)));
  }

  addButtonListener("prev", () => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
  });
  addButtonListener("next", () => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  });
  addButtonListener("playpause", dispatchPlaypause);
  addButtonListener("reset", () => {
    dispatchTimerControl("reset");
  });
  addButtonListener("close", () => {
    bus.dispatchEvent(new CustomEvent("peitho:closerequest"));
  });

  const onPointerDown = (event: Event): void => {
    const pointer = event as MouseEvent;
    const target = pointer.target;
    if (!target || !("closest" in target)) return;
    const button = (target as Element).closest<HTMLButtonElement>(".btn");
    if (!button || !options.root.contains(button)) return;
    const rect = button.getBoundingClientRect();
    const width = rect.width || 1;
    const height = rect.height || 1;
    const x = ((pointer.clientX - rect.left) / width) * 100;
    const y = ((pointer.clientY - rect.top) / height) * 100;
    button.style.setProperty("--rx", `${x}%`);
    button.style.setProperty("--ry", `${y}%`);
    button.classList.remove("pressed");
    void button.offsetWidth;
    button.classList.add("pressed");
    const timeout = win.setTimeout(() => {
      button.classList.remove("pressed");
      rippleTimeouts.delete(timeout);
    }, 550);
    rippleTimeouts.add(timeout);
  };
  options.root.addEventListener("pointerdown", onPointerDown);

  const interval = win.setInterval(tick, 250);
  tick();

  return {
    mainShell,
    previewShell,
    tick,
    destroy(): void {
      win.clearInterval(interval);
      for (const timeout of rippleTimeouts) win.clearTimeout(timeout);
      rippleTimeouts.clear();
      options.root.removeEventListener("pointerdown", onPointerDown);
      while (buttonCleanups.length > 0) buttonCleanups.pop()?.();
      agendaCleanup();
      trackerCleanup();
      bus.removeEventListener("peitho:slidechange", onSlideChange);
      keyboardCleanup();
      syncCleanup();
      previewShell.destroy();
      mainShell.destroy();
    }
  };
}
