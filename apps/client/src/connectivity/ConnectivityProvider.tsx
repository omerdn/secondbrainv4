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

/** Opens the link for the current session. */
export type LinkOpener = (authenticated: boolean, handlers: LinkHandlers) => Link;

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
 * takes headers as a third argument, which the DOM typings do not know.
 */
function createSocket(url: string, token: string | null): SocketLike {
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

/** Signed in: `/ws` (falling back to the probe); signed out: the `/health` probe. */
export const defaultLinkOpener: LinkOpener = (authenticated, handlers) => {
  const probe = (h: LinkHandlers) => openProbeLink(API_URL, h, globalThis.fetch.bind(globalThis));
  if (!authenticated) return probe(handlers);
  return openWithFallback(
    (h) => openSocketLink(socketUrl(API_URL), credentials.token, h, createSocket),
    probe,
    handlers,
  );
};

const isNetworkError = (error: unknown) => error instanceof ApiError && error.code === "network";

/**
 * Runs the connection state machine (ARCHITECTURE §10.2) and exposes its state. Screens for
 * the blocked and reconnecting states come with T0.5.4.
 */
export function ConnectivityProvider({
  authenticated,
  children,
  openLink = defaultLinkOpener,
  network = netInfoSource,
  random = Math.random,
}: {
  /** Whether a session exists (decides between the socket and the probe). */
  authenticated: boolean;
  children: ReactNode;
  openLink?: LinkOpener;
  network?: NetworkSource;
  random?: () => number;
}) {
  const queryClient = useQueryClient();
  const [state, setState] = useState<ConnectivityState>(initial().state);
  const stateRef = useRef<ConnectivityState>(state);
  const linkRef = useRef<Link | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const authenticatedRef = useRef(authenticated);
  const listenersRef = useRef(new Set<MessageListener>());
  // Effects run from callbacks; keep the latest dependencies in refs.
  const depsRef = useRef({ openLink, random, queryClient });
  depsRef.current = { openLink, random, queryClient };

  const dispatchRef = useRef<(event: ConnectivityEvent) => void>(() => {});

  const run = useCallback((effect: ConnectivityEffect) => {
    switch (effect.type) {
      case "open": {
        linkRef.current?.close();
        const dispatch = (event: ConnectivityEvent) => dispatchRef.current(event);
        linkRef.current = depsRef.current.openLink(authenticatedRef.current, {
          up: () => dispatch({ type: "link_up" }),
          failed: () => dispatch({ type: "link_failed" }),
          lost: (reason) => dispatch({ type: "link_lost", reason }),
          message: (message) => {
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
    if (authenticatedRef.current === authenticated) return;
    authenticatedRef.current = authenticated;
    if (stateRef.current.kind !== "offline_blocked") run({ type: "open" });
  }, [authenticated, run]);

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
