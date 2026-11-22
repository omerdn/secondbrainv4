import type { SocketLike } from "./link";

/** The parts of `@tauri-apps/plugin-websocket` used here. */
export interface PluginSocket {
  addListener(listener: (message: PluginMessage) => void): unknown;
  send(message: string): Promise<void>;
  disconnect(): Promise<void>;
}

export type PluginMessage =
  | { type: "Text"; data: string }
  | { type: "Close"; data: { code: number; reason: string } | null }
  | { type: "Binary" | "Ping" | "Pong"; data: unknown };

export type PluginConnect = (
  url: string,
  config: { headers: Record<string, string> },
) => Promise<PluginSocket>;

/**
 * A {@link SocketLike} over the Tauri WebSocket plugin, which opens the connection from Rust
 * and can send the `Authorization` header that the webview's own WebSocket cannot (ADR-0020).
 */
export function pluginSocketLike(
  url: string,
  token: string | null,
  connect: PluginConnect,
): SocketLike {
  let socket: PluginSocket | null = null;
  let closed = false;
  const finish = (code?: number) => {
    if (closed) return;
    closed = true;
    like.onclose?.({ code });
  };

  const like: SocketLike = {
    onopen: null,
    onmessage: null,
    onclose: null,
    onerror: null,
    send(data) {
      socket?.send(data).catch(() => {
        like.onerror?.();
        finish();
      });
    },
    close(code) {
      const current = socket;
      finish(code);
      void current?.disconnect().catch(() => undefined);
    },
  };

  const headers: Record<string, string> =
    token === null ? {} : { Authorization: `Bearer ${token}` };
  connect(url, { headers }).then(
    (opened) => {
      if (closed) {
        void opened.disconnect().catch(() => undefined);
        return;
      }
      socket = opened;
      opened.addListener((message) => {
        if (message.type === "Text") like.onmessage?.({ data: message.data });
        else if (message.type === "Close") finish(message.data?.code);
      });
      like.onopen?.();
    },
    () => {
      like.onerror?.();
      finish();
    },
  );
  return like;
}
