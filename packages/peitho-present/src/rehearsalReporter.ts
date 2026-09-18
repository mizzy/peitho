import type { ManifestSection } from "../../../bindings/ManifestSection";
import type { RehearsalSnapshot } from "../../../bindings/RehearsalSnapshot";
import {
  isTimerAdoptStart,
  isValidTimerAdoptDetail,
  roundNonNegativeMs,
  type TimerAdoptDetail,
  type TimerControlDetail
} from "./shell";
import type { SectionActuals } from "./sectionActuals";
import type { SlideTimeline } from "./slideTimeline";

export type RehearsalReporterShell = {
  elapsedMs(): number;
  startedAt(): number | null;
  isPaused(): boolean;
};

export type RehearsalReportDetail = {
  snapshot: RehearsalSnapshot;
  final: boolean;
};

export type RehearsalReporterOptions = {
  actuals: Pick<SectionActuals, "actualMs" | "flush">;
  timeline: Pick<SlideTimeline, "entries">;
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
  let hasReportedRunStart = hasStarted;
  let timerPaused = hasStarted && options.shell.isPaused();

  function markStarted(): void {
    if (options.shell.startedAt() !== null) hasStarted = true;
  }

  function snapshot(): RehearsalSnapshot {
    const actualMs = options.actuals.actualMs();
    return {
      version: 2,
      elapsedMs: roundNonNegativeMs(options.shell.elapsedMs()),
      sections: options.sections.map((section, index) => ({
        name: section.name,
        plannedDurationMs: section.plannedDurationMs,
        actualMs: roundNonNegativeMs(actualMs[index] ?? 0)
      })),
      timeline: options.timeline.entries()
    };
  }

  function report(final: boolean): void {
    markStarted();
    if (!hasStarted) return;
    options.actuals.flush();
    bus.dispatchEvent(
      new CustomEvent<RehearsalReportDetail>("peitho:rehearsalreport", {
        detail: { snapshot: snapshot(), final }
      })
    );
  }

  function onSlideChange(): void {
    report(false);
  }

  function onTimerControl(event: Event): void {
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action === "start") {
      timerPaused = false;
      hasStarted = true;
      report(false);
      hasReportedRunStart = true;
      return;
    }
    if (action === "resume") {
      timerPaused = false;
      markStarted();
      return;
    }
    if (action === "pause") {
      timerPaused = true;
      report(false);
    }
    if (action === "reset") {
      timerPaused = false;
      report(false);
      hasReportedRunStart = false;
    }
  }

  function onTimerAdopt(event: Event): void {
    const detail = (event as CustomEvent<TimerAdoptDetail>).detail;
    if (!isValidTimerAdoptDetail(detail)) return;
    const adoptedStart = isTimerAdoptStart(detail);
    const adoptedPause = !detail.running && detail.elapsedMs > 0;
    const transitionedToPaused = adoptedPause && !timerPaused;
    timerPaused = adoptedPause;
    if (adoptedStart) hasStarted = true;
    if (adoptedStart && !hasReportedRunStart) {
      report(false);
      hasReportedRunStart = true;
      return;
    }
    if (transitionedToPaused) report(false);
    if (!detail.running && detail.elapsedMs === 0 && hasStarted) {
      report(false);
      hasReportedRunStart = false;
    }
  }

  function onCloseRequest(): void {
    report(true);
  }

  function tick(): void {
    markStarted();
    if (!hasStarted || options.shell.startedAt() === null || options.shell.isPaused()) return;
    report(false);
  }

  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  bus.addEventListener("peitho:timeradopt", onTimerAdopt);
  bus.addEventListener("peitho:closerequest", onCloseRequest);
  win.addEventListener("pagehide", onCloseRequest);
  const interval = win.setInterval(tick, 5_000);

  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:timercontrol", onTimerControl);
    bus.removeEventListener("peitho:timeradopt", onTimerAdopt);
    bus.removeEventListener("peitho:closerequest", onCloseRequest);
    win.removeEventListener("pagehide", onCloseRequest);
  };
}
