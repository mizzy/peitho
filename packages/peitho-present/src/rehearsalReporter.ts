import type { ManifestSection } from "../../../bindings/ManifestSection";
import type { RehearsalSnapshot } from "../../../bindings/RehearsalSnapshot";
import type { TimerAdoptDetail, TimerControlDetail } from "./shell";
import type { SectionActuals } from "./sectionActuals";

export type RehearsalReporterShell = {
  elapsedMs(): number;
  startedAt(): number | null;
  isPaused(): boolean;
};

export type RehearsalReporterOptions = {
  actuals: Pick<SectionActuals, "actualMs" | "flush">;
  shell: RehearsalReporterShell;
  sections: ManifestSection[];
  window?: Window;
  bus?: EventTarget;
};

export function installRehearsalReporter(options: RehearsalReporterOptions): () => void {
  if (options.sections.length === 0) return () => undefined;

  const win = options.window ?? window;
  const bus = options.bus ?? win;
  let hasStarted = options.shell.startedAt() !== null;

  function markStarted(): void {
    if (options.shell.startedAt() !== null) hasStarted = true;
  }

  function snapshot(): RehearsalSnapshot {
    const actualMs = options.actuals.actualMs();
    return {
      version: 1,
      elapsedMs: roundNonNegativeMs(options.shell.elapsedMs()),
      sections: options.sections.map((section, index) => ({
        name: section.name,
        plannedDurationMs: section.plannedDurationMs,
        actualMs: roundNonNegativeMs(actualMs[index] ?? 0)
      }))
    };
  }

  function report(): void {
    markStarted();
    if (!hasStarted) return;
    options.actuals.flush();
    bus.dispatchEvent(
      new CustomEvent<RehearsalSnapshot>("peitho:rehearsalreport", {
        detail: snapshot()
      })
    );
  }

  function onSlideChange(): void {
    report();
  }

  function onTimerControl(event: Event): void {
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action === "start" || action === "resume") {
      markStarted();
      return;
    }
    if (action === "pause" || action === "reset") report();
  }

  function onTimerAdopt(event: Event): void {
    const detail = (event as CustomEvent<TimerAdoptDetail>).detail;
    if (!isValidTimerAdoptDetail(detail)) return;
    if (detail.running || detail.elapsedMs > 0) hasStarted = true;
    if (!detail.running && detail.elapsedMs === 0 && hasStarted) report();
  }

  function onCloseRequest(): void {
    report();
  }

  function tick(): void {
    markStarted();
    if (!hasStarted || options.shell.startedAt() === null || options.shell.isPaused()) return;
    report();
  }

  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  bus.addEventListener("peitho:timeradopt", onTimerAdopt);
  bus.addEventListener("peitho:closerequest", onCloseRequest);
  const interval = win.setInterval(tick, 5_000);

  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:timercontrol", onTimerControl);
    bus.removeEventListener("peitho:timeradopt", onTimerAdopt);
    bus.removeEventListener("peitho:closerequest", onCloseRequest);
  };
}

function isValidTimerAdoptDetail(detail: TimerAdoptDetail | undefined): detail is TimerAdoptDetail {
  return (
    typeof detail?.running === "boolean" &&
    typeof detail.elapsedMs === "number" &&
    Number.isFinite(detail.elapsedMs) &&
    detail.elapsedMs >= 0 &&
    typeof detail.previousElapsedMs === "number" &&
    Number.isFinite(detail.previousElapsedMs) &&
    detail.previousElapsedMs >= 0
  );
}

function roundNonNegativeMs(ms: number): number {
  return Math.max(0, Math.round(ms));
}
