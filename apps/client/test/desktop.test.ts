import { pluginSocketLike, type PluginMessage } from "@/connectivity/tauri-socket-adapter";
import { isTauri, sessionTransport } from "@/lib/platform";

// T4.1.1 / ADR-0020: the desktop shell runs the web build but carries the session as a
// bearer token and opens /ws from Rust.
describe("desktop shell detection", () => {
  it("recognises the Tauri webview", () => {
    expect(isTauri({ __TAURI_INTERNALS__: {} })).toBe(true);
    expect(isTauri({})).toBe(false);
  });

  it("uses the cookie only in a plain browser", () => {
    expect(sessionTransport("web", false)).toBe("cookie");
    expect(sessionTransport("web", true)).toBe("bearer");
    expect(sessionTransport("ios", false)).toBe("bearer");
    expect(sessionTransport("android", false)).toBe("bearer");
  });
});

function fakePlugin() {
  let listener: ((message: PluginMessage) => void) | null = null;
  const sent: string[] = [];
  const socket = {
    addListener: (l: (message: PluginMessage) => void) => {
      listener = l;
    },
    send: jest.fn((message: string) => {
      sent.push(message);
      return Promise.resolve();
    }),
    disconnect: jest.fn(() => Promise.resolve()),
  };
  let resolve!: (value: typeof socket) => void;
  let reject!: (error: unknown) => void;
  const connect = jest.fn(
    () =>
      new Promise<typeof socket>((res, rej) => {
        resolve = res;
        reject = rej;
      }),
  );
  return {
    socket,
    sent,
    connect,
    open: () => resolve(socket),
    fail: () => reject(new Error("refused")),
    emit: (message: PluginMessage) => listener?.(message),
  };
}

const flush = () => new Promise((r) => setImmediate(r));

describe("Tauri WebSocket adapter", () => {
  it("connects with the bearer header and relays open, text and close", async () => {
    const plugin = fakePlugin();
    const like = pluginSocketLike("wss://api.test/ws", "t0k", plugin.connect);
    const events: string[] = [];
    like.onopen = () => events.push("open");
    like.onmessage = (e) => events.push(`msg ${String(e.data)}`);
    like.onclose = (e) => events.push(`close ${String(e.code)}`);

    expect(plugin.connect).toHaveBeenCalledWith("wss://api.test/ws", {
      headers: { Authorization: "Bearer t0k" },
    });
    plugin.open();
    await flush();
    like.send('{"type":"ping"}');
    plugin.emit({ type: "Text", data: '{"type":"hello"}' });
    plugin.emit({ type: "Ping", data: null });
    plugin.emit({ type: "Close", data: { code: 4003, reason: "session ended" } });
    plugin.emit({ type: "Close", data: null });

    expect(plugin.sent).toEqual(['{"type":"ping"}']);
    expect(events).toEqual(["open", 'msg {"type":"hello"}', "close 4003"]);
  });

  it("reports a refused connection as error and close", async () => {
    const plugin = fakePlugin();
    const like = pluginSocketLike("wss://api.test/ws", null, plugin.connect);
    const events: string[] = [];
    like.onerror = () => events.push("error");
    like.onclose = () => events.push("close");
    expect(plugin.connect).toHaveBeenCalledWith("wss://api.test/ws", { headers: {} });
    plugin.fail();
    await flush();
    expect(events).toEqual(["error", "close"]);
  });

  it("disconnects a socket that opens after close()", async () => {
    const plugin = fakePlugin();
    const like = pluginSocketLike("wss://api.test/ws", "t", plugin.connect);
    const onopen = jest.fn();
    like.onopen = onopen;
    like.close(1000);
    plugin.open();
    await flush();
    expect(onopen).not.toHaveBeenCalled();
    expect(plugin.socket.disconnect).toHaveBeenCalled();
  });
});
