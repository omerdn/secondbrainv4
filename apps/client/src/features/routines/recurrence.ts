/**
 * The routine plan picker's model (PRD R1-R3, R13, T2.2.5): the form, its RRULE text and a
 * preview of the next occurrences. Matching mirrors the server's `recurrence` crate, so the
 * preview shows the same days the server will schedule. Days are `YYYY-MM-DD` strings.
 */
import type { CreateRoutineRequest } from "@secondbrain/api-types";

import { addDays, isDay } from "@/lib/dates";

export const FREQUENCIES = ["daily", "weekly", "monthly", "yearly"] as const;
export type Frequency = (typeof FREQUENCIES)[number];

/** Monday first, as the server counts weeks. */
export const WEEKDAYS = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"] as const;
export type Weekday = (typeof WEEKDAYS)[number];

/** "The last day of the month" (`BYMONTHDAY=-1`). */
export const LAST_DAY = -1;
export const INTERVAL_MAX = 99;
export const DAILY_TARGET_MAX = 100;
export const REMINDER_LIMIT = 24;
export const PREVIEW_COUNT = 5;
/** How far ahead the preview looks for occurrences. */
const PREVIEW_HORIZON_DAYS = 800;

export interface YearlyDate {
  month: number;
  day: number;
}

export interface RoutineForm {
  title: string;
  frequency: Frequency;
  /** Every N days, weeks, months or years. */
  interval: string;
  weekdays: Weekday[];
  /** 1-31, or [`LAST_DAY`]. */
  monthDays: number[];
  yearlyDates: YearlyDate[];
  dailyTarget: string;
  startDate: string;
  endDate: string;
  /** `HH:MM`, sorted. */
  reminders: string[];
  timezone: string;
}

export function emptyForm(today: string, timezone: string): RoutineForm {
  return {
    title: "",
    frequency: "daily",
    interval: "1",
    weekdays: [],
    monthDays: [],
    yearlyDates: [],
    dailyTarget: "1",
    startDate: today,
    endDate: "",
    reminders: [],
    timezone,
  };
}

export type FormField =
  | "title"
  | "interval"
  | "weekdays"
  | "monthDays"
  | "yearlyDates"
  | "dailyTarget"
  | "startDate"
  | "endDate"
  | "reminders";
export type FormError =
  | "required"
  | "wholeNumber"
  | "intervalRange"
  | "targetRange"
  | "pickWeekday"
  | "pickMonthDay"
  | "pickDate"
  | "invalidDay"
  | "endBeforeStart"
  | "tooManyReminders";
export type FormErrors = Partial<Record<FormField, FormError>>;

const DAYS_IN_MONTH = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/** Whether `month`/`day` can happen in some year (29 February included). */
export function isYearlyDate({ month, day }: YearlyDate): boolean {
  return (
    Number.isInteger(month) &&
    Number.isInteger(day) &&
    month >= 1 &&
    month <= 12 &&
    day >= 1 &&
    day <= (DAYS_IN_MONTH[month - 1] ?? 0)
  );
}

/** `HH:MM` on a 24-hour clock, or `null`. Accepts `7:5` and `07.05`. */
export function normalizeTime(raw: string): string | null {
  const match = /^(\d{1,2})[:.](\d{1,2})$/.exec(raw.trim());
  if (match === null) {
    return null;
  }
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  if (hours > 23 || minutes > 59) {
    return null;
  }
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
}

/** The reminders with `time` added once, in order; unchanged at the limit (R13). */
export function addReminder(reminders: string[], time: string): string[] {
  if (reminders.includes(time) || reminders.length >= REMINDER_LIMIT) {
    return reminders;
  }
  return [...reminders, time].sort();
}

/** `values` with `value` added or removed. */
export function toggle<T>(values: T[], value: T): T[] {
  return values.includes(value) ? values.filter((v) => v !== value) : [...values, value];
}

function wholeNumber(raw: string): number | null {
  const trimmed = raw.trim();
  return /^\d+$/.test(trimmed) ? Number(trimmed) : null;
}

const byWeekday = (a: Weekday, b: Weekday) => WEEKDAYS.indexOf(a) - WEEKDAYS.indexOf(b);
/** Positive days in order, then the last day, as the server writes them. */
const byMonthDay = (a: number, b: number) => a - b;

/**
 * The RRULE text for the plan, one rule per line (R1): yearly dates become one rule each.
 * Empty when the plan is not complete yet.
 */
export function buildRrule(form: RoutineForm, interval: number): string {
  const every = interval === 1 ? "" : `;INTERVAL=${String(interval)}`;
  switch (form.frequency) {
    case "daily":
      return `FREQ=DAILY${every}`;
    case "weekly":
      return form.weekdays.length === 0
        ? ""
        : `FREQ=WEEKLY${every};BYDAY=${[...form.weekdays].sort(byWeekday).join(",")}`;
    case "monthly":
      return form.monthDays.length === 0
        ? ""
        : `FREQ=MONTHLY${every};BYMONTHDAY=${[...form.monthDays].sort(byMonthDay).join(",")}`;
    case "yearly":
      return [...form.yearlyDates]
        .sort((a, b) => a.month - b.month || a.day - b.day)
        .map((d) => `FREQ=YEARLY${every};BYMONTH=${String(d.month)};BYMONTHDAY=${String(d.day)}`)
        .join("\n");
  }
}

export type FormResult =
  | { errors: FormErrors; request?: undefined }
  | { errors?: undefined; request: CreateRoutineRequest };

/** The create request, or what is wrong with each field. */
export function validateForm(form: RoutineForm): FormResult {
  const errors: FormErrors = {};
  const title = form.title.trim();
  if (title === "") {
    errors.title = "required";
  }
  const interval = wholeNumber(form.interval);
  if (interval === null) {
    errors.interval = "wholeNumber";
  } else if (interval < 1 || interval > INTERVAL_MAX) {
    errors.interval = "intervalRange";
  }
  const target = wholeNumber(form.dailyTarget);
  if (target === null) {
    errors.dailyTarget = "wholeNumber";
  } else if (target < 1 || target > DAILY_TARGET_MAX) {
    errors.dailyTarget = "targetRange";
  }
  if (form.frequency === "weekly" && form.weekdays.length === 0) {
    errors.weekdays = "pickWeekday";
  }
  if (form.frequency === "monthly" && form.monthDays.length === 0) {
    errors.monthDays = "pickMonthDay";
  }
  if (form.frequency === "yearly" && form.yearlyDates.length === 0) {
    errors.yearlyDates = "pickDate";
  }
  const start = form.startDate.trim();
  const end = form.endDate.trim();
  if (start === "") {
    errors.startDate = "required";
  } else if (!isDay(start)) {
    errors.startDate = "invalidDay";
  }
  if (end !== "" && !isDay(end)) {
    errors.endDate = "invalidDay";
  } else if (end !== "" && isDay(start) && end < start) {
    errors.endDate = "endBeforeStart";
  }
  if (form.reminders.length > REMINDER_LIMIT) {
    errors.reminders = "tooManyReminders";
  }
  if (Object.keys(errors).length > 0 || interval === null || target === null) {
    return { errors };
  }
  const request: CreateRoutineRequest = {
    title,
    timezone: form.timezone,
    rrule: buildRrule(form, interval),
    daily_target: target,
    start_date: start,
  };
  if (end !== "") {
    request.end_date = end;
  }
  if (form.reminders.length > 0) {
    request.reminder_times = form.reminders;
  }
  return { request };
}

// --- Matching, as in the server's `recurrence::Rule::matches` ---------------------------------

export interface Rule {
  freq: Frequency;
  interval: number;
  byDay: number[];
  byMonthDay: number[];
  byMonth: number[];
}

const FREQ_CODES: Record<string, Frequency> = {
  DAILY: "daily",
  WEEKLY: "weekly",
  MONTHLY: "monthly",
  YEARLY: "yearly",
};

function parseRule(line: string): Rule | null {
  const rule: Rule = { freq: "daily", interval: 1, byDay: [], byMonthDay: [], byMonth: [] };
  let freq: Frequency | undefined;
  for (const part of line.split(";")) {
    const [key, value = ""] = part.split("=");
    const list = value.split(",").filter((v) => v !== "");
    switch (key) {
      case "FREQ":
        freq = FREQ_CODES[value];
        break;
      case "INTERVAL":
        rule.interval = Number(value);
        break;
      case "BYDAY":
        rule.byDay = list.map((code) => WEEKDAYS.indexOf(code as Weekday));
        break;
      case "BYMONTHDAY":
        rule.byMonthDay = list.map(Number);
        break;
      case "BYMONTH":
        rule.byMonth = list.map(Number);
        break;
      default:
        return null;
    }
  }
  if (freq === undefined || !Number.isInteger(rule.interval) || rule.interval < 1) {
    return null;
  }
  return { ...rule, freq };
}

/** Parses one RRULE per non-blank line; `null` if any line is not one the picker writes. */
export function parseRules(rrule: string): Rule[] | null {
  const rules: Rule[] = [];
  for (const line of rrule.split("\n")) {
    if (line.trim() === "") {
      continue;
    }
    const rule = parseRule(line.trim());
    if (rule === null) {
      return null;
    }
    rules.push(rule);
  }
  return rules.length === 0 ? null : rules;
}

interface Parts {
  year: number;
  month: number;
  day: number;
  /** Monday = 0. */
  weekday: number;
  /** Days since 1970-01-01. */
  serial: number;
}

function parts(day: string): Parts {
  const date = new Date(`${day}T00:00:00Z`);
  return {
    year: date.getUTCFullYear(),
    month: date.getUTCMonth() + 1,
    day: date.getUTCDate(),
    weekday: (date.getUTCDay() + 6) % 7,
    serial: Math.round(date.getTime() / 86_400_000),
  };
}

function daysInMonth(year: number, month: number): number {
  return new Date(Date.UTC(year, month, 0)).getUTCDate();
}

function onMonthDay(rule: Rule, start: Parts, date: Parts): boolean {
  if (rule.byMonthDay.length === 0) {
    return date.day === start.day;
  }
  const length = daysInMonth(date.year, date.month);
  return rule.byMonthDay.some((d) => (d > 0 ? date.day === d : date.day === length + 1 + d));
}

function matches(rule: Rule, start: Parts, date: Parts): boolean {
  if (date.serial < start.serial) {
    return false;
  }
  switch (rule.freq) {
    case "daily":
      return (date.serial - start.serial) % rule.interval === 0;
    case "weekly": {
      const weeks = (date.serial - date.weekday - (start.serial - start.weekday)) / 7;
      const onDay =
        rule.byDay.length === 0
          ? date.weekday === start.weekday
          : rule.byDay.includes(date.weekday);
      return weeks % rule.interval === 0 && onDay;
    }
    case "monthly": {
      const months = date.year * 12 + date.month - (start.year * 12 + start.month);
      return months % rule.interval === 0 && onMonthDay(rule, start, date);
    }
    case "yearly": {
      const inMonth =
        rule.byMonth.length === 0 ? date.month === start.month : rule.byMonth.includes(date.month);
      return (
        (date.year - start.year) % rule.interval === 0 && inMonth && onMonthDay(rule, start, date)
      );
    }
  }
}

/** Whether the plan starting on `start` has an occurrence on `day`. */
export function occursOn(rules: Rule[], start: string, day: string): boolean {
  const from = parts(start);
  const date = parts(day);
  return rules.some((rule) => matches(rule, from, date));
}

/**
 * The first `count` occurrences on or after `from`, up to `end` when given; shorter when the plan
 * runs out or nothing falls within the preview horizon.
 */
export function nextOccurrences(
  rrule: string,
  start: string,
  from: string,
  count: number,
  end?: string,
): string[] {
  const rules = parseRules(rrule);
  if (rules === null || !isDay(start) || !isDay(from)) {
    return [];
  }
  const result: string[] = [];
  let day = from < start ? start : from;
  for (let i = 0; i < PREVIEW_HORIZON_DAYS && result.length < count; i += 1) {
    if (end !== undefined && day > end) {
      break;
    }
    if (occursOn(rules, start, day)) {
      result.push(day);
    }
    day = addDays(day, 1);
  }
  return result;
}
