import { invoke } from "@tauri-apps/api/core";

import { isTauri } from "@/lib/platform";

/**
 * Where the session token lives between launches (ARCHITECTURE §15).
 *
 * Browser: nowhere in script reach. The token is an `HttpOnly` cookie set by the server, so
 * this store is a no-op. Desktop shell: the OS secure storage (Keychain, Credential Manager,
 * Secret Service) through the shell's `session_token_*` commands (ADR-0020). Native builds
 * use `token-store.native.ts`.
 */
export const tokenStore = {
  get: (): Promise<string | null> =>
    isTauri() ? invoke<string | null>("session_token_get") : Promise.resolve(null),
  set: (token: string): Promise<void> =>
    isTauri() ? invoke("session_token_set", { token }) : Promise.resolve(),
  clear: (): Promise<void> => (isTauri() ? invoke("session_token_clear") : Promise.resolve()),
};
