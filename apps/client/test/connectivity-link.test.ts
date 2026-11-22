import {
  type LinkHandlers,
  openProbeLink,
  openSocketLink,
  openWithFallback,
  RESPONSE_TIMEOUT_MS,
  type SocketLike,
  socketUrl,
} from "@/connectivity/link";

class FakeSocket implements SocketLike {
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  sent: unknown[] = [];
  closed = false;

  constructor(
    readonly url: string,
    readonly token: string | null,
  ) {}

  send(data: string) {
    this.sent.push(JSON.parse(data));
  }

  close() {
    this.closed = true;
  }

  receive(message: object) {
    this.onmessage?.({ data: JSON.stringify(message) });
  }
}

const hello = {
  type: "hello",
  server_time: "2026-11-22T09:30:00Z",
  heartbeat_secs: 25,
  idle_timeout_secs: 60,
};

function handlers() {
  const calls: string[] = [];
  const messages: unknown[] = [];
  const h: LinkHandlers = {
    up: () => calls.push("up"),
    failed: () => calls.push("failed"),
    lost: (reason) => calls.push(`lost:${reason}`),
    message: (message) => messages.push(message),
  };
  return { h, calls, messages };
}

function socketLink(token: string | null = "tok") {
  const sockets: FakeSocket[] = [];
  const { h, calls, messages } = handlers();
  const link = openSocketLink("ws://api.test/ws", token, h, (url, t) => {
    const socket = new FakeSocket(url, t);
    sockets.push(socket);
    return socket;
  });
  const socket = sockets[0];
  if (socket === undefined) throw new Error("no socket");
  return { link, socket, calls, messages };
}

beforeEach(() => jest.useFakeTimers());
afterEach(() => jest.useRealTimers());

describe("socket link", () => {
  it("derives the socket URL from the API URL", () => {
    expect(socketUrl("http://localhost:8080")).toBe("ws://localhost:8080/ws");
    expect(socketUrl("https://api.example.com/")).toBe("wss://api.example.com/ws");
  });

  it("is up on hello and passes the token to the socket", () => {
    const { socket, calls } = socketLink();
    expect(socket.token).toBe("tok");
    socket.receive(hello);
    expect(calls).toEqual(["up"]);
  });

  it("fails if hello does not arrive in time", () => {
    const { socket, calls } = socketLink();
    jest.advanceTimersByTime(RESPONSE_TIMEOUT_MS);
    expect(calls).toEqual(["failed"]);
    expect(socket.closed).toBe(true);
  });

  it("fails when the handshake is refused", () => {
    const { socket, calls } = socketLink();
    socket.onclose?.();
    expect(calls).toEqual(["failed"]);
  });

  it("pings every heartbeat and stays up while pongs arrive", () => {
    const { socket, calls } = socketLink();
    socket.receive(hello);
    for (let i = 0; i < 3; i += 1) {
      jest.advanceTimersByTime(25_000);
      expect(socket.sent.at(-1)).toEqual({ type: "ping" });
      socket.receive({ type: "pong", server_time: hello.server_time });
    }
    expect(socket.sent).toHaveLength(3);
    expect(calls).toEqual(["up"]);
  });

  it("is lost when a pong does not arrive within the timeout", () => {
    const { socket, calls } = socketLink();
    socket.receive(hello);
    jest.advanceTimersByTime(25_000 + RESPONSE_TIMEOUT_MS);
    expect(calls).toEqual(["up", "lost:heartbeat_timeout"]);
    expect(socket.closed).toBe(true);
  });

  it("is lost when the socket closes after hello", () => {
    const { socket, calls } = socketLink();
    socket.receive(hello);
    socket.onclose?.();
    expect(calls).toEqual(["up", "lost:socket_closed"]);
  });

  it("check pings now and reports a failed check", () => {
    const { link, socket, calls } = socketLink();
    socket.receive(hello);
    link.check();
    expect(socket.sent).toEqual([{ type: "ping" }]);
    jest.advanceTimersByTime(RESPONSE_TIMEOUT_MS);
    expect(calls).toEqual(["up", "lost:check_failed"]);
  });

  it("forwards events and ignores junk", () => {
    const { socket, messages, calls } = socketLink();
    socket.receive(hello);
    socket.onmessage?.({ data: "not json" });
    socket.onmessage?.({ data: new ArrayBuffer(2) });
    const event = { type: "event", channel: "user:1", event: { name: "item.changed", data: {} } };
    socket.receive(event);
    expect(messages).toEqual([event]);
    expect(calls).toEqual(["up"]);
  });

  it("reports nothing after close", () => {
    const { link, socket, calls } = socketLink();
    socket.receive(hello);
    link.close();
    expect(socket.closed).toBe(true);
    socket.onclose?.();
    jest.advanceTimersByTime(60_000);
    expect(calls).toEqual(["up"]);
  });
});

describe("probe link", () => {
  const respond = (ok: boolean) =>
    jest.fn(() => Promise.resolve(new Response(null, { status: ok ? 200 : 503 })));

  it("is up when /health answers", async () => {
    const fetchImpl = respond(true);
    const { h, calls } = handlers();
    openProbeLink("http://api.test/", h, fetchImpl);
    await jest.runAllTimersAsync();
    expect(calls).toEqual(["up"]);
    expect(fetchImpl).toHaveBeenCalledWith("http://api.test/health", expect.anything());
  });

  it("fails on an error status or no response", async () => {
    for (const fetchImpl of [
      respond(false),
      jest.fn(() => Promise.reject(new TypeError("offline"))),
    ]) {
      const { h, calls } = handlers();
      openProbeLink("http://api.test", h, fetchImpl);
      await jest.runAllTimersAsync();
      expect(calls).toEqual(["failed"]);
    }
  });

  it("a failed check reports the link lost", async () => {
    let ok = true;
    const fetchImpl = jest.fn(() =>
      Promise.resolve(new Response(null, { status: ok ? 200 : 503 })),
    );
    const { h, calls } = handlers();
    const link = openProbeLink("http://api.test", h, fetchImpl);
    await jest.runAllTimersAsync();
    link.check();
    await jest.runAllTimersAsync();
    ok = false;
    link.check();
    await jest.runAllTimersAsync();
    expect(calls).toEqual(["up", "lost:check_failed"]);
  });
});

describe("fallback", () => {
  it("opens the fallback when the primary attempt fails", () => {
    const { h, calls } = handlers();
    let primaryHandlers: LinkHandlers | undefined;
    const fallback = jest.fn((f: LinkHandlers) => {
      f.up();
      return { check: jest.fn(), close: jest.fn() };
    });
    openWithFallback(
      (p) => {
        primaryHandlers = p;
        return { check: jest.fn(), close: jest.fn() };
      },
      fallback,
      h,
    );
    primaryHandlers?.failed();
    expect(fallback).toHaveBeenCalledTimes(1);
    expect(calls).toEqual(["up"]);
  });

  it("does not open the fallback after close", () => {
    const { h } = handlers();
    let primaryHandlers: LinkHandlers | undefined;
    const fallback = jest.fn(() => ({ check: jest.fn(), close: jest.fn() }));
    const link = openWithFallback(
      (p) => {
        primaryHandlers = p;
        return { check: jest.fn(), close: jest.fn() };
      },
      fallback,
      h,
    );
    link.close();
    primaryHandlers?.failed();
    expect(fallback).not.toHaveBeenCalled();
  });
});
