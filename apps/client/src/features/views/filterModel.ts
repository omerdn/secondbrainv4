import type { ItemFilter, ItemListState } from "@secondbrain/api-types";

/** Kinds the filter bar offers; module kinds arrive with the module registry. */
export const FILTER_KINDS = ["task", "routine", "goal", "tracker", "notebook", "page"] as const;

export const FILTER_STATES: readonly ItemListState[] = ["active", "archived", "trash"];

/** `list` with `value` added, or removed if it was there. */
export function toggled(list: readonly string[] | undefined, value: string): string[] {
  const current = list ?? [];
  return current.includes(value) ? current.filter((entry) => entry !== value) : [...current, value];
}

const sorted = (list: readonly string[] | undefined) =>
  list !== undefined && list.length > 0 ? [...list].sort() : undefined;

/**
 * The canonical form of a filter: no empty dimensions, sorted values, and no `state` when it is
 * the default. Equal filters serialize equally, so they share one query cache entry.
 */
export function cleanFilter(filter: ItemFilter): ItemFilter {
  const clean: ItemFilter = {};
  const tagIds = sorted(filter.tag_ids);
  const kinds = sorted(filter.kinds);
  const people = sorted(filter.people);
  const teamIds = sorted(filter.team_ids);
  const statuses = sorted(filter.statuses);
  if (tagIds !== undefined) clean.tag_ids = tagIds;
  if (kinds !== undefined) clean.kinds = kinds;
  if (people !== undefined) clean.people = people;
  if (teamIds !== undefined) clean.team_ids = teamIds;
  if (filter.date !== undefined) clean.date = filter.date;
  if (statuses !== undefined) clean.statuses = statuses;
  if (filter.state !== undefined && filter.state !== "active") clean.state = filter.state;
  return clean;
}

export function sameFilter(a: ItemFilter, b: ItemFilter): boolean {
  return JSON.stringify(cleanFilter(a)) === JSON.stringify(cleanFilter(b));
}

export function isEmptyFilter(filter: ItemFilter): boolean {
  return Object.keys(cleanFilter(filter)).length === 0;
}
