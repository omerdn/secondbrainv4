/** Human-readable plans and days for the routine screens (PRD R1). */
import type { TFunction } from "i18next";

import { parseRules, WEEKDAYS } from "./recurrence";

/** `day` (`YYYY-MM-DD`) in the language, e.g. "Pzt 23 Kasım". */
export function formatDay(day: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    weekday: "short",
    day: "numeric",
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  }).format(new Date(`${day}T00:00:00Z`));
}

/** A month and day in the language, e.g. "8 Mart". */
export function formatMonthDay(month: number, day: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { day: "numeric", month: "long", timeZone: "UTC" }).format(
    new Date(Date.UTC(2000, month - 1, day)),
  );
}

/** The month's name in the language. */
export function monthName(month: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { month: "long", timeZone: "UTC" }).format(
    new Date(Date.UTC(2000, month - 1, 1)),
  );
}

/** One sentence for the plan, e.g. "2 haftada bir: Pzt, Per"; "Özel plan" for mixed rules. */
export function describePlan(rrule: string, t: TFunction, locale: string): string {
  const rules = parseRules(rrule);
  const first = rules?.[0];
  if (rules === null || first === undefined) {
    return t("routines.summary.custom");
  }
  const same = rules.every((r) => r.freq === first.freq && r.interval === first.interval);
  if (!same || (first.freq !== "yearly" && rules.length > 1)) {
    return t("routines.summary.custom");
  }
  const count = first.interval;
  switch (first.freq) {
    case "daily":
      return t("routines.summary.daily", { count });
    case "weekly": {
      const days = first.byDay
        .map((index) => WEEKDAYS[index])
        .filter((code) => code !== undefined)
        .map((code) => t(`routines.weekday.${code}`))
        .join(", ");
      return t("routines.summary.weekly", { count, days });
    }
    case "monthly": {
      const days = first.byMonthDay
        .map((d) => (d < 0 ? t("routines.lastDay") : String(d)))
        .join(", ");
      return t("routines.summary.monthly", { count, days });
    }
    case "yearly": {
      const days = rules
        .flatMap((r) =>
          r.byMonth.flatMap((m) => r.byMonthDay.map((d) => formatMonthDay(m, d, locale))),
        )
        .join(", ");
      return t("routines.summary.yearly", { count, days });
    }
  }
}
