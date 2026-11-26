/** The global search shortcut (T2.7.6): ⌘K / Ctrl+K anywhere, or "/" outside a text field. */
export interface ShortcutEvent {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  /** Whether the focus is in a field that takes text. */
  typing: boolean;
}

export function isSearchShortcut(event: ShortcutEvent): boolean {
  if (event.altKey) return false;
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") return true;
  return event.key === "/" && !event.metaKey && !event.ctrlKey && !event.typing;
}
