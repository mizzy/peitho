import type { RehearsalSlideEntry } from "../../../bindings/RehearsalSlideEntry";
import {
  isTimerAdoptStart,
  isValidTimerAdoptDetail,
  roundNonNegativeMs,
  type PresentShell,
  type SlideChangeDetail,
  type TimerAdoptDetail,
  type TimerControlDetail
} from "./shell";

export type SlideTimelineShell = Pick<
  PresentShell,
  "manifest" | "currentIndex" | "elapsedMs" | "startedAt"
>;

export type SlideTimeline = {
  entries(): RehearsalSlideEntry[];
  destroy(): void;
};

export type SlideTimelineOptions = {
  shell: SlideTimelineShell;
  bus?: EventTarget;
  log?: Pick<Console, "error">;
};

export function installSlideTimeline(options: SlideTimelineOptions): SlideTimeline {
  const bus = options.bus ?? window;
  const log = options.log ?? console;
  const entries: RehearsalSlideEntry[] = [];
  let hasStarted = options.shell.startedAt() !== null;

  function append(entry: RehearsalSlideEntry): void {
    const previous = entries.at(-1);
    if (
      previous?.key === entry.key &&
      previous.index === entry.index &&
      previous.atMs === entry.atMs
    ) {
      return;
    }
    entries.push(entry);
  }

  function currentSlideEntry(atMs: number): RehearsalSlideEntry | null {
    const { currentIndex, manifest } = options.shell;
    const slide = manifest?.slides[currentIndex];
    if (slide == null || slide.index !== currentIndex) return null;
    return { key: slide.key, index: slide.index, atMs };
  }

  function onPresentationStart(): void {
    hasStarted = true;
    const entry = currentSlideEntry(0);
    if (entry == null) {
      log.error("Invalid current slide for peitho:presentationstart event");
      return;
    }
    append(entry);
  }

  function onSlideChange(event: Event): void {
    const detail = (event as CustomEvent<unknown>).detail;
    if (!isValidSlideChangeDetail(detail, options.shell)) {
      log.error("Invalid peitho:slidechange event");
      return;
    }
    if (!hasStarted) return;
    const atMs = timelinePosition(options.shell.elapsedMs());
    if (atMs == null) {
      log.error("Invalid rehearsal timeline elapsed time");
      return;
    }
    append({ key: detail.key, index: detail.index, atMs });
  }

  function onTimerControl(event: Event): void {
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action !== "reset") return;
    entries.length = 0;
    hasStarted = false;
  }

  function onTimerAdopt(event: Event): void {
    const detail = (event as CustomEvent<TimerAdoptDetail>).detail;
    if (!isValidTimerAdoptDetail(detail)) {
      log.error("Invalid peitho:timeradopt event");
      return;
    }
    if (!detail.running && detail.elapsedMs === 0) {
      entries.length = 0;
      hasStarted = false;
      return;
    }
    const adoptedAtMs = timelinePosition(detail.elapsedMs);
    if (adoptedAtMs == null) {
      log.error("Invalid rehearsal timeline elapsed time");
      return;
    }
    // Adoption rebases the shared timer; existing timeline positions follow the same truth.
    for (const entry of entries) {
      if (entry.atMs > adoptedAtMs) entry.atMs = adoptedAtMs;
    }
    if (!isTimerAdoptStart(detail)) return;
    hasStarted = true;
    if (entries.length > 0) return;
    const entry = currentSlideEntry(adoptedAtMs);
    if (entry == null) {
      log.error("Invalid current slide for peitho:timeradopt event");
      return;
    }
    append(entry);
  }

  bus.addEventListener("peitho:presentationstart", onPresentationStart);
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  bus.addEventListener("peitho:timeradopt", onTimerAdopt);

  return {
    entries: () => entries.map((entry) => ({ ...entry })),
    destroy(): void {
      bus.removeEventListener("peitho:presentationstart", onPresentationStart);
      bus.removeEventListener("peitho:slidechange", onSlideChange);
      bus.removeEventListener("peitho:timercontrol", onTimerControl);
      bus.removeEventListener("peitho:timeradopt", onTimerAdopt);
    }
  };
}

function isValidSlideChangeDetail(
  detail: unknown,
  shell: SlideTimelineShell
): detail is SlideChangeDetail {
  if (typeof detail !== "object" || detail === null) return false;
  const candidate = detail as Partial<SlideChangeDetail>;
  const slide = shell.manifest?.slides[candidate.index as number];
  return (
    slide != null &&
    candidate.total === shell.manifest?.slideCount &&
    slide.index === candidate.index &&
    slide.key === candidate.key
  );
}

function timelinePosition(ms: number): number | null {
  if (!Number.isFinite(ms) || ms < 0) return null;
  const rounded = roundNonNegativeMs(ms);
  return Number.isSafeInteger(rounded) ? rounded : null;
}
