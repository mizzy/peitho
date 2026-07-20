import type { PresentShell } from "./shell";
import { isOverrun } from "./timeTracker";

const URGENCY_ORDER = ["normal", "warning", "urgent", "overrun"] as const;

export type TimerUrgency = (typeof URGENCY_ORDER)[number];

export type UrgencyChangeDetail = { from: TimerUrgency; to: TimerUrgency };

export type UrgencyTracker = { destroy(): void };

export type UrgencyTrackerShell = Pick<PresentShell, "elapsedMs" | "startedAt">;

export type UrgencyTrackerOptions = {
  shell: UrgencyTrackerShell;
  plannedDurationMs: number | null;
  window?: Window;
  bus: EventTarget;
};

const URGENCY_RANK = {
  normal: 0,
  warning: 1,
  urgent: 2,
  overrun: 3
} as const satisfies Record<TimerUrgency, number>;

export function urgencyFor(
  elapsedMs: number,
  plannedDurationMs: number | null
): TimerUrgency {
  if (plannedDurationMs == null) return "normal";
  if (isOverrun(elapsedMs, plannedDurationMs)) return "overrun";

  const remainingMs = plannedDurationMs - elapsedMs;
  if (remainingMs <= 60_000) return "urgent";
  if (remainingMs <= 180_000) return "warning";
  return "normal";
}

export function installUrgencyTracker(options: UrgencyTrackerOptions): UrgencyTracker {
  const win = options.window ?? window;
  const bus = options.bus;
  let lastKnown: TimerUrgency | null = null;

  const emit = (from: TimerUrgency, to: TimerUrgency): void => {
    bus.dispatchEvent(
      new CustomEvent<UrgencyChangeDetail>("peitho:urgencychange", {
        detail: { from, to }
      })
    );
  };

  const tick = (): void => {
    if (options.shell.startedAt() === null) {
      lastKnown = null;
      return;
    }
    const target = urgencyFor(options.shell.elapsedMs(), options.plannedDurationMs);
    if (lastKnown === null) {
      lastKnown = target;
      return;
    }
    const fromIndex = URGENCY_RANK[lastKnown];
    const toIndex = URGENCY_RANK[target];
    if (toIndex <= fromIndex) {
      lastKnown = target;
      return;
    }
    for (let index = fromIndex; index < toIndex; index += 1) {
      emit(URGENCY_ORDER[index], URGENCY_ORDER[index + 1]);
    }
    lastKnown = target;
  };

  const interval = win.setInterval(tick, 250);

  return {
    destroy(): void {
      win.clearInterval(interval);
    }
  };
}
