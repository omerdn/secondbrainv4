import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";

/** Links that launched the app, then every link opened while it runs (desktop shell only). */
export function listenForDeepLinks(onLink: (url: string) => void): () => void {
  let stop: (() => void) | null = null;
  let cancelled = false;
  void getCurrent().then((urls) => urls?.forEach(onLink));
  void onOpenUrl((urls) => urls.forEach(onLink)).then((unlisten) => {
    if (cancelled) unlisten();
    else stop = unlisten;
  });
  return () => {
    cancelled = true;
    stop?.();
  };
}
