import type { ItemMember, RoutinePeopleDays } from "@secondbrain/api-types";

import { addDays } from "@/lib/dates";

/** Days the "who did it" list shows, today included. */
export const PEOPLE_DAYS = 7;

/** One person's line on one day. */
export interface PersonLine {
  userId: string;
  done: number;
  /** Whether they are one of the routine's responsible people. */
  responsible: boolean;
}

/** The days of the list, newest first. */
export function recentDays(today: string, count = PEOPLE_DAYS): string[] {
  return Array.from({ length: count }, (_, back) => addDays(today, -back));
}

/**
 * Who did what on `date` (R5): every responsible person, even with nothing done, then anyone else
 * who added something, each in member order.
 */
export function linesOn(
  data: RoutinePeopleDays,
  date: string,
  order: readonly string[],
): PersonLine[] {
  const done = new Map<string, number>();
  for (const entry of data.days) {
    if (entry.date === date) done.set(entry.user_id, entry.done);
  }
  const rank = (id: string) => {
    const at = order.indexOf(id);
    return at === -1 ? order.length : at;
  };
  const byRank = (a: string, b: string) => rank(a) - rank(b) || a.localeCompare(b);
  const responsible = [...data.responsible].sort(byRank);
  const others = [...done.keys()].filter((id) => !data.responsible.includes(id)).sort(byRank);
  return [
    ...responsible.map((userId) => ({ userId, done: done.get(userId) ?? 0, responsible: true })),
    ...others.map((userId) => ({ userId, done: done.get(userId) ?? 0, responsible: false })),
  ];
}

/** Names by user id; someone who left keeps an id only. */
export function namesOf(members: readonly ItemMember[]): Map<string, string> {
  return new Map(members.map((member) => [member.user_id, member.display_name]));
}

/** `order` with the person at `index` moved one place earlier. */
export function movedUp(order: readonly string[], index: number): string[] {
  const next = [...order];
  if (index <= 0 || index >= next.length) return next;
  const [person] = next.splice(index, 1);
  if (person !== undefined) next.splice(index - 1, 0, person);
  return next;
}
