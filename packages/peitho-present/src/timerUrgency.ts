import { isOverrun } from "./timeTracker";

export type TimerUrgency = "normal" | "warning" | "urgent" | "overrun";

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
