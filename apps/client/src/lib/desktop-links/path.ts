/** Scheme of the desktop deep links (T4.1.5). */
export const DEEP_LINK_SCHEME = "routune";

/**
 * The in-app route of a `routune://` link, e.g. `routune://verify-email?token=x` →
 * `/verify-email?token=x`. Anything else (other schemes, malformed URLs) is ignored, so a
 * link can only navigate inside the app.
 */
export function deepLinkPath(link: string): string | null {
  let url: URL;
  try {
    url = new URL(link);
  } catch {
    return null;
  }
  if (url.protocol !== `${DEEP_LINK_SCHEME}:`) return null;
  const segments = [url.hostname, ...url.pathname.split("/")]
    .map((part) => part.trim())
    .filter((part) => part !== "" && part !== "." && part !== "..");
  return `/${segments.map((part) => encodeURIComponent(decodeURIComponent(part))).join("/")}${url.search}`;
}
