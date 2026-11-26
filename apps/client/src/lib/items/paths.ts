/** Where an item opens, by kind; anything else opens the shared item screen. */
export function itemPath(kind: string, id: string): string {
  const routes: Record<string, string> = {
    task: "tasks",
    routine: "routines",
    goal: "goals",
    tracker: "trackers",
    page: "pages",
  };
  return `/${routes[kind] ?? "items"}/${encodeURIComponent(id)}`;
}
