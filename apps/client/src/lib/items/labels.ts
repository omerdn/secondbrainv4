import type { AccessReason } from "@secondbrain/api-types";

const BUILT_IN_KINDS = ["task", "routine", "goal", "tracker", "notebook", "page"] as const;

/** Catalog key under `items.kinds`. */
export type KindLabel = (typeof BUILT_IN_KINDS)[number] | "module";

/** Label key of an item kind; every `module:<name>` shares one label. */
export function kindLabel(kind: string): KindLabel {
  return BUILT_IN_KINDS.find((builtIn) => builtIn === kind) ?? "module";
}

/** Catalog key under `items.roles`. */
export type RoleLabel =
  "owner" | "assignee" | "watcher" | "editor" | "viewer" | "team" | "notebook" | "linked";

const roleOf = (reason: AccessReason): RoleLabel =>
  reason.kind === "team_role" ? "team" : reason.kind;

/** The caller's roles on an item, without repeats, in the server's order (direct ones first). */
export function roleLabels(reasons: readonly AccessReason[]): RoleLabel[] {
  return [...new Set(reasons.map(roleOf))];
}
