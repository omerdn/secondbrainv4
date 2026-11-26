import type { LinkPreview } from "@secondbrain/api-types";

/**
 * Embeds and link cards in pages (PRD Y4, ADR-0013, T2.5.8). The server decides what a link
 * becomes; the editor checks again before rendering, because page content can come from any
 * collaborator: only these origins are ever put in an iframe.
 */
export const EMBED_ORIGINS = [
  "https://www.youtube-nocookie.com",
  "https://open.spotify.com",
  "https://w.soundcloud.com",
  "https://drive.google.com",
  "https://docs.google.com",
] as const;

/** Whether `src` may be shown in an iframe. */
export function isAllowedEmbed(src: unknown): src is string {
  if (typeof src !== "string") return false;
  try {
    const url = new URL(src);
    return (EMBED_ORIGINS as readonly string[]).includes(url.origin);
  } catch {
    return false;
  }
}

/** `url` if it is an http(s) link, otherwise null (no `javascript:` and the like). */
export function safeLink(url: unknown): string | null {
  if (typeof url !== "string") return null;
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed.toString() : null;
  } catch {
    return null;
  }
}

/** `src` if it is an https image, otherwise null. */
export function safeImage(src: unknown): string | null {
  const link = safeLink(src);
  return link?.startsWith("https://") === true ? link : null;
}

/** The editor node for a server preview. */
export function previewNode(preview: LinkPreview) {
  return preview.type === "embed"
    ? {
        type: "embed",
        attrs: { src: preview.embed_url, provider: preview.provider, url: preview.url },
      }
    : {
        type: "linkPreview",
        attrs: {
          url: preview.url,
          title: preview.title,
          description: preview.description,
          image: preview.image,
          site: preview.site_name,
        },
      };
}
