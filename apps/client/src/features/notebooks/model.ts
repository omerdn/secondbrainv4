/** Notebook and page names (PRD Y1); the server's item title rules. */
export const TITLE_MAX_CHARS = 500;

export type TitleError = "required" | "tooLong";

/** Why `title` cannot be saved, or null when it can. */
export function titleError(title: string): TitleError | null {
  const trimmed = title.trim();
  if (trimmed.length === 0) return "required";
  if ([...trimmed].length > TITLE_MAX_CHARS) return "tooLong";
  return null;
}

/** Item kinds a page can be linked to (PRD B1). */
export const PAGE_TARGET_KINDS = ["task", "routine", "goal", "tracker"] as const;

/** Items a page can still be linked to, matching `query`. */
export function pageLinkCandidates<T extends { id: string; kind: string; title: string }>(
  items: readonly T[],
  linked: ReadonlySet<string>,
  query: string,
): T[] {
  const needle = query.trim().toLocaleLowerCase("tr");
  return items.filter(
    (item) =>
      (PAGE_TARGET_KINDS as readonly string[]).includes(item.kind) &&
      !linked.has(item.id) &&
      (needle === "" || item.title.toLocaleLowerCase("tr").includes(needle)),
  );
}
