/** Pure helpers of the Today screen (PRD H1-H6). */

/** Where an item opens, by kind; anything else opens the shared item screen. */
export function itemPath(kind: string, id: string): string {
  const routes: Record<string, string> = {
    task: "tasks",
    routine: "routines",
    goal: "goals",
    tracker: "trackers",
    page: "pages",
  };
  return `/${routes[kind] ?? "items"}/${encodeURIComponent(id)}`;
}

/** A 0..1 ratio as a whole percent. */
export function percent(ratio: number): number {
  return Number.isFinite(ratio) ? Math.round(ratio * 100) : 0;
}

/** Days from `today` to `day` (negative in the past); both `YYYY-MM-DD`. */
export function daysFrom(today: string, day: string): number {
  const ms = Date.parse(`${day}T00:00:00Z`) - Date.parse(`${today}T00:00:00Z`);
  return Math.round(ms / 86_400_000);
}

/** Whether a forgotten threshold typed in a field is valid: whole days, 1 to 365. */
export function parseThreshold(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d{1,3}$/.test(trimmed)) return null;
  const days = Number(trimmed);
  return days >= 1 && days <= 365 ? days : null;
}
