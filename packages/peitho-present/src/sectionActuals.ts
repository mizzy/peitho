import type { ManifestSection } from "../../../bindings/ManifestSection";
import { sectionIndexForSlide } from "./sections";
import type {
  PresentShell,
  SlideChangeDetail,
  TimerAdoptDetail,
  TimerControlDetail
} from "./shell";

export type SectionActualsShell = Pick<
  PresentShell,
  "currentIndex" | "elapsedMs" | "startedAt"
>;

export type SectionActuals = {
  actualMs(): readonly number[];
  flush(): void;
  destroy(): void;
};

export type SectionActualsOptions = {
  shell: SectionActualsShell;
  sections: ManifestSection[];
  window?: Window;
  bus?: EventTarget;
  log?: Pick<Console, "error">;
};

export function installSectionActuals(options: SectionActualsOptions): SectionActuals {
  if (options.sections.length === 0) {
    return {
      actualMs: () => [],
      flush: () => undefined,
      destroy: () => undefined
    };
  }

  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const log = options.log ?? console;
  const actualMs = new Array<number>(options.sections.length).fill(0);
  let lastElapsedMs = options.shell.elapsedMs();

  function flushElapsedToSectionOf(slideIndex: number | null): void {
    if (slideIndex === null || options.shell.startedAt() === null) return;
    const elapsedMs = options.shell.elapsedMs();
    const delta = Math.max(0, elapsedMs - lastElapsedMs);
    const sectionIndex = sectionIndexForSlide(options.sections, slideIndex);
    if (sectionIndex >= 0) actualMs[sectionIndex] += delta;
    lastElapsedMs = elapsedMs;
  }

  function onSlideChange(event: Event): void {
    const previousIndex =
      (event as CustomEvent<SlideChangeDetail>).detail?.previousIndex ?? null;
    flushElapsedToSectionOf(previousIndex);
  }

  function onTimerControl(event: Event): void {
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action !== "reset") return;
    actualMs.fill(0);
    lastElapsedMs = 0;
  }

  function onTimerAdopt(event: Event): void {
    const detail = (event as CustomEvent<TimerAdoptDetail>).detail;
    if (
      typeof detail?.running !== "boolean" ||
      typeof detail.elapsedMs !== "number" ||
      !Number.isFinite(detail.elapsedMs) ||
      detail.elapsedMs < 0 ||
      typeof detail.previousElapsedMs !== "number" ||
      !Number.isFinite(detail.previousElapsedMs) ||
      detail.previousElapsedMs < 0
    ) {
      log.error("Invalid peitho:timeradopt event");
      return;
    }
    if (!detail.running && detail.elapsedMs === 0) {
      actualMs.fill(0);
      lastElapsedMs = 0;
      return;
    }
    if (options.shell.startedAt() !== null) {
      const sectionIndex = sectionIndexForSlide(options.sections, options.shell.currentIndex);
      if (sectionIndex >= 0) {
        actualMs[sectionIndex] += Math.max(0, detail.previousElapsedMs - lastElapsedMs);
      }
    }
    lastElapsedMs = detail.elapsedMs;
  }

  function tick(): void {
    if (options.shell.startedAt() === null) {
      actualMs.fill(0);
      lastElapsedMs = 0;
      return;
    }

    flush();
  }

  function flush(): void {
    flushElapsedToSectionOf(options.shell.currentIndex);
  }

  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  bus.addEventListener("peitho:timeradopt", onTimerAdopt);
  const interval = win.setInterval(tick, 250);

  return {
    actualMs: () => actualMs.slice(),
    flush,
    destroy(): void {
      win.clearInterval(interval);
      bus.removeEventListener("peitho:slidechange", onSlideChange);
      bus.removeEventListener("peitho:timercontrol", onTimerControl);
      bus.removeEventListener("peitho:timeradopt", onTimerAdopt);
    }
  };
}
