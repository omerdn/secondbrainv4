import TauriWebSocket from "@tauri-apps/plugin-websocket";

import type { SocketLike } from "./link";
import { pluginSocketLike } from "./tauri-socket-adapter";

/** Desktop shell: `/ws` opened from Rust with the bearer token (ADR-0020). */
export function tauriSocket(url: string, token: string | null): SocketLike {
  return pluginSocketLike(url, token, (target, config) => TauriWebSocket.connect(target, config));
}
