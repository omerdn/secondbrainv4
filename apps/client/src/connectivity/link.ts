import type { ClientMessage, ServerMessage } from "@secondbrain/api-types";

import type { LostReason } from "./machine";

/**
 * A live connection to the server whose health drives the state machine. Signed-in clients use
 * the `/ws` socket (heartbeat every `heartbeat_secs`); signed-out clients, which cannot open
 * it, use a one-off `/health` probe.
 *
 * A link reports exactly one of `up` / `failed` for its attempt, then at most one `lost`.
 * After `close()` it reports nothing.
 */
export interface Link {
  /** Proves the link is alive now; reports `lost("check_failed")` if it is not. */
  check(): void;
  close(): void;
}

export interface LinkHandlers {
  up(): void;
  failed(): void;
  lost(reason: LostReason): void;
  /**
   * The server closed the link because the session ended (sign-out elsewhere, revocation,
   * password reset, expiry). Reconnecting will not help; the user must sign in again.
   */
  sessionEnded?(): void;
  /** Server messages other than `hello`/`pong` (events, subscription answers). */
  message?(message: ServerMessage): void;
}

/** Close code the server uses when the connection's session ended (T0.5.6). */
export const CLOSE_SESSION_ENDED = 4003;

/** How long a connection attempt, a ping or a probe may take (ARCHITECTURE §10.2: 10 s). */
export const RESPONSE_TIMEOUT_MS = 10_000;

/** Minimal WebSocket surface used here (browser and React Native). */
export interface SocketLike {
  onopen: (() => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: ((event: { code?: number }) => void) | null;
  onerror: (() => void) | null;
  send(data: string): void;
  close(code?: number, reason?: string): void;
}

export type SocketFactory = (url: string, token: string | null) => SocketLike;

/** `http(s)://host` → `ws(s)://host/ws`. */
export function socketUrl(apiUrl: string): string {
  return `${apiUrl.replace(/\/+$/u, "").replace(/^http/u, "ws")}/ws`;
}

function parse(data: unknown): ServerMessage | null {
  if (typeof data !== "string") return null;
  try {
    const value: unknown = JSON.parse(data);
    return typeof value === "object" && value !== null && "type" in value
      ? (value as ServerMessage)
      : null;
  } catch {
    return null;
  }
}

/**
 * The `/ws` link: up on `hello` (after subscribing to `channels`); then pings every `heartbeat_secs` and counts the link as lost
 * if a `pong` (or any message) does not arrive within {@link RESPONSE_TIMEOUT_MS}.
 */
export function openSocketLink(
  url: string,
  token: string | null,
  handlers: LinkHandlers,
  createSocket: SocketFactory,
  /** Channels to subscribe to once connected, e.g. `user:<own id>`. */
  channels: readonly string[] = [],
): Link {
  let state: "connecting" | "up" | "done" = "connecting";
  let heartbeat: ReturnType<typeof setInterval> | undefined;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  const socket = createSocket(url, token);

  const clearTimers = () => {
    clearInterval(heartbeat);
    clearTimeout(deadline);
    heartbeat = undefined;
    deadline = undefined;
  };
  const finish = (report: () => void) => {
    if (state === "done") return;
    state = "done";
    clearTimers();
    socket.onopen = socket.onmessage = socket.onclose = socket.onerror = null;
    socket.close();
    report();
  };
  const fail = (reason: LostReason) =>
    finish(state === "up" ? () => handlers.lost(reason) : () => handlers.failed());
  const send = (message: ClientMessage) => {
    try {
      socket.send(JSON.stringify(message));
    } catch {
      fail("socket_closed");
    }
  };
  const ping = (reason: LostReason) => {
    send({ type: "ping" });
    if (deadline === undefined) {
      deadline = setTimeout(() => fail(reason), RESPONSE_TIMEOUT_MS);
    }
  };

  deadline = setTimeout(() => fail("heartbeat_timeout"), RESPONSE_TIMEOUT_MS);
  socket.onmessage = (event) => {
    const message = parse(event.data);
    if (message === null || state === "done") return;
    // Any message proves the link is alive.
    clearTimeout(deadline);
    deadline = undefined;
    if (message.type === "hello") {
      if (state !== "connecting") return;
      state = "up";
      const every = Math.max(1, message.heartbeat_secs) * 1_000;
      heartbeat = setInterval(() => ping("heartbeat_timeout"), every);
      for (const channel of channels) send({ type: "subscribe", channel });
      if (state === "up") handlers.up();
    } else if (message.type !== "pong") {
      handlers.message?.(message);
    }
  };
  socket.onclose = (event) => {
    if (event.code === CLOSE_SESSION_ENDED && handlers.sessionEnded !== undefined) {
      const ended = handlers.sessionEnded.bind(handlers);
      finish(ended);
    } else {
      fail("socket_closed");
    }
  };
  // An error is always followed by `close`; nothing to do until then.
  socket.onerror = null;

  return {
    check: () => {
      if (state === "up") ping("check_failed");
    },
    close: () => {
      if (state === "done") return;
      state = "done";
      clearTimers();
      socket.onopen = socket.onmessage = socket.onclose = socket.onerror = null;
      socket.close(1000, "client closed");
    },
  };
}

/**
 * The signed-out link: `GET /health` must answer within {@link RESPONSE_TIMEOUT_MS}. There is
 * no heartbeat; OS events and failed requests (→ `check`) cover a lost connection (BP5).
 */
export function openProbeLink(
  apiUrl: string,
  handlers: LinkHandlers,
  fetchImpl: typeof fetch,
): Link {
  let state: "connecting" | "up" | "done" = "connecting";
  let controller: AbortController | undefined;

  const probe = async (): Promise<boolean> => {
    controller = new AbortController();
    const timer = setTimeout(() => controller?.abort(), RESPONSE_TIMEOUT_MS);
    try {
      const response = await fetchImpl(`${apiUrl.replace(/\/+$/u, "")}/health`, {
        signal: controller.signal,
        credentials: "omit",
      });
      return response.ok;
    } catch {
      return false;
    } finally {
      clearTimeout(timer);
    }
  };

  void probe().then((ok) => {
    if (state !== "connecting") return;
    state = ok ? "up" : "done";
    if (ok) handlers.up();
    else handlers.failed();
  });

  return {
    check: () => {
      if (state !== "up") return;
      void probe().then((ok) => {
        if (state !== "up" || ok) return;
        state = "done";
        handlers.lost("check_failed");
      });
    },
    close: () => {
      state = "done";
      controller?.abort();
    },
  };
}

/**
 * Opens `primary`; if its attempt fails, opens `fallback` instead with the same handlers.
 *
 * Signed-in clients use the socket with the probe as fallback: when the server is reachable
 * but refuses the socket (e.g. the session was revoked), the app is still online over HTTP and
 * the next request's 401 signs the user out, instead of showing "no connection" forever.
 */
export function openWithFallback(
  primary: (handlers: LinkHandlers) => Link,
  fallback: (handlers: LinkHandlers) => Link,
  handlers: LinkHandlers,
): Link {
  let closed = false;
  let current: Link = primary({
    ...handlers,
    failed: () => {
      if (!closed) current = fallback(handlers);
    },
  });
  return {
    check: () => current.check(),
    close: () => {
      closed = true;
      current.close();
    },
  };
}
