import { ApiError } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import type { AuthResponse, ClientKind, CurrentUser } from "@secondbrain/api-types";
import { useQueryClient } from "@tanstack/react-query";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { Platform } from "react-native";

import { credentials } from "@/lib/api";
import { isTauri, sessionTransport, type Transport } from "@/lib/platform";

import { tokenStore as defaultTokenStore } from "./token-store";

/**
 * - `loading`: restoring a saved session at start-up.
 * - `unavailable`: the server could not be asked (network, 5xx); the app is online-only
 *   (ADR-0003), so nothing is shown until a retry succeeds.
 */
export type SessionState =
  | { status: "loading" }
  | { status: "unavailable" }
  | { status: "signedOut" }
  | { status: "signedIn"; user: CurrentUser };

export interface Session {
  state: SessionState;
  /** `client` to send with register / login, matching the transport. */
  client: ClientKind;
  /** Stores the credentials of a successful register / login / provider sign-in. */
  signedIn: (response: AuthResponse) => Promise<void>;
  /** Ends the session on the server (best effort) and forgets it locally. */
  signOut: () => Promise<void>;
  /** Forgets the session locally only (e.g. after a password reset ended it on the server). */
  forget: () => Promise<void>;
  /** Restores the session again after `unavailable`. */
  retry: () => void;
  /** Replaces the signed-in user after a profile change (`PATCH /me`). */
  updateUser: (user: CurrentUser) => void;
}

export type TokenStore = typeof defaultTokenStore;

export type { Transport } from "@/lib/platform";

/** Browsers use the cookie; native apps and the desktop shell a bearer token (ADR-0020). */
export function defaultTransport(os: string, tauri = isTauri()): Transport {
  return sessionTransport(os, tauri);
}

const SessionContext = createContext<Session | null>(null);

export interface SessionProviderProps {
  children: ReactNode;
  /** Defaults by platform; injectable for tests. */
  transport?: Transport;
  tokenStore?: TokenStore;
}

export function SessionProvider({
  children,
  transport = defaultTransport(Platform.OS),
  tokenStore = defaultTokenStore,
}: SessionProviderProps) {
  const api = useApi();
  const queryClient = useQueryClient();
  const [state, setState] = useState<SessionState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);

  const clearLocal = useCallback(async () => {
    credentials.token = null;
    credentials.csrfToken = null;
    queryClient.clear();
    await tokenStore.clear().catch(() => undefined);
  }, [queryClient, tokenStore]);

  useEffect(() => {
    let cancelled = false;
    const settle = (next: SessionState) => {
      if (!cancelled) setState(next);
    };
    void (async () => {
      try {
        if (transport === "bearer") {
          const token = await tokenStore.get();
          if (token === null) return settle({ status: "signedOut" });
          credentials.token = token;
        } else {
          // After a reload (or an OAuth redirect) the cookie may still hold a session; the CSRF
          // token is derived from it and has to be fetched again.
          credentials.csrfToken = (await api.auth.csrf()).csrf_token;
        }
        settle({ status: "signedIn", user: await api.me.get() });
      } catch (error) {
        if (error instanceof ApiError && error.code === "unauthenticated") {
          await clearLocal();
          settle({ status: "signedOut" });
        } else {
          settle({ status: "unavailable" });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [api, attempt, clearLocal, tokenStore, transport]);

  const signedIn = useCallback(
    async (response: AuthResponse) => {
      queryClient.clear();
      if (transport === "bearer") {
        if (response.token === undefined) throw new Error("native sign-in returned no token");
        await tokenStore.set(response.token);
        credentials.token = response.token;
      } else {
        credentials.csrfToken = response.csrf_token ?? null;
      }
      setState({ status: "signedIn", user: response.user });
    },
    [queryClient, tokenStore, transport],
  );

  const forget = useCallback(async () => {
    await clearLocal();
    setState({ status: "signedOut" });
  }, [clearLocal]);

  const signOut = useCallback(async () => {
    await api.auth.logout().catch(() => undefined);
    await forget();
  }, [api, forget]);

  const retry = useCallback(() => {
    setState({ status: "loading" });
    setAttempt((n) => n + 1);
  }, []);

  const updateUser = useCallback((user: CurrentUser) => {
    setState((current) => (current.status === "signedIn" ? { status: "signedIn", user } : current));
  }, []);

  const client: ClientKind = transport === "cookie" ? "web" : "native";
  const value = useMemo(
    () => ({ state, client, signedIn, signOut, forget, retry, updateUser }),
    [state, client, signedIn, signOut, forget, retry, updateUser],
  );
  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

export function useSession(): Session {
  const session = useContext(SessionContext);
  if (session === null) throw new Error("useSession must be used inside <SessionProvider>");
  return session;
}
