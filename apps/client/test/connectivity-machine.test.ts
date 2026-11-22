import {
  type ConnectivityEvent,
  type ConnectivityState,
  initial,
  MAX_RETRY_DELAY_MS,
  retryDelay,
  transition,
} from "@/connectivity/machine";

const half = () => 0.5;

function run(events: ConnectivityEvent[], from: ConnectivityState = initial().state) {
  let state = from;
  const effects = [];
  for (const event of events) {
    const next = transition(state, event, half);
    state = next.state;
    effects.push(...next.effects);
  }
  return { state, effects };
}

describe("connectivity state machine (ARCHITECTURE §10.2)", () => {
  it("boots by opening a link", () => {
    expect(initial()).toEqual({ state: { kind: "booting" }, effects: [{ type: "open" }] });
  });

  it("goes online when the first link comes up", () => {
    expect(run([{ type: "link_up" }]).state).toEqual({ kind: "online" });
  });

  it.each<ConnectivityEvent>([
    { type: "link_failed" },
    { type: "os_offline" },
    { type: "link_lost", reason: "socket_closed" },
  ])("BP1: blocks the app when booting fails ($type)", (event) => {
    expect(run([event])).toEqual({
      state: { kind: "offline_blocked" },
      effects: [{ type: "close" }],
    });
  });

  it.each<ConnectivityEvent>([{ type: "retry" }, { type: "os_online" }])(
    "BP1: boots again from the blocked screen on $type",
    (event) => {
      expect(run([event], { kind: "offline_blocked" })).toEqual({
        state: { kind: "booting" },
        effects: [{ type: "open" }],
      });
    },
  );

  it("BP1: stays blocked on other signals", () => {
    const blocked: ConnectivityState = { kind: "offline_blocked" };
    for (const event of [
      { type: "link_up" },
      { type: "link_failed" },
      { type: "os_offline" },
      { type: "request_failed" },
    ] as const) {
      expect(transition(blocked, event, half)).toEqual({ state: blocked, effects: [] });
    }
  });

  it.each<ConnectivityEvent>([
    { type: "link_lost", reason: "heartbeat_timeout" },
    { type: "os_offline" },
    { type: "link_failed" },
  ])("BP2: a lost connection starts reconnecting right away ($type)", (event) => {
    expect(run([event], { kind: "online" })).toEqual({
      state: { kind: "reconnecting", attempt: 1 },
      effects: [{ type: "open" }],
    });
  });

  it("BP5: a failed request only asks the link to prove itself", () => {
    expect(run([{ type: "request_failed" }], { kind: "online" })).toEqual({
      state: { kind: "online" },
      effects: [{ type: "check" }],
    });
  });

  it("BP2: retries with growing delays until the link is back, then refetches", () => {
    const { state, effects } = run(
      [
        { type: "link_lost", reason: "socket_closed" },
        { type: "link_failed" },
        { type: "retry" },
        { type: "link_failed" },
        { type: "retry" },
        { type: "link_up" },
      ],
      { kind: "online" },
    );
    expect(state).toEqual({ kind: "online" });
    expect(effects).toEqual([
      { type: "open" },
      { type: "schedule_retry", delayMs: retryDelay(1, 0.5) },
      { type: "open" },
      { type: "schedule_retry", delayMs: retryDelay(2, 0.5) },
      { type: "open" },
      { type: "cancel_retry" },
      { type: "refetch_all" },
    ]);
  });

  it("BP2: the OS coming back online skips the wait", () => {
    expect(run([{ type: "os_online" }], { kind: "reconnecting", attempt: 3 })).toEqual({
      state: { kind: "reconnecting", attempt: 4 },
      effects: [{ type: "cancel_retry" }, { type: "open" }],
    });
  });

  it("ignores signals that do not change anything", () => {
    const reconnecting: ConnectivityState = { kind: "reconnecting", attempt: 2 };
    for (const event of [{ type: "os_offline" }, { type: "request_failed" }] as const) {
      expect(transition(reconnecting, event, half)).toEqual({ state: reconnecting, effects: [] });
    }
    for (const event of [{ type: "link_up" }, { type: "os_online" }, { type: "retry" }] as const) {
      expect(transition({ kind: "online" }, event, half).effects).toEqual([]);
    }
    for (const event of [
      { type: "os_online" },
      { type: "request_failed" },
      { type: "retry" },
    ] as const) {
      expect(transition({ kind: "booting" }, event, half)).toEqual({
        state: { kind: "booting" },
        effects: [],
      });
    }
  });
});

describe("retry delays (BP5: no per-second checks)", () => {
  it("double per attempt and are capped", () => {
    const ceilings = [1, 2, 3, 4, 5, 6, 7, 50].map((attempt) => retryDelay(attempt, 0.999999));
    expect(ceilings).toEqual([1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000, 30_000]);
  });

  it("are jittered between half and the full delay", () => {
    expect(retryDelay(3, 0)).toBe(2_000);
    expect(retryDelay(3, 0.5)).toBe(3_000);
    for (let attempt = 1; attempt < 40; attempt += 1) {
      for (const random of [0, 0.3, 0.7, 0.999]) {
        const delay = retryDelay(attempt, random);
        expect(delay).toBeGreaterThanOrEqual(500);
        expect(delay).toBeLessThanOrEqual(MAX_RETRY_DELAY_MS);
      }
    }
  });
});
