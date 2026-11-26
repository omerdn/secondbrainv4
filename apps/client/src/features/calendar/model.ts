import type { Calendar, Item } from "@secondbrain/api-types";

import { addDays } from "@/lib/dates";

/**
 * Calendar arithmetic (PRD C1, T2.7.4). Days are `YYYY-MM-DD` strings; weeks start on Monday.
 * Everything here is pure, so the views only lay out what these return.
 */

export type CalendarView = "day" | "week" | "month";
export const VIEWS: readonly CalendarView[] = ["day", "week", "month"];

const parse = (day: string) => new Date(`${day}T00:00:00Z`);
const format = (date: Date) => date.toISOString().slice(0, 10);

/** 0 for Monday … 6 for Sunday. */
export function weekday(day: string): number {
  return (parse(day).getUTCDay() + 6) % 7;
}

/** The Monday of `day`'s week. */
export function startOfWeek(day: string): string {
  return addDays(day, -weekday(day));
}

/** The first day of `day`'s month. */
export function startOfMonth(day: string): string {
  return `${day.slice(0, 7)}-01`;
}

/** The last day of `day`'s month. */
export function endOfMonth(day: string): string {
  const date = parse(startOfMonth(day));
  date.setUTCMonth(date.getUTCMonth() + 1);
  date.setUTCDate(0);
  return format(date);
}

/** The days a view shows around `anchor`, both ends included; a month is whole weeks. */
export function rangeFor(view: CalendarView, anchor: string): { from: string; to: string } {
  switch (view) {
    case "day":
      return { from: anchor, to: anchor };
    case "week": {
      const from = startOfWeek(anchor);
      return { from, to: addDays(from, 6) };
    }
    case "month": {
      const from = startOfWeek(startOfMonth(anchor));
      const last = endOfMonth(anchor);
      return { from, to: addDays(last, 6 - weekday(last)) };
    }
  }
}

/** The anchor one step back (`-1`) or forward (`1`); a month step lands on the 1st. */
export function shift(view: CalendarView, anchor: string, step: -1 | 1): string {
  switch (view) {
    case "day":
      return addDays(anchor, step);
    case "week":
      return addDays(anchor, 7 * step);
    case "month": {
      const date = parse(startOfMonth(anchor));
      date.setUTCMonth(date.getUTCMonth() + step);
      return format(date);
    }
  }
}

/** Every day of `from..=to`. */
export function daysOf(from: string, to: string): string[] {
  const days: string[] = [];
  for (let day = from; day <= to; day = addDays(day, 1)) days.push(day);
  return days;
}

/** The rows of a month grid: weeks of seven days. */
export function weeksOf(from: string, to: string): string[][] {
  const days = daysOf(from, to);
  const weeks: string[][] = [];
  for (let i = 0; i < days.length; i += 7) weeks.push(days.slice(i, i + 7));
  return weeks;
}

/** What a day holds. */
export type EntryKind = "routine" | "taskStart" | "taskDue" | "taskOn" | "goalStart" | "goalEnd";

export interface Entry {
  key: string;
  kind: EntryKind;
  item: Item;
}

const ORDER: Record<EntryKind, number> = {
  taskDue: 0,
  taskOn: 1,
  taskStart: 2,
  routine: 3,
  goalStart: 4,
  goalEnd: 5,
};

/**
 * The entries of each day of `from..=to`: a routine on each occurrence, a task on its start and
 * due days (one entry when they are the same or one is missing), a goal on its first and last
 * day. Goals running through a day are listed apart ({@link goalsOn}).
 */
export function entriesByDay(calendar: Calendar, from: string, to: string): Map<string, Entry[]> {
  const days = new Map<string, Entry[]>(daysOf(from, to).map((day) => [day, []]));
  const add = (day: string | null, kind: EntryKind, item: Item) => {
    if (day === null) return;
    days.get(day)?.push({ key: `${kind}:${item.id}:${day}`, kind, item });
  };
  for (const routine of calendar.routines) {
    for (const date of routine.dates) add(date, "routine", routine.item);
  }
  for (const task of calendar.tasks) {
    const { start_date: start, due_date: due } = task;
    if (start !== null && due !== null && start !== due) {
      add(start, "taskStart", task.item);
      add(due, "taskDue", task.item);
    } else {
      add(due ?? start, due !== null ? "taskDue" : "taskOn", task.item);
    }
  }
  for (const goal of calendar.goals) {
    add(goal.start_date, "goalStart", goal.item);
    if (goal.end_date !== goal.start_date) add(goal.end_date, "goalEnd", goal.item);
  }
  for (const entries of days.values()) {
    entries.sort(
      (a, b) =>
        ORDER[a.kind] - ORDER[b.kind] ||
        a.item.title.localeCompare(b.item.title) ||
        a.item.id.localeCompare(b.item.id),
    );
  }
  return days;
}

/** The goals whose range holds `day`. */
export function goalsOn(calendar: Calendar, day: string): Item[] {
  return calendar.goals
    .filter((goal) => goal.start_date <= day && day <= goal.end_date)
    .map((goal) => goal.item);
}
