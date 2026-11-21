import { act, render, screen } from "@testing-library/react-native";
import { Text } from "react-native";

import { useSession } from "@/features/auth";
import { credentials } from "@/lib/api";

import {
  apiError,
  type Call,
  fakeServer,
  json,
  memoryTokenStore,
  Providers,
  resetCredentials,
  user,
} from "./support/api";

let session: ReturnType<typeof useSession>;

function Probe() {
  session = useSession();
  const { state } = session;
  return (
    <Text testID="state">{state.status === "signedIn" ? `in:${state.user.id}` : state.status}</Text>
  );
}

beforeEach(resetCredentials);

describe("SessionProvider — bearer (native)", () => {
  it("starts signed out without a saved token, without calling the server", async () => {
    const { api, calls } = fakeServer({});
    await render(
      <Providers api={api} transport="bearer">
        <Probe />
      </Providers>,
    );
    expect(await screen.findByText("signedOut")).toBeOnTheScreen();
    expect(calls).toEqual([]);
    expect(session.client).toBe("native");
  });

  it("restores a saved token", async () => {
    const { api, calls } = fakeServer({ "GET /me": () => json(200, user) });
    await render(
      <Providers api={api} transport="bearer" tokenStore={memoryTokenStore("saved")}>
        <Probe />
      </Providers>,
    );
    expect(await screen.findByText("in:u1")).toBeOnTheScreen();
    expect(credentials.token).toBe("saved");
    expect(calls.map((c) => `${c.method} ${c.path}`)).toEqual(["GET /me"]);
  });

  it("forgets a token the server no longer accepts", async () => {
    const store = memoryTokenStore("expired");
    const { api } = fakeServer({ "GET /me": () => apiError(401, "unauthenticated") });
    await render(
      <Providers api={api} transport="bearer" tokenStore={store}>
        <Probe />
      </Providers>,
    );
    expect(await screen.findByText("signedOut")).toBeOnTheScreen();
    expect(store.value).toBeNull();
    expect(credentials.token).toBeNull();
  });

  it("stays blocked when the server is unreachable, and retries", async () => {
    let up = false;
    const { api } = fakeServer({
      "GET /me": () => (up ? json(200, user) : apiError(503, "internal")),
    });
    await render(
      <Providers api={api} transport="bearer" tokenStore={memoryTokenStore("t")}>
        <Probe />
      </Providers>,
    );
    expect(await screen.findByText("unavailable")).toBeOnTheScreen();
    up = true;
    await act(() => session.retry());
    expect(await screen.findByText("in:u1")).toBeOnTheScreen();
  });

  it("stores the token of a sign-in and clears everything on sign-out", async () => {
    const store = memoryTokenStore();
    const logout: Call[] = [];
    const { api } = fakeServer({
      "POST /auth/logout": (call) => {
        logout.push(call);
        return new Response(null, { status: 204 });
      },
    });
    await render(
      <Providers api={api} transport="bearer" tokenStore={store}>
        <Probe />
      </Providers>,
    );
    await screen.findByText("signedOut");
    await act(() => session.signedIn({ user, token: "new", expires_at: "2027-01-01T00:00:00Z" }));
    expect(screen.getByTestId("state")).toHaveTextContent("in:u1");
    expect(store.value).toBe("new");
    expect(credentials.token).toBe("new");

    await act(() => session.signOut());
    expect(screen.getByTestId("state")).toHaveTextContent("signedOut");
    expect(logout).toHaveLength(1);
    expect(store.value).toBeNull();
    expect(credentials.token).toBeNull();
  });

  it("signs out locally even if the server call fails", async () => {
    const store = memoryTokenStore("t");
    const { api } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/logout": () => apiError(500, "internal"),
    });
    await render(
      <Providers api={api} transport="bearer" tokenStore={store}>
        <Probe />
      </Providers>,
    );
    await screen.findByText("in:u1");
    await act(() => session.signOut());
    expect(screen.getByTestId("state")).toHaveTextContent("signedOut");
    expect(store.value).toBeNull();
  });

  it("rejects a native sign-in response without a token", async () => {
    const { api } = fakeServer({});
    await render(
      <Providers api={api} transport="bearer">
        <Probe />
      </Providers>,
    );
    await screen.findByText("signedOut");
    await expect(session.signedIn({ user, expires_at: "x" })).rejects.toThrow("no token");
  });
});

describe("SessionProvider — cookie (web)", () => {
  it("restores the cookie session and its CSRF token after a reload", async () => {
    const { api, calls } = fakeServer({
      "GET /auth/csrf": () => json(200, { csrf_token: "csrf-1" }),
      "GET /me": () => json(200, user),
    });
    await render(
      <Providers api={api} transport="cookie">
        <Probe />
      </Providers>,
    );
    expect(await screen.findByText("in:u1")).toBeOnTheScreen();
    expect(credentials.csrfToken).toBe("csrf-1");
    expect(calls.map((c) => c.path)).toEqual(["/auth/csrf", "/me"]);
    expect(session.client).toBe("web");
  });

  it("is signed out without a cookie session", async () => {
    const { api } = fakeServer({ "GET /auth/csrf": () => apiError(401, "unauthenticated") });
    await render(
      <Providers api={api} transport="cookie">
        <Probe />
      </Providers>,
    );
    expect(await screen.findByText("signedOut")).toBeOnTheScreen();
  });

  it("keeps the CSRF token of a sign-in and sends it on sign-out", async () => {
    const { api, calls } = fakeServer({
      "GET /auth/csrf": () => apiError(401, "unauthenticated"),
      "POST /auth/logout": () => new Response(null, { status: 204 }),
    });
    await render(
      <Providers api={api} transport="cookie">
        <Probe />
      </Providers>,
    );
    await screen.findByText("signedOut");
    await act(() => session.signedIn({ user, csrf_token: "csrf-2", expires_at: "x" }));
    expect(credentials.csrfToken).toBe("csrf-2");
    await act(() => session.signOut());
    expect(calls.at(-1)?.headers["X-CSRF-Token"]).toBe("csrf-2");
    expect(credentials.csrfToken).toBeNull();
  });
});
