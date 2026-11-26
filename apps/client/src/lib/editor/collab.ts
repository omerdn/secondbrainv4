import { API_URL } from "@/lib/api";

/**
 * Collaborative editing (ADR-0019, T2.5.7): shared pieces of the web and native editors. The
 * server speaks y-sync at `/collab/{page_id}` and checks access to the page.
 */

/** `http(s)://host` → `ws(s)://host/collab`; y-websocket appends `/{room}`. */
export function collabServerUrl(apiUrl: string = API_URL): string {
  return `${apiUrl.replace(/\/+$/u, "").replace(/^http/u, "ws")}/collab`;
}

/**
 * The Yjs fragment holding the page body (ProseMirror document). The server reads the same
 * root for plain text, snapshots and process page lines (`notebooks::snapshots`).
 */
export const PAGE_FIELD = "default";

/** Paragraph attributes the server writes on process page lines (T3); the editor keeps them. */
export const SERVER_LINE_ATTRIBUTES = ["blockId", "date"] as const;

/** The y-websocket connection state. */
export type CollabStatus = "connecting" | "connected" | "disconnected";

const CURSOR_COLORS = ["#2563eb", "#db2777", "#16a34a", "#ea580c", "#7c3aed", "#0891b2"];

/** A stable colour for a user's live cursor. */
export function cursorColor(userId: string): string {
  let hash = 0;
  for (const char of userId) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return CURSOR_COLORS[hash % CURSOR_COLORS.length] ?? "#2563eb";
}

export interface CollabEditorProps {
  pageId: string;
  user: { id: string; name: string };
  /** Shown without editing (no EDIT on the page; the server drops edits anyway). */
  readOnly?: boolean;
}
