import type { ManifestSection } from "../../../bindings/ManifestSection";
import type { PresentShell, SlideChangeDetail, TimerControlDetail } from "./shell";
import { formatMinuteSeconds, isValidDurationMs } from "./timeTracker";

const EM_DASH = "—";
const MINUS_SIGN = "−";

type AgendaState = "done" | "current" | "upcoming";
type AgendaOutcome = "under" | "over";

type AgendaRow = {
  row: HTMLElement;
  time: HTMLElement;
  delta: HTMLElement;
  section: ManifestSection;
};

export type AgendaOptions = {
  root: HTMLElement;
  shell: Pick<PresentShell, "currentIndex" | "elapsedMs" | "startedAt">;
  sections: ManifestSection[];
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  log?: Pick<Console, "error">;
};

export function installAgenda(options: AgendaOptions): () => void {
  if (options.sections.length === 0) return () => undefined;

  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
  const log = options.log ?? console;
  if (!validateSections(options.sections, log)) return () => undefined;
  const host = doc.createElement("section");
  host.dataset.peithoAgenda = "true";
  host.innerHTML = [
    '<div data-peitho-agenda-head>',
    '<span data-peitho-agenda-title>Agenda</span>',
    '<span data-peitho-agenda-hint>Actual / Planned</span>',
    "</div>",
    '<div data-peitho-agenda-list></div>'
  ].join("");
  options.root.appendChild(host);
  const list = host.querySelector<HTMLElement>("[data-peitho-agenda-list]")!;
  const rows = options.sections.map((section) => createRow(doc, section));
  list.append(...rows.map(({ row }) => row));
  const actualMs = new Array<number>(options.sections.length).fill(0);
  let lastElapsedMs = options.shell.elapsedMs();

  function sectionIndexForSlide(slideIndex: number): number {
    return options.sections.findIndex(
      (section) => slideIndex >= section.startIndex && slideIndex <= section.endIndex
    );
  }

  function render(): void {
    const currentSection = sectionIndexForSlide(options.shell.currentIndex);
    rows.forEach((row, index) => updateRow(row, index, currentSection, actualMs[index]));
  }

  function flushElapsedToSectionOf(slideIndex: number | null): void {
    if (slideIndex === null || options.shell.startedAt() === null) return;
    const elapsedMs = options.shell.elapsedMs();
    const delta = Math.max(0, elapsedMs - lastElapsedMs);
    const sectionIndex = sectionIndexForSlide(slideIndex);
    if (sectionIndex >= 0) actualMs[sectionIndex] += delta;
    lastElapsedMs = elapsedMs;
  }

  function onSlideChange(event: Event): void {
    const previousIndex =
      (event as CustomEvent<SlideChangeDetail>).detail?.previousIndex ?? null;
    flushElapsedToSectionOf(previousIndex);
    render();
  }

  function onTimerControl(event: Event): void {
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action !== "reset") return;
    actualMs.fill(0);
    lastElapsedMs = 0;
    render();
  }

  function tick(): void {
    if (options.shell.startedAt() === null) {
      // Reset events zero actuals immediately in onTimerControl; this branch keeps
      // the stopped-state display zeroed when the shell is already stopped.
      actualMs.fill(0);
      lastElapsedMs = 0;
      render();
      return;
    }

    flushElapsedToSectionOf(options.shell.currentIndex);
    render();
  }

  render();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  const interval = win.setInterval(tick, 250);

  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:timercontrol", onTimerControl);
    host.remove();
  };
}

function validateSections(
  sections: ManifestSection[],
  log: Pick<Console, "error">
): boolean {
  let expectedStartIndex = 0;
  for (const [index, section] of sections.entries()) {
    const label = `manifest section ${index + 1} "${section.name}"`;
    if (!isValidDurationMs(section.plannedDurationMs)) {
      log.error(`Invalid plannedDurationMs for ${label} in manifest.json`);
      return false;
    }
    if (!isValidSlideIndex(section.startIndex) || !isValidSlideIndex(section.endIndex)) {
      log.error(
        `Invalid ${label}: startIndex and endIndex must be non-negative integers`
      );
      return false;
    }
    if (section.endIndex < section.startIndex) {
      log.error(`Invalid ${label}: endIndex must be greater than or equal to startIndex`);
      return false;
    }
    if (section.startIndex !== expectedStartIndex) {
      log.error(
        `Invalid ${label}: expected startIndex ${expectedStartIndex}, got ${section.startIndex}`
      );
      return false;
    }
    expectedStartIndex = section.endIndex + 1;
  }
  return true;
}

function isValidSlideIndex(index: number): boolean {
  return Number.isSafeInteger(index) && index >= 0;
}

function createRow(doc: Document, section: ManifestSection): AgendaRow {
  const row = doc.createElement("div");
  row.dataset.peithoAgendaRow = "true";
  row.innerHTML = [
    '<span data-peitho-agenda-marker aria-hidden="true"></span>',
    '<span data-peitho-agenda-label><span data-peitho-agenda-name></span><span data-peitho-agenda-range></span></span>',
    '<span data-peitho-agenda-time></span>',
    '<span data-peitho-agenda-delta></span>'
  ].join("");
  row.querySelector("[data-peitho-agenda-name]")!.textContent = section.name;
  row.querySelector("[data-peitho-agenda-range]")!.textContent = formatSlideRange(section);
  return {
    row,
    section,
    time: row.querySelector("[data-peitho-agenda-time]")!,
    delta: row.querySelector("[data-peitho-agenda-delta]")!
  };
}

function updateRow(
  view: AgendaRow,
  index: number,
  currentSection: number,
  actual: number
): void {
  const state = agendaState(index, currentSection);
  view.row.dataset.peithoAgendaState = state;
  if (state === "done") {
    view.row.dataset.peithoAgendaOutcome = outcomeFor(actual, view.section.plannedDurationMs);
  } else {
    delete view.row.dataset.peithoAgendaOutcome;
  }
  view.time.textContent = `${actualText(state, actual)} / ${formatMinuteSeconds(
    view.section.plannedDurationMs
  )}`;
  view.delta.textContent = deltaText(state, actual, view.section.plannedDurationMs);
}

function agendaState(index: number, currentSection: number): AgendaState {
  if (index < currentSection) return "done";
  if (index === currentSection) return "current";
  return "upcoming";
}

function actualText(state: AgendaState, actual: number): string {
  return state === "upcoming" ? EM_DASH : formatMinuteSeconds(actual);
}

function formatSlideRange(section: ManifestSection): string {
  const start = String(section.startIndex + 1).padStart(2, "0");
  const end = String(section.endIndex + 1).padStart(2, "0");
  return section.startIndex === section.endIndex ? start : `${start}–${end}`;
}

function deltaText(state: AgendaState, actual: number, planned: number): string {
  if (state !== "done") return "·";
  const diffSec = diffSeconds(actual, planned);
  const sign = diffSec > 0 ? "+" : MINUS_SIGN;
  return `${sign}${formatMinuteSeconds(Math.abs(diffSec) * 1000)}`;
}

function outcomeFor(actual: number, planned: number): AgendaOutcome {
  return diffSeconds(actual, planned) > 0 ? "over" : "under";
}

function diffSeconds(actual: number, planned: number): number {
  return Math.round((actual - planned) / 1000);
}
