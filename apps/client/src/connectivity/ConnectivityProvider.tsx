import { ApiError } from "@secondbrain/api-client";
import type { ServerMessage } from "@secondbrain/api-types";
import NetInfo from "@react-native-community/netinfo";
import { useQueryClient } from "@tanstack/react-query";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Platform } from "react-native";

import { API_URL, credentials } from "@/lib/api";
import { isTauri } from "@/lib/platform";

import { tauriSocket } from "./tauri-socket";
import { defaultRules, eventOf, type InvalidationRules, invalidationsFor } from "./invalidation";
import {
  type Link,
  type LinkHandlers,
  openProbeLink,
  openSocketLink,
  openWithFallback,
  type SocketLike,
  socketUrl,
} from "./link";
import {
  type ConnectivityEffect,
  type ConnectivityEvent,
  type ConnectivityState,
  initial,
  transition,
} from "./machine";

type MessageListener = (message: ServerMessage) => void;

export interface Connectivity {
  state: ConnectivityState;
  /** "Tekrar dene" on the no-connection screen (BP1). */
  retry: () => void;
  /** Receives server messages on `/ws` (events for query invalidation, T0.5.5). */
  addMessageListener: (listener: MessageListener) => () => void;
}

const ConnectivityContext = createContext<Connectivity | null>(null);

/** Reports OS network changes (`true` = connected); returns an unsubscribe function. */
export type NetworkSource = (listener: (connected: boolean) => void) => () => void;

/** Opens the link for the current session (`userId` null when signed out). */
export type LinkOpener = (userId: string | null, handlers: LinkHandlers) => Link;

/**
 * NetInfo on every platform (`navigator.onLine` events on web). Only changes of "connected"
 * are reported: NetInfo also emits on other changes, which would otherwise skip the retry
 * delay. Its own internet-reachability polling (a request to a third-party URL) is turned off;
 * our link already proves reachability against our server.
 */
export const netInfoSource: NetworkSource = (listener) => {
  NetInfo.configure({ reachabilityShouldRun: () => false });
  let last: boolean | null = null;
  return NetInfo.addEventListener((state) => {
    if (state.isConnected === null || state.isConnected === last) return;
    last = state.isConnected;
    listener(state.isConnected);
  });
};

/**
 * Browsers send the session cookie on the handshake by themselves; React Native's WebSocket
 * takes headers as a third argument, which the DOM typings do not know. The desktop shell
 * opens it from Rust with the bearer token (ADR-0020).
 */
function createSocket(url: string, token: string | null): SocketLike {
  if (isTauri()) return tauriSocket(url, token);
  if (Platform.OS === "web" || token === null) {
    return new WebSocket(url) as unknown as SocketLike;
  }
  const NativeSocket = WebSocket as unknown as new (
    url: string,
    protocols: string[] | null,
    options: { headers: Record<string, string> },
  ) => SocketLike;
  return new NativeSocket(url, null, { headers: { Authorization: `Bearer ${token}` } });
}

/**
 * Signed in: `/ws`, subscribed to the user's own channel (falling back to the probe); signed
 * out: the `/health` probe.
 */
export const defaultLinkOpener: LinkOpener = (userId, handlers) => {
  const probe = (h: LinkHandlers) => openProbeLink(API_URL, h, globalThis.fetch.bind(globalThis));
  if (userId === null) return probe(handlers);
  return openWithFallback(
    (h) =>
      openSocketLink(socketUrl(API_URL), credentials.token, h, createSocket, [`user:${userId}`]),
    probe,
    handlers,
  );
};

const isNetworkError = (error: unknown) => error instanceof ApiError && error.code === "network";

/**
 * Runs the connection state machine (ARCHITECTURE §10.2) and exposes its state; `/ws` events
 * invalidate the queries they concern (T0.5.5), a reconnect refetches everything.
 */
export function ConnectivityProvider({
  userId,
  children,
  openLink = defaultLinkOpener,
  network = netInfoSource,
  random = Math.random,
  invalidationRules = defaultRules,
  onSessionEnded,
}: {
  /** The signed-in user, or null (decides between the socket and the probe). */
  userId: string | null;
  children: ReactNode;
  openLink?: LinkOpener;
  network?: NetworkSource;
  random?: () => number;
  /** Which queries each `/ws` event invalidates (T0.5.5). */
  invalidationRules?: InvalidationRules;
  /** The server ended the session (close code 4003): sign the user out locally (T0.5.7). */
  onSessionEnded?: () => void;
}) {
  const queryClient = useQueryClient();
  const [state, setState] = useState<ConnectivityState>(initial().state);
  const stateRef = useRef<ConnectivityState>(state);
  const linkRef = useRef<Link | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const userIdRef = useRef(userId);
  const listenersRef = useRef(new Set<MessageListener>());
  // Effects run from callbacks; keep the latest dependencies in refs.
  const depsRef = useRef({ openLink, random, queryClient, invalidationRules, onSessionEnded });
  depsRef.current = { openLink, random, queryClient, invalidationRules, onSessionEnded };

  const dispatchRef = useRef<(event: ConnectivityEvent) => void>(() => {});

  const run = useCallback((effect: ConnectivityEffect) => {
    switch (effect.type) {
      case "open": {
        linkRef.current?.close();
        const dispatch = (event: ConnectivityEvent) => dispatchRef.current(event);
        linkRef.current = depsRef.current.openLink(userIdRef.current, {
          up: () => dispatch({ type: "link_up" }),
          failed: () => dispatch({ type: "link_failed" }),
          lost: (reason) => dispatch({ type: "link_lost", reason }),
          sessionEnded: () => {
            dispatch({ type: "session_ended" });
            depsRef.current.onSessionEnded?.();
          },
          message: (message) => {
            const event = eventOf(message);
            if (event !== null) {
              const { queryClient: client, invalidationRules: rules } = depsRef.current;
              for (const queryKey of invalidationsFor(event, rules)) {
                void client.invalidateQueries({ queryKey });
              }
            }
            for (const listener of listenersRef.current) listener(message);
          },
        });
        break;
      }
      case "close":
        linkRef.current?.close();
        linkRef.current = null;
        break;
      case "check":
        linkRef.current?.check();
        break;
      case "schedule_retry":
        clearTimeout(timerRef.current);
        timerRef.current = setTimeout(() => dispatchRef.current({ type: "retry" }), effect.delayMs);
        break;
      case "cancel_retry":
        clearTimeout(timerRef.current);
        timerRef.current = undefined;
        break;
      case "refetch_all":
        void depsRef.current.queryClient.invalidateQueries();
        break;
    }
  }, []);

  dispatchRef.current = (event: ConnectivityEvent) => {
    const next = transition(stateRef.current, event, depsRef.current.random);
    stateRef.current = next.state;
    setState(next.state);
    next.effects.forEach(run);
  };

  // Start (and restart after a strict-mode remount) from BOOTING.
  useEffect(() => {
    const start = initial();
    stateRef.current = start.state;
    setState(start.state);
    start.effects.forEach(run);
    return () => {
      clearTimeout(timerRef.current);
      linkRef.current?.close();
      linkRef.current = null;
    };
  }, [run]);

  // Signing in or out swaps the link (socket ↔ probe) without leaving the current state.
  useEffect(() => {
    if (userIdRef.current === userId) return;
    userIdRef.current = userId;
    if (stateRef.current.kind !== "offline_blocked") run({ type: "open" });
  }, [userId, run]);

  useEffect(
    () =>
      network((connected) => dispatchRef.current({ type: connected ? "os_online" : "os_offline" })),
    [network],
  );

  // Failed HTTP requests are a signal too (BP5).
  useEffect(() => {
    const report = () => dispatchRef.current({ type: "request_failed" });
    const unsubscribeQueries = queryClient.getQueryCache().subscribe((event) => {
      if (
        event.type === "updated" &&
        event.action.type === "error" &&
        isNetworkError(event.action.error)
      ) {
        report();
      }
    });
    const unsubscribeMutations = queryClient.getMutationCache().subscribe((event) => {
      if (
        event.type === "updated" &&
        event.action.type === "error" &&
        isNetworkError(event.action.error)
      ) {
        report();
      }
    });
    return () => {
      unsubscribeQueries();
      unsubscribeMutations();
    };
  }, [queryClient]);

  const retry = useCallback(() => dispatchRef.current({ type: "retry" }), []);
  const addMessageListener = useCallback((listener: MessageListener) => {
    listenersRef.current.add(listener);
    return () => {
      listenersRef.current.delete(listener);
    };
  }, []);

  const value = useMemo(
    () => ({ state, retry, addMessageListener }),
    [state, retry, addMessageListener],
  );
  return <ConnectivityContext.Provider value={value}>{children}</ConnectivityContext.Provider>;
}

export function useConnectivity(): Connectivity {
  const value = useContext(ConnectivityContext);
  if (value === null) throw new Error("useConnectivity must be used inside ConnectivityProvider");
  return value;
}

/**
 * Calls `handler` with the data of every `/ws` event named `name` (such as `session.timer`,
 * which carries state rather than ids). Does nothing outside the provider.
 */
export function useServerEvent(name: string, handler: (data: unknown) => void): void {
  const value = useContext(ConnectivityContext);
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  const addMessageListener = value?.addMessageListener;
  useEffect(() => {
    if (addMessageListener === undefined) return undefined;
    return addMessageListener((message) => {
      const event = eventOf(message);
      if (event?.name === name) handlerRef.current(event.data);
    });
  }, [addMessageListener, name]);
}
