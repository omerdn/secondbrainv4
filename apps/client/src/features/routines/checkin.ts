/**
 * Check-in and heatmap model (PRD R4, R7, R8, T2.2.6). Pure functions over the server's
 * `RoutineDay`s; days are `YYYY-MM-DD` strings in the routine's own zone.
 */
import type { RoutineDay } from "@secondbrain/api-types";

import { addDays } from "@/lib/dates";

/** How many days back a day can still be marked (R7); the server checks it too. */
export const BACKDATE_DAYS = 3;
/** Weeks the heatmap shows, the current one included. */
export const HEATMAP_WEEKS = 15;

/** A heatmap square: `off` without an occurrence, otherwise 0 (nothing) to 4 (complete). */
export type HeatLevel = "off" | 0 | 1 | 2 | 3 | 4;

/** Share of the day's target that was done, from 0 to 1 (R4: partial days count). */
export function completionShare(day: Pick<RoutineDay, "done" | "target">): number {
  if (day.target <= 0) {
    return day.done > 0 ? 1 : 0;
  }
  return Math.min(Math.max(day.done / day.target, 0), 1);
}

/** The square's shade grows with the completion share (R8); a full day is always the darkest. */
export function heatLevel(day: RoutineDay): HeatLevel {
  if (!day.scheduled) {
    return "off";
  }
  const share = completionShare(day);
  if (share >= 1) {
    return 4;
  }
  if (share <= 0) {
    return 0;
  }
  return share < 1 / 3 ? 1 : share < 2 / 3 ? 2 : 3;
}

/** Whether `date` can be marked on `today`: up to 3 days back, never ahead (R7). */
export function canMark(date: string, today: string): boolean {
  return date <= today && date >= addDays(today, -BACKDATE_DAYS);
}

/** Monday of the week `day` falls in. */
export function weekStart(day: string): string {
  const weekday = (new Date(`${day}T00:00:00Z`).getUTCDay() + 6) % 7;
  return addDays(day, -weekday);
}

/** The heatmap's first day: Monday, `weeks - 1` weeks before this week. */
export function heatmapStart(today: string, weeks: number = HEATMAP_WEEKS): string {
  return addDays(weekStart(today), -7 * (weeks - 1));
}

/**
 * Columns of seven days, Monday first, from [`heatmapStart`] to the end of this week. Days after
 * `today`, and days the server did not return, are `null`.
 */
export function heatmapWeeks(
  days: RoutineDay[],
  today: string,
  weeks: number = HEATMAP_WEEKS,
): (RoutineDay | null)[][] {
  const byDate = new Map(days.map((day) => [day.date, day]));
  const start = heatmapStart(today, weeks);
  return Array.from({ length: weeks }, (_, week) =>
    Array.from({ length: 7 }, (_, weekday) => {
      const date = addDays(start, week * 7 + weekday);
      return date > today ? null : (byDate.get(date) ?? null);
    }),
  );
}
