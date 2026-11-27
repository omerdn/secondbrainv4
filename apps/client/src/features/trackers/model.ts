import type {
  CreateTrackerRequest,
  TrackerAggregation,
  TrackerDirection,
  TrackerEntry,
  TrackerPeriod,
} from "@secondbrain/api-types";

import { addDays, isDay } from "@/lib/dates";

export { addDays, isDay };

export const PERIODS = ["day", "week", "month", "total", "range"] as const;
export const AGGREGATIONS = ["sum", "count", "last", "max"] as const;
export const DIRECTIONS = ["increase", "limit"] as const;

export type PeriodType = (typeof PERIODS)[number];

/** Days the history chart covers (PRD K3). */
export const HISTORY_DAYS = 14;

const DECIMAL = /^-?\d{1,16}(\.\d{1,4})?$/;

/** What the user typed as a decimal string the API accepts; a comma also works (`12,5`). */
export function normalizeDecimal(raw: string): string | null {
  const value = raw.trim().replace(",", ".");
  return DECIMAL.test(value) ? value : null;
}

export interface TrackerForm {
  title: string;
  unit: string;
  target: string;
  period: PeriodType;
  rangeStart: string;
  rangeEnd: string;
  aggregation: TrackerAggregation;
  direction: TrackerDirection;
}

export type FormField = "title" | "unit" | "target" | "rangeStart" | "rangeEnd";
export type FormError = "required" | "invalidNumber" | "positive" | "invalidDay" | "endBeforeStart";
export type FormErrors = Partial<Record<FormField, FormError>>;

export const emptyForm: TrackerForm = {
  title: "",
  unit: "",
  target: "",
  period: "month",
  rangeStart: "",
  rangeEnd: "",
  aggregation: "sum",
  direction: "increase",
};

function periodOf(form: TrackerForm): TrackerPeriod {
  return form.period === "range"
    ? { type: "range", start: form.rangeStart, end: form.rangeEnd }
    : { type: form.period };
}

/** Checks the form (PRD T1) and builds the request, or says what is wrong with each field. */
export function validateForm(
  form: TrackerForm,
): { request: CreateTrackerRequest; errors?: undefined } | { errors: FormErrors } {
  const errors: FormErrors = {};
  if (form.title.trim() === "") {
    errors.title = "required";
  }
  if (form.unit.trim() === "") {
    errors.unit = "required";
  }
  const target = normalizeDecimal(form.target);
  if (form.target.trim() === "") {
    errors.target = "required";
  } else if (target === null) {
    errors.target = "invalidNumber";
  } else if (Number(target) <= 0) {
    errors.target = "positive";
  }
  if (form.period === "range") {
    if (!isDay(form.rangeStart)) {
      errors.rangeStart = "invalidDay";
    }
    if (!isDay(form.rangeEnd)) {
      errors.rangeEnd = "invalidDay";
    } else if (isDay(form.rangeStart) && form.rangeEnd < form.rangeStart) {
      errors.rangeEnd = "endBeforeStart";
    }
  }
  if (Object.keys(errors).length > 0 || target === null) {
    return { errors };
  }
  return {
    request: {
      title: form.title.trim(),
      unit: form.unit.trim(),
      target_value: target,
      period: periodOf(form),
      aggregation: form.aggregation,
      direction: form.direction,
    },
  };
}

export interface DayValue {
  day: string;
  /** `null` on a day without entries. */
  value: number | null;
}

function combine(aggregation: TrackerAggregation, entries: readonly TrackerEntry[]): number {
  const values = entries.map((entry) => Number(entry.value));
  switch (aggregation) {
    case "sum":
      return Math.round(values.reduce((sum, value) => sum + value, 0) * 10_000) / 10_000;
    case "count":
      return values.length;
    case "max":
      return Math.max(...values);
    case "last": {
      const latest = entries.reduce((a, b) => (b.occurred_at > a.occurred_at ? b : a));
      return Number(latest.value);
    }
  }
}

/** One value per day for the history chart, oldest first, ending on `lastDay` (PRD K3). */
export function dailySeries(
  entries: readonly TrackerEntry[],
  aggregation: TrackerAggregation,
  lastDay: string,
  days: number = HISTORY_DAYS,
): DayValue[] {
  const byDay = new Map<string, TrackerEntry[]>();
  for (const entry of entries) {
    byDay.set(entry.local_date, [...(byDay.get(entry.local_date) ?? []), entry]);
  }
  return Array.from({ length: days }, (_, index) => {
    const day = addDays(lastDay, index - days + 1);
    const onDay = byDay.get(day);
    return { day, value: onDay === undefined ? null : combine(aggregation, onDay) };
  });
}

/** Bar height as a share of the tallest bar, 0 to 1. Negative values have no height. */
export function barShare(value: number | null, series: readonly DayValue[]): number {
  const tallest = Math.max(0, ...series.map((point) => point.value ?? 0));
  if (value === null || value <= 0 || tallest === 0) {
    return 0;
  }
  return value / tallest;
}

/** How full the progress bar is, 0 to 1. */
export function progressShare(value: string | null, target: string): number {
  const share = Number(value ?? 0) / Number(target);
  return Number.isFinite(share) ? Math.min(1, Math.max(0, share)) : 0;
}

/** A decimal string for display, with the language's decimal mark. Never rounds. */
export function formatAmount(value: string | number, language: string): string {
  const text = String(value);
  return language.startsWith("tr") ? text.replace(".", ",") : text;
}

/** People of a shared tracker in the side-by-side view (AK5): the reader first, then by value. */
export function sideBySide<T extends { user_id: string; value: string }>(
  people: readonly T[],
  me: string | null,
): T[] {
  return [...people].sort(
    (a, b) =>
      Number(b.user_id === me) - Number(a.user_id === me) ||
      Number(b.value) - Number(a.value) ||
      a.user_id.localeCompare(b.user_id),
  );
}
