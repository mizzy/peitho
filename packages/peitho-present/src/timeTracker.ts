import type { SlideChangeDetail, TimerControlDetail } from "./shell";

export type TimeTrackerShell = {
  manifest: { slideCount: number } | null;
  currentIndex: number;
  elapsedMs(): number;
};

export type TimeTrackerOptions = {
  root: HTMLElement;
  shell: TimeTrackerShell;
  plannedDurationMs: number;
  window?: Window;
  document?: Document;
  console?: Pick<Console, "error">;
  bus?: EventTarget;
  variant?: "present" | "presenter";
};

const clamp01 = (ratio: number): number => Math.min(Math.max(ratio, 0), 1);

export function isOverrun(elapsedMs: number, plannedDurationMs: number): boolean {
  return elapsedMs > plannedDurationMs;
}

function isValidSlideChangeDetail(detail: unknown): detail is SlideChangeDetail {
  if (typeof detail !== "object" || detail === null) return false;
  const candidate = detail as Partial<SlideChangeDetail>;
  const { index, previousIndex, total } = candidate;
  return (
    typeof index === "number" &&
    Number.isFinite(index) &&
    index >= 0 &&
    typeof total === "number" &&
    Number.isFinite(total) &&
    total > 0 &&
    (previousIndex === null ||
      (typeof previousIndex === "number" && Number.isFinite(previousIndex) && previousIndex >= 0))
  );
}

export function installTimeTracker(options: TimeTrackerOptions): () => void {
  if (!Number.isFinite(options.plannedDurationMs) || options.plannedDurationMs <= 0) {
    throw new Error("plannedDurationMs must be a positive finite number");
  }
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const log = options.console ?? console;
  const bus = options.bus ?? win;
  const track = doc.createElement("div");
  track.className = "peitho-time-tracker";
  track.dataset.peithoTimeTracker = options.variant ?? "present";
  track.innerHTML = [
    '<span data-peitho-marker="rabbit" aria-label="slide progress">🐰</span>',
    '<span data-peitho-marker="turtle" aria-label="time progress">🐢</span>'
  ].join("");
  options.root.appendChild(track);

  const rabbit = track.querySelector<HTMLElement>('[data-peitho-marker="rabbit"]')!;
  const turtle = track.querySelector<HTMLElement>('[data-peitho-marker="turtle"]')!;
  let autoStarted = false;

  const setMarker = (element: HTMLElement, ratio: number): void => {
    element.style.left = `${Math.round(ratio * 10_000) / 100}%`;
  };
  const updateSlides = (index: number, total: number): void => {
    const ratio = total <= 1 ? 0 : index / (total - 1);
    setMarker(rabbit, clamp01(ratio));
  };
  const tick = (): void => {
    const elapsedMs = options.shell.elapsedMs();
    const ratio = elapsedMs / options.plannedDurationMs;
    setMarker(turtle, clamp01(ratio));
    track.toggleAttribute(
      "data-peitho-overrun",
      isOverrun(elapsedMs, options.plannedDurationMs)
    );
  };
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<unknown>).detail;
    if (!isValidSlideChangeDetail(detail)) {
      log.error("Invalid peitho:slidechange event");
      return;
    }
    updateSlides(detail.index, detail.total);
    if (!autoStarted && detail.previousIndex !== null && detail.index > detail.previousIndex) {
      autoStarted = true;
      bus.dispatchEvent(
        new CustomEvent<TimerControlDetail>("peitho:timercontrol", {
          detail: { action: "start" }
        })
      );
    }
  };

  updateSlides(options.shell.currentIndex, options.shell.manifest?.slideCount ?? 0);
  tick();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  const interval = win.setInterval(tick, 250);

  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    track.remove();
  };
}
