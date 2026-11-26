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
