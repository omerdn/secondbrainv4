/**
 * Messages between the native side and the editor WebView for collaborative editing
 * (spike T0.6.2, ADR-0019).
 *
 * The WebView cannot authenticate a WebSocket itself (no session cookie, no Authorization
 * header, `null` Origin), and the session token must not enter it. So y-websocket inside the
 * WebView talks to a stand-in socket that relays frames over the 10tap bridge, and React
 * Native opens the real `/collab/{page_id}` socket with the bearer token.
 *
 * Every relayed connection has an id chosen by the WebView. Messages for any other id are
 * stale (the WebView reconnected, or its page was replaced) and are ignored on both sides;
 * otherwise the old socket's close would tear down the new connection.
 */
export type ToWeb =
  /** Start collaborating: who the local user is (live cursor label and colour). */
  | { type: "collab-start"; payload: { name: string; color: string } }
  | { type: "collab-open"; payload: { id: string } }
  /** A binary y-sync frame from the server, base64. */
  | { type: "collab-recv"; payload: { id: string; data: string } }
  | { type: "collab-closed"; payload: { id: string } };

export type ToNative =
  /** The WebView's provider wants connection `id` (replacing any previous one). */
  | { type: "collab-connect"; payload: { id: string } }
  /** A binary y-sync frame for the server, base64. */
  | { type: "collab-send"; payload: { id: string; data: string } }
  | { type: "collab-disconnect"; payload: { id: string } };

export function isToNative(message: { type: string }): message is ToNative {
  return (
    message.type === "collab-connect" ||
    message.type === "collab-send" ||
    message.type === "collab-disconnect"
  );
}
