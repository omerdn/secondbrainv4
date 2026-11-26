/** Calendar-day helpers shared by features. Days are `YYYY-MM-DD` strings, free of time zones. */

/** Whether `raw` is a real calendar day as `YYYY-MM-DD`. */
export function isDay(raw: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(raw)) {
    return false;
  }
  const parsed = new Date(`${raw}T00:00:00Z`);
  return !Number.isNaN(parsed.getTime()) && parsed.toISOString().slice(0, 10) === raw;
}

/** `day` moved by `delta` calendar days. Works in UTC, so no time zone can shift the date. */
export function addDays(day: string, delta: number): string {
  const parsed = new Date(`${day}T00:00:00Z`);
  parsed.setUTCDate(parsed.getUTCDate() + delta);
  return parsed.toISOString().slice(0, 10);
}

/** The device's IANA time zone, or `UTC` when the platform does not say. */
export function deviceTimeZone(): string {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    return typeof zone === "string" && zone !== "" ? zone : "UTC";
  } catch {
    return "UTC";
  }
}

/** The calendar day of `now` in `timeZone` as `YYYY-MM-DD`; UTC for an unknown zone. */
export function todayIn(timeZone: string, now: Date = new Date()): string {
  const format = (zone: string) => {
    const parts = new Intl.DateTimeFormat("en-CA", {
      timeZone: zone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).formatToParts(now);
    const part = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
    return `${part("year")}-${part("month")}-${part("day")}`;
  };
  try {
    return format(timeZone);
  } catch {
    return format("UTC");
  }
}
