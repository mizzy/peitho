import type { ManifestSection } from "../../../bindings/ManifestSection";
import { sectionIndexForSlide, validateSections } from "./sections";
import type { PresentShell } from "./shell";
import type { SectionActuals } from "./sectionActuals";
import { formatMinuteSeconds } from "./timeTracker";

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
  actuals: Pick<SectionActuals, "actualMs">;
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
  const rawLog = options.log ?? console;
  const log = { error: rawLog.error };
  if (!validateSections(options.sections, log)) return () => undefined;
  const host = doc.createElement("section");
  host.dataset.peithoAgenda = "true";
  host.innerHTML = [
    '<div data-peitho-agenda-head>',
    '<span data-peitho-agenda-title>Agenda</span>',
    '<span data-peitho-agenda-hint>Actual / Planned</span>',
    '<span data-peitho-agenda-head-spacer aria-hidden="true"></span>',
    "</div>",
    '<div data-peitho-agenda-list></div>'
  ].join("");
  options.root.appendChild(host);
  const list = host.querySelector<HTMLElement>("[data-peitho-agenda-list]")!;
  const rows = options.sections.map((section) => createRow(doc, section));
  list.append(...rows.map(({ row }) => row));

  function render(): void {
    const currentSection = sectionIndexForSlide(options.sections, options.shell.currentIndex);
    const actuals = options.actuals.actualMs();
    rows.forEach((row, index) =>
      updateRow(row, index, currentSection, Math.max(0, actuals[index] ?? 0))
    );
  }

  function onSlideChange(event: Event): void {
    void event;
    render();
  }

  function tick(): void {
    render();
  }

  render();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onSlideChange);
  bus.addEventListener("peitho:timeradopt", onSlideChange);
  const interval = win.setInterval(tick, 250);

  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:timercontrol", onSlideChange);
    bus.removeEventListener("peitho:timeradopt", onSlideChange);
    host.remove();
  };
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
  const outcome =
    state === "upcoming" ? null : outcomeFor(actual, view.section.plannedDurationMs);
  if (outcome === "over" || (state === "done" && outcome === "under")) {
    view.row.dataset.peithoAgendaOutcome = outcome;
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
  return actual > 0 || state !== "upcoming" ? formatMinuteSeconds(actual) : EM_DASH;
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
