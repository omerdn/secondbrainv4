import type { ItemPermission, TeamPermission, TeamRole } from "@secondbrain/api-types";

/** The caller's teams; shared with tag colors and the filter bar. */
export const TEAMS_KEY = ["teams"] as const;

/** Colors offered for a new team (`#rrggbb`, lower case as the server stores them). */
export const TEAM_COLORS = ["#4f46e5", "#16a34a", "#0ea5e9", "#f97316", "#db2777", "#a855f7"];

export const ROLES: readonly TeamRole[] = ["admin", "manager", "member", "guest"];

export const ITEM_PERMISSIONS: readonly ItemPermission[] = [
  "view",
  "edit",
  "share",
  "link",
  "manage",
  "delete",
];

export const TEAM_PERMISSIONS: readonly TeamPermission[] = [
  "create_items",
  "assign",
  "invite_members",
  "manage_tags",
  "create_notebooks",
  "view_activity",
];

/** Admins and managers run the team and see its settings (PRD TK5). */
export function canManage(role: TeamRole): boolean {
  return role === "admin" || role === "manager";
}

/**
 * Whether `actor` may remove someone with `role` (TK7, T5.1.6): only admins remove admins and
 * managers. The server also checks the "invite members" right.
 */
export function mayRemove(actor: TeamRole, role: TeamRole): boolean {
  return actor === "admin" || (actor === "manager" && (role === "member" || role === "guest"));
}

/** `values` with `value` added or taken out, in the order of `all`. */
export function toggledIn<T>(all: readonly T[], values: readonly T[], value: T): T[] {
  const on = values.includes(value);
  return all.filter((entry) => (entry === value ? !on : values.includes(entry)));
}
