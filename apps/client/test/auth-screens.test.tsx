import { act, fireEvent, screen } from "expo-router/testing-library";

import { t } from "@/features/auth/strings";

import {
  apiError,
  type Call,
  fakeServer,
  json,
  memoryTokenStore,
  resetCredentials,
  user,
} from "./support/api";
import { renderApp } from "./support/router";

beforeEach(resetCredentials);

const press = (name: string) => act(() => fireEvent.press(screen.getByRole("button", { name })));
const type = (label: string, text: string) =>
  act(() => fireEvent.changeText(screen.getByLabelText(label), text));

describe("session gate", () => {
  it("sends visitors from / to the login screen", async () => {
    await renderApp(fakeServer({}).api, "/");
    expect(await screen.findByRole("header", { name: t.login.title })).toBeOnTheScreen();
    expect(screen).toHavePathname("/login");
  });

  it("sends signed-in users from /login home", async () => {
    const { api } = fakeServer({ "GET /me": () => json(200, user) });
    await renderApp(api, "/login", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByText("Ali")).toBeOnTheScreen();
    expect(screen).toHavePathname("/");
  });

  it("blocks with a retry when the server is unreachable", async () => {
    const { api } = fakeServer({ "GET /me": () => apiError(500, "internal") });
    await renderApp(api, "/", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByText(t.session.unavailable)).toBeOnTheScreen();
    expect(screen.getByRole("button", { name: t.session.retry })).toBeOnTheScreen();
  });
});

describe("login", () => {
  it("validates before calling the server", async () => {
    const { api, calls } = fakeServer({});
    await renderApp(api, "/login");
    await screen.findByRole("header", { name: t.login.title });
    await press(t.login.submit);
    expect(screen.getAllByText(t.validation.required)).toHaveLength(2);
    expect(calls).toEqual([]);
  });

  it("signs in, stores the token and goes home", async () => {
    const store = memoryTokenStore();
    const logins: Call[] = [];
    const { api } = fakeServer({
      "POST /auth/login": (call) => {
        logins.push(call);
        return json(200, { user, token: "tok", expires_at: "2027-01-01T00:00:00Z" });
      },
    });
    await renderApp(api, "/login", { tokenStore: store });
    await screen.findByRole("header", { name: t.login.title });
    await type(t.email, " ali@example.com ");
    await type(t.password, "correct horse");
    await press(t.login.submit);
    expect(await screen.findByText("Ali")).toBeOnTheScreen();
    expect(screen).toHavePathname("/");
    expect(logins[0]?.body).toEqual({
      email: "ali@example.com",
      password: "correct horse",
      client: "native",
      device: "iOS",
    });
    expect(store.value).toBe("tok");
  });

  it("shows wrong credentials and rate limits as a form message", async () => {
    let limited = false;
    const { api } = fakeServer({
      "POST /auth/login": () =>
        limited
          ? json(429, { error: { code: "rate_limited", message: "" } }, { "Retry-After": "12" })
          : apiError(401, "unauthenticated"),
    });
    await renderApp(api, "/login");
    await screen.findByRole("header", { name: t.login.title });
    await type(t.email, "ali@example.com");
    await type(t.password, "wrong");
    await press(t.login.submit);
    expect(await screen.findByText(t.errors.badCredentials)).toBeOnTheScreen();
    limited = true;
    await press(t.login.submit);
    expect(await screen.findByText(t.errors.rateLimited(12))).toBeOnTheScreen();
  });

  it("explains a failed provider sign-in from the redirect", async () => {
    await renderApp(fakeServer({}).api, "/login?error=google");
    expect(await screen.findByText(t.providers.failed.google)).toBeOnTheScreen();
  });

  it("links to register and forgot password", async () => {
    await renderApp(fakeServer({}).api, "/login");
    await screen.findByRole("header", { name: t.login.title });
    await press(`${t.login.noAccount} ${t.login.toRegister}`);
    expect(await screen.findByRole("header", { name: t.register.title })).toBeOnTheScreen();
    await press(`${t.register.haveAccount} ${t.register.toLogin}`);
    await screen.findByRole("header", { name: t.login.title });
    await press(t.login.forgot);
    expect(screen).toHavePathname("/forgot-password");
  });
});

describe("register", () => {
  it("creates the account and signs in", async () => {
    const { api, calls } = fakeServer({
      "POST /auth/register": () => json(201, { user, token: "tok", expires_at: "x" }),
    });
    await renderApp(api, "/register");
    await screen.findByRole("header", { name: t.register.title });
    await type(t.displayName, " Ali ");
    await type(t.email, "ali@example.com");
    await type(t.password, "correct horse");
    await press(t.register.submit);
    expect(await screen.findByText("Ali")).toBeOnTheScreen();
    expect(calls[0]?.body).toMatchObject({ display_name: "Ali", client: "native" });
  });

  it("shows a taken address and server field errors on the field", async () => {
    let conflict = true;
    const { api } = fakeServer({
      "POST /auth/register": () =>
        conflict
          ? apiError(409, "conflict")
          : apiError(422, "validation_failed", [
              { field: "password", code: "too_short", message: "too short" },
            ]),
    });
    await renderApp(api, "/register");
    await screen.findByRole("header", { name: t.register.title });
    await type(t.displayName, "Ali");
    await type(t.email, "ali@example.com");
    await type(t.password, "correct horse");
    await press(t.register.submit);
    expect(await screen.findByText(t.errors.emailTaken)).toBeOnTheScreen();
    conflict = false;
    await press(t.register.submit);
    expect(await screen.findByText(t.validation.passwordShort)).toBeOnTheScreen();
    expect(screen.queryByText(t.errors.emailTaken)).toBeNull();
  });
});

describe("forgot password", () => {
  it("always shows the same confirmation", async () => {
    const { api, calls } = fakeServer({
      "POST /auth/password/forgot": () => new Response(null, { status: 202 }),
    });
    await renderApp(api, "/forgot-password");
    await screen.findByRole("header", { name: t.forgot.title });
    await type(t.email, "nobody@example.com");
    await press(t.forgot.submit);
    expect(await screen.findByText(t.forgot.sent)).toBeOnTheScreen();
    expect(calls[0]?.body).toEqual({ email: "nobody@example.com" });
  });
});

describe("reset password", () => {
  it("explains a missing token", async () => {
    await renderApp(fakeServer({}).api, "/reset-password");
    expect(await screen.findByText(t.reset.missingToken)).toBeOnTheScreen();
  });

  it("sets the new password, forgets the session and returns to login", async () => {
    const store = memoryTokenStore("old");
    const { api, calls } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/password/reset": () => new Response(null, { status: 204 }),
    });
    await renderApp(api, "/reset-password?token=abc", { tokenStore: store });
    await screen.findByRole("header", { name: t.reset.title });
    await type(t.newPassword, "new password");
    await type(t.confirmPassword, "new passwort");
    await press(t.reset.submit);
    expect(screen.getByText(t.validation.mismatch)).toBeOnTheScreen();
    await type(t.confirmPassword, "new password");
    await press(t.reset.submit);
    expect(await screen.findByText(t.reset.done)).toBeOnTheScreen();
    expect(screen).toHavePathname("/login");
    expect(calls.find((c) => c.path === "/auth/password/reset")?.body).toEqual({
      token: "abc",
      password: "new password",
    });
    expect(store.value).toBeNull();
  });

  it("explains an expired or used link", async () => {
    const { api } = fakeServer({
      "POST /auth/password/reset": () =>
        apiError(422, "validation_failed", [{ field: "token", code: "invalid", message: "" }]),
    });
    await renderApp(api, "/reset-password?token=used");
    await screen.findByRole("header", { name: t.reset.title });
    await type(t.newPassword, "new password");
    await type(t.confirmPassword, "new password");
    await press(t.reset.submit);
    expect(await screen.findByText(t.reset.invalidToken)).toBeOnTheScreen();
    expect(screen.getByRole("button", { name: t.reset.requestNew })).toBeOnTheScreen();
  });
});

describe("home", () => {
  it("signs out back to the login screen", async () => {
    const { api } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/logout": () => new Response(null, { status: 204 }),
    });
    await renderApp(api, "/", { tokenStore: memoryTokenStore("t") });
    await screen.findByText("Ali");
    await press(t.session.signOut);
    expect(await screen.findByRole("header", { name: t.login.title })).toBeOnTheScreen();
  });
});
