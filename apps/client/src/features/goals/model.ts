/** Goal forms and links (PRD D1-D3, T2.3.3). Pure; days are `YYYY-MM-DD` strings. */
import type { CreateGoalRequest, GoalLink, Item } from "@secondbrain/api-types";

import { isDay } from "@/lib/dates";

/** Kinds that can be linked to a goal; pages are shown but never count (D2). */
export const LINKABLE_KINDS = ["task", "routine", "tracker", "page"] as const;
export const CONTRIBUTING_KINDS: readonly string[] = ["task", "routine", "tracker"];
/** Highest weight of one link (D3). */
export const WEIGHT_MAX = 100;

export interface GoalForm {
  title: string;
  description: string;
  startDate: string;
  endDate: string;
}

export const emptyGoalForm: GoalForm = { title: "", description: "", startDate: "", endDate: "" };

export type GoalField = "title" | "startDate" | "endDate";
export type GoalError = "required" | "invalidDay" | "endBeforeStart";
export type GoalErrors = Partial<Record<GoalField, GoalError>>;

/** What is wrong with the form; both dates are required (D1). */
export function goalErrors(form: GoalForm): GoalErrors {
  const errors: GoalErrors = {};
  const start = form.startDate.trim();
  const end = form.endDate.trim();
  if (form.title.trim() === "") {
    errors.title = "required";
  }
  if (start === "") {
    errors.startDate = "required";
  } else if (!isDay(start)) {
    errors.startDate = "invalidDay";
  }
  if (end === "") {
    errors.endDate = "required";
  } else if (!isDay(end)) {
    errors.endDate = "invalidDay";
  } else if (isDay(start) && end < start) {
    errors.endDate = "endBeforeStart";
  }
  return errors;
}

/** The create request, or `null` while the form has errors. */
export function goalRequest(form: GoalForm): CreateGoalRequest | null {
  if (Object.keys(goalErrors(form)).length > 0) {
    return null;
  }
  const description = form.description.trim();
  return {
    title: form.title.trim(),
    start_date: form.startDate.trim(),
    end_date: form.endDate.trim(),
    ...(description === "" ? {} : { description }),
  };
}

/**
 * A weight as the server reads it: more than 0, at most 100, two decimals at most; a comma
 * works as the decimal mark. `null` otherwise (D3).
 */
export function normalizeWeight(raw: string): string | null {
  const text = raw.trim().replace(",", ".");
  if (!/^\d{1,3}(\.\d{1,2})?$/.test(text)) {
    return null;
  }
  const value = Number(text);
  if (value <= 0 || value > WEIGHT_MAX) {
    return null;
  }
  return String(value);
}

/** Items that could still be linked: linkable kinds, not in the trash or linked, matching `query`. */
export function linkCandidates(
  items: readonly Item[],
  links: readonly GoalLink[],
  query: string,
): Item[] {
  const linked = new Set(links.map((link) => link.item.id));
  const needle = query.trim().toLocaleLowerCase();
  return items.filter(
    (item) =>
      (LINKABLE_KINDS as readonly string[]).includes(item.kind) &&
      item.deleted_at === null &&
      !linked.has(item.id) &&
      (needle === "" || item.title.toLocaleLowerCase().includes(needle)),
  );
}

/** The share of each counting link's weight, for the "what weighs how much" hint (D3). */
export function weightShares(links: readonly GoalLink[]): Map<string, number> {
  const counting = links.filter((link) => link.contributes);
  const total = counting.reduce((sum, link) => sum + Number(link.weight), 0);
  return new Map(
    counting.map((link) => [link.item.id, total > 0 ? Number(link.weight) / total : 0]),
  );
}

/** `day` (`YYYY-MM-DD`) in the language, e.g. "1 Aralık 2026". */
export function formatDate(day: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  }).format(new Date(`${day}T00:00:00Z`));
}
