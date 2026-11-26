import { useApi } from "@secondbrain/api-client/react";
import type { Item } from "@secondbrain/api-types";
import { useQueries } from "@tanstack/react-query";

/** The items `ids` name; an item the caller cannot see is left out, so it stays unnamed. */
export function useItemTitles(ids: readonly string[]): Map<string, Item> {
  const api = useApi();
  const results = useQueries({
    queries: ids.map((id) => ({
      queryKey: ["item", id],
      queryFn: () => api.items.get(id),
      retry: false,
    })),
  });
  const items = new Map<string, Item>();
  results.forEach((result, index) => {
    const id = ids[index];
    if (id !== undefined && result.data !== undefined) items.set(id, result.data);
  });
  return items;
}
