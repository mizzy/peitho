import { expect, it } from "vitest";
import { urgencyFor } from "../src/timerUrgency";

it.each([
  ["planned duration is unset", 120_000, null, "normal"],
  ["remaining time is above three minutes", 119_999, 300_000, "normal"],
  ["remaining time is exactly three minutes", 120_000, 300_000, "warning"],
  ["remaining time is one minute and one second", 239_000, 300_000, "warning"],
  ["remaining time is exactly one minute", 240_000, 300_000, "urgent"],
  ["remaining time is one second", 299_000, 300_000, "urgent"],
  ["remaining time is zero", 300_000, 300_000, "urgent"],
  ["elapsed time exceeds the planned duration", 300_001, 300_000, "overrun"],
  ["two minute plans start at warning", 0, 120_000, "warning"],
  ["thirty second plans start at urgent", 0, 30_000, "urgent"]
] as const)("returns %s", (_label, elapsedMs, plannedDurationMs, expected) => {
  expect(urgencyFor(elapsedMs, plannedDurationMs)).toBe(expected);
});
