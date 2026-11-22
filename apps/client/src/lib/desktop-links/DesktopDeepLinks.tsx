import { type Href, useRouter } from "expo-router";
import { useEffect } from "react";

import { isTauri } from "@/lib/platform";

import { listenForDeepLinks } from "./listen";
import { deepLinkPath } from "./path";

/** Routes `routune://` links opened on the desktop inside the app (T4.1.5). */
export function DesktopDeepLinks({
  listen = isTauri() ? listenForDeepLinks : null,
}: {
  listen?: ((onLink: (url: string) => void) => () => void) | null;
}) {
  const router = useRouter();
  useEffect(() => {
    if (listen === null) return;
    return listen((url) => {
      const path = deepLinkPath(url);
      if (path !== null) router.push(path as Href);
    });
  }, [listen, router]);
  return null;
}
