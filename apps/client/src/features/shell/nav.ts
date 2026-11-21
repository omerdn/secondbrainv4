import type Ionicons from "@expo/vector-icons/Ionicons";
import type { ComponentProps } from "react";

export type IconName = ComponentProps<typeof Ionicons>["name"];

/** One top-level destination of the app (PRD §6). */
export interface Destination {
  key: string;
  label: string;
  href: string;
  icon: IconName;
  /** Has its own bottom tab on phones; the others are under "More". */
  tab: boolean;
}

/**
 * Top-level sections in display order. Labels are Turkish until i18n (T0.4.7). Modules (PRD §12)
 * will register more destinations through the module registry (ARCHITECTURE §13).
 */
export const destinations: readonly Destination[] = [
  { key: "today", label: "Bugün", href: "/", icon: "today-outline", tab: true },
  { key: "tasks", label: "Görevler", href: "/tasks", icon: "checkbox-outline", tab: true },
  { key: "routines", label: "Rutinler", href: "/routines", icon: "repeat-outline", tab: true },
  { key: "goals", label: "Hedefler", href: "/goals", icon: "flag-outline", tab: false },
  {
    key: "trackers",
    label: "Takipler",
    href: "/trackers",
    icon: "stats-chart-outline",
    tab: false,
  },
  { key: "notebooks", label: "Defterler", href: "/notebooks", icon: "book-outline", tab: true },
  { key: "calendar", label: "Takvim", href: "/calendar", icon: "calendar-outline", tab: false },
];

export const settings: Destination = {
  key: "settings",
  label: "Ayarlar",
  href: "/settings",
  icon: "settings-outline",
  tab: false,
};

export const more: Destination = {
  key: "more",
  label: "Daha fazla",
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
export function activeTab(pathname: string): string | null {
  const direct = tabs.find((tab) => tab.key !== "more" && isActive(pathname, tab.href));
  if (direct !== undefined) return direct.key;
  return [more, ...moreItems].some((item) => isActive(pathname, item.href)) ? "more" : null;
}
