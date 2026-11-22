/**
 * Connection state machine (ARCHITECTURE §10.2, PRD BP1–BP5). Pure: it decides what to do;
 * the provider (`ConnectivityProvider`) performs the effects.
 *
 * ```
 * BOOTING ──(no connection)──▶ OFFLINE_BLOCKED ──(retry / OS back online)──▶ BOOTING
 * BOOTING ──(ok)──▶ ONLINE ──(lost)──▶ RECONNECTING ──(ok)──▶ ONLINE (+ refetch everything)
 * ```
 *
 * Signals (BP5, never polled every second): the link (WebSocket heartbeat, or the health probe
 * when signed out), OS network events and failed HTTP requests.
 */

export type ConnectivityState =
  /** First connection attempt after launch; nothing is shown yet (BP1). */
  | { kind: "booting" }
  /** The first attempt failed: "no connection" screen with a retry button (BP1). */
  | { kind: "offline_blocked" }
  | { kind: "online" }
  /** Connection lost while running: full-screen lock, retries with growing delays (BP2). */
  | { kind: "reconnecting"; attempt: number };

/** Why a live link was considered lost. */
export type LostReason = "socket_closed" | "heartbeat_timeout" | "check_failed";

export type ConnectivityEvent =
  /** The link is up: WebSocket `hello` received, or the health probe answered. */
  | { type: "link_up" }
  /** A connection attempt failed. */
  | { type: "link_failed" }
  /** A link that was up died. */
  | { type: "link_lost"; reason: LostReason }
  | { type: "os_offline" }
  | { type: "os_online" }
  /** An HTTP request got no response at all (`ApiError` code `network`). */
  | { type: "request_failed" }
  /** The user pressed retry, or the retry timer fired. */
  | { type: "retry" };

export type ConnectivityEffect =
  /** Open a new link (closing any previous one). */
  | { type: "open" }
  /** Close the current link. */
  | { type: "close" }
  /** Ask the current link to prove it is alive now (reports `link_lost` if not). */
  | { type: "check" }
  /** Fire `retry` after `delayMs`. */
  | { type: "schedule_retry"; delayMs: number }
  | { type: "cancel_retry" }
  /** Invalidate and refetch all server state (BP2: "veriler tazelenir"). */
  | { type: "refetch_all" };

export interface Transition {
  state: ConnectivityState;
  effects: ConnectivityEffect[];
}

/** First retry delay; doubles per attempt up to {@link MAX_RETRY_DELAY_MS}. */
export const BASE_RETRY_DELAY_MS = 1_000;
export const MAX_RETRY_DELAY_MS = 30_000;

/**
 * Delay before reconnect attempt `attempt + 1`: exponential (1 s, 2 s, 4 s … 30 s) with
 * jitter in [50 %, 100 %] so many clients do not reconnect in lockstep after a server restart.
 * `random` is in [0, 1).
 */
export function retryDelay(attempt: number, random: number): number {
  const exponent = Math.max(0, attempt - 1);
  const ceiling = Math.min(MAX_RETRY_DELAY_MS, BASE_RETRY_DELAY_MS * 2 ** Math.min(exponent, 30));
  return Math.round(ceiling * (0.5 + random / 2));
}

/** Starting point: booting, with the first link being opened. */
export function initial(): Transition {
  return { state: { kind: "booting" }, effects: [{ type: "open" }] };
}

const stay = (state: ConnectivityState): Transition => ({ state, effects: [] });

/**
 * Next state and effects for `event`. `random` feeds the retry jitter (injected for tests).
 */
export function transition(
  state: ConnectivityState,
  event: ConnectivityEvent,
  random: () => number = Math.random,
): Transition {
  switch (state.kind) {
    case "booting":
      switch (event.type) {
        case "link_up":
          return stay({ kind: "online" });
        case "link_failed":
        case "link_lost":
        case "os_offline":
          return { state: { kind: "offline_blocked" }, effects: [{ type: "close" }] };
        default:
          return stay(state);
      }

    case "offline_blocked":
      switch (event.type) {
        case "retry":
        case "os_online":
          return { state: { kind: "booting" }, effects: [{ type: "open" }] };
        default:
          return stay(state);
      }

    case "online":
      switch (event.type) {
        case "link_lost":
        case "os_offline":
          // First attempt right away; growing delays only after it fails.
          return { state: { kind: "reconnecting", attempt: 1 }, effects: [{ type: "open" }] };
        case "link_failed":
          // A replacement link (e.g. after signing in) could not be opened.
          return { state: { kind: "reconnecting", attempt: 1 }, effects: [{ type: "open" }] };
        case "request_failed":
          // One failed request is not proof (CORS, a single bad route): verify the link.
          return { state, effects: [{ type: "check" }] };
        default:
          return stay(state);
      }

    case "reconnecting":
      switch (event.type) {
        case "link_up":
          return {
            state: { kind: "online" },
            effects: [{ type: "cancel_retry" }, { type: "refetch_all" }],
          };
        case "link_failed":
        case "link_lost":
          return {
            state,
            effects: [{ type: "schedule_retry", delayMs: retryDelay(state.attempt, random()) }],
          };
        case "retry":
          return {
            state: { kind: "reconnecting", attempt: state.attempt + 1 },
            effects: [{ type: "open" }],
          };
        case "os_online":
          // The OS says the network is back: try now instead of waiting for the timer.
          return {
            state: { kind: "reconnecting", attempt: state.attempt + 1 },
            effects: [{ type: "cancel_retry" }, { type: "open" }],
          };
        default:
          return stay(state);
      }
  }
}
