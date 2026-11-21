/**
 * Where the session token lives between launches (ARCHITECTURE §15).
 *
 * Web: nowhere in script reach. The token is an `HttpOnly` cookie set by the server, so this
 * store is a no-op; native builds use `token-store.native.ts` (secure storage).
 */
export const tokenStore = {
  get: (): Promise<string | null> => Promise.resolve(null),
  set: (_token: string): Promise<void> => Promise.resolve(),
  clear: (): Promise<void> => Promise.resolve(),
};
