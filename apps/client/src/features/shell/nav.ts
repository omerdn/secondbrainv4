import type Ionicons from "@expo/vector-icons/Ionicons";
import type { ComponentProps } from "react";

import type { Catalog } from "@/lib/i18n/locales/tr";

export type IconName = ComponentProps<typeof Ionicons>["name"];

/** Section key; its label is `nav.<key>` in the catalogs. */
export type Section = keyof Catalog["nav"];

/** One top-level destination of the app (PRD §6). */
export interface Destination {
  key: Section;
  href: string;
  icon: IconName;
  /** Has its own bottom tab on phones; the others are under "More". */
  tab: boolean;
}

/**
 * Top-level sections in display order; labels come from `nav.<key>` (T0.4.7). Modules (PRD §12)
 * will register more destinations through the module registry (ARCHITECTURE §13).
 */
export const destinations: readonly Destination[] = [
  { key: "today", href: "/", icon: "today-outline", tab: true },
  { key: "tasks", href: "/tasks", icon: "checkbox-outline", tab: true },
  { key: "routines", href: "/routines", icon: "repeat-outline", tab: true },
  { key: "goals", href: "/goals", icon: "flag-outline", tab: false },
  { key: "trackers", href: "/trackers", icon: "stats-chart-outline", tab: false },
  { key: "notebooks", href: "/notebooks", icon: "book-outline", tab: true },
  { key: "session", href: "/session", icon: "timer-outline", tab: false },
  { key: "calendar", href: "/calendar", icon: "calendar-outline", tab: false },
  { key: "tags", href: "/tags", icon: "pricetags-outline", tab: false },
  { key: "views", href: "/views", icon: "funnel-outline", tab: false },
  { key: "archive", href: "/archive", icon: "archive-outline", tab: false },
  { key: "trash", href: "/trash", icon: "trash-outline", tab: false },
];

export const settings: Destination = {
  key: "settings",
  href: "/settings",
  icon: "settings-outline",
  tab: false,
};

export const more: Destination = {
  key: "more",
  href: "/more",
  icon: "ellipsis-horizontal",
  tab: true,
};

/** Bottom tabs on phones: the primary destinations plus "More". */
export const tabs: readonly Destination[] = [...destinations.filter((d) => d.tab), more];

/** What "More" lists: every destination without its own tab, then settings. */
export const moreItems: readonly Destination[] = [...destinations.filter((d) => !d.tab), settings];

/** Whether `pathname` is inside `href` (`/` only matches itself). */
export function isActive(pathname: string, href: string): boolean {
  if (href === "/") return pathname === "/";
  return pathname === href || pathname.startsWith(`${href}/`);
}

/** The bottom tab to highlight: a destination under "More" highlights "More". */
export function activeTab(pathname: string): Section | null {
  const direct = tabs.find((tab) => tab.key !== "more" && isActive(pathname, tab.href));
  if (direct !== undefined) return direct.key;
  return [more, ...moreItems].some((item) => isActive(pathname, item.href)) ? "more" : null;
}
