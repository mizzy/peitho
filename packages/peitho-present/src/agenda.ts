import type { ManifestSection } from "../../../bindings/ManifestSection";
import type { PresentShell } from "./shell";
import { formatMinuteSeconds } from "./timeTracker";

const EM_DASH = "—";
const MINUS_SIGN = "−";

type AgendaState = "done" | "current" | "upcoming";

export type AgendaOptions = {
  root: HTMLElement;
  shell: Pick<PresentShell, "currentIndex" | "elapsedMs" | "startedAt">;
  sections: ManifestSection[];
  window?: Window;
  document?: Document;
  bus?: EventTarget;
};

export function installAgenda(options: AgendaOptions): () => void {
  if (options.sections.length === 0) return () => undefined;

  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
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
  const actualMs = new Array<number>(options.sections.length).fill(0);
  let lastElapsedMs = options.shell.elapsedMs();

  function sectionIndexForSlide(slideIndex: number): number {
    return options.sections.findIndex(
      (section) => slideIndex >= section.startIndex && slideIndex <= section.endIndex
    );
  }

  function render(): void {
    const currentSection = sectionIndexForSlide(options.shell.currentIndex);
    list.replaceChildren(
      ...options.sections.map((section, index) =>
        renderRow(doc, section, index, currentSection, actualMs[index])
      )
    );
  }

  function onSlideChange(): void {
    render();
  }

  function tick(): void {
    const elapsedMs = options.shell.elapsedMs();
    if (options.shell.startedAt() === null) {
      actualMs.fill(0);
      lastElapsedMs = 0;
      render();
      return;
    }

    const delta = Math.max(0, elapsedMs - lastElapsedMs);
    const sectionIndex = sectionIndexForSlide(options.shell.currentIndex);
    if (sectionIndex >= 0) actualMs[sectionIndex] += delta;
    lastElapsedMs = elapsedMs;
    render();
  }

  render();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  const interval = win.setInterval(tick, 250);

  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    host.remove();
  };
}

function renderRow(
  doc: Document,
  section: ManifestSection,
  index: number,
  currentSection: number,
  actual: number
): HTMLElement {
  const state = agendaState(index, currentSection);
  const row = doc.createElement("div");
  row.dataset.peithoAgendaRow = "true";
  row.dataset.peithoAgendaState = state;
  if (state === "done") {
    row.dataset.peithoAgendaDelta = actual > section.plannedDurationMs ? "over" : "under";
  }
  row.innerHTML = [
    '<span data-peitho-agenda-marker aria-hidden="true"></span>',
    '<span data-peitho-agenda-label><span data-peitho-agenda-name></span><span data-peitho-agenda-range></span></span>',
    '<span data-peitho-agenda-time></span>',
    '<span data-peitho-agenda-delta></span>'
  ].join("");
  row.querySelector("[data-peitho-agenda-name]")!.textContent = section.name;
  row.querySelector("[data-peitho-agenda-range]")!.textContent = formatSlideRange(section);
  row.querySelector("[data-peitho-agenda-time]")!.textContent = `${actualText(
    state,
    actual
  )} / ${formatMinuteSeconds(section.plannedDurationMs)}`;
  row.querySelector("[data-peitho-agenda-delta]")!.textContent = deltaText(
    state,
    actual,
    section.plannedDurationMs
  );
  return row;
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
  const diff = actual - planned;
  const sign = diff > 0 ? "+" : MINUS_SIGN;
  return `${sign}${formatMinuteSeconds(Math.abs(diff))}`;
}
