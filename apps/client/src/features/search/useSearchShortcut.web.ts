import { useEffect, useRef } from "react";

import { isSearchShortcut } from "./shortcut";

const typingIn = (target: EventTarget | null): boolean => {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement
  );
};

/** Opens the global search on ⌘K / Ctrl+K, or "/" outside a text field (T2.7.6). */
export function useSearchShortcut(open: () => void): void {
  const openRef = useRef(open);
  openRef.current = open;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (
        isSearchShortcut({
          key: event.key,
          metaKey: event.metaKey,
          ctrlKey: event.ctrlKey,
          altKey: event.altKey,
          typing: typingIn(event.target),
        })
      ) {
        event.preventDefault();
        openRef.current();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, []);
}
