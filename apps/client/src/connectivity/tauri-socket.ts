import type { SocketLike } from "./link";

/** Native builds never run inside the desktop shell; see `tauri-socket.web.ts`. */
export function tauriSocket(_url: string, _token: string | null): SocketLike {
  throw new Error("the Tauri socket exists only in the desktop shell");
}
