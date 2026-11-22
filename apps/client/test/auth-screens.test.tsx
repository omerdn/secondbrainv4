import { act, fireEvent, screen } from "expo-router/testing-library";

import { APP_NAME } from "@/lib/app-info";
import { i18n } from "@/lib/i18n";

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

const tr = i18n.getFixedT("tr");

beforeEach(resetCredentials);

const press = (name: string) => act(() => fireEvent.press(screen.getByRole("button", { name })));
const type = (label: string, text: string) =>
  act(() => fireEvent.changeText(screen.getByLabelText(label), text));

describe("session gate", () => {
  it("sends visitors from / to the login screen", async () => {
    await renderApp(fakeServer({}).api, "/");
    expect(await screen.findByRole("header", { name: tr("auth.login.title") })).toBeOnTheScreen();
    expect(screen).toHavePathname("/login");
    // T0.7.5: the brand and what the app is for sit above the form.
    expect(screen.getByRole("header", { name: APP_NAME })).toBeOnTheScreen();
    expect(screen.getByText(tr("common.tagline"))).toBeOnTheScreen();
  });

  it("sends signed-in users from /login home", async () => {
    const { api } = fakeServer({ "GET /me": () => json(200, user) });
    await renderApp(api, "/login", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByRole("header", { name: "Bugün" })).toBeOnTheScreen();
    expect(screen).toHavePathname("/");
  });

  it("blocks with a retry when the server is unreachable", async () => {
    const { api } = fakeServer({ "GET /me": () => apiError(500, "internal") });
    await renderApp(api, "/", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByText(tr("auth.session.unavailable"))).toBeOnTheScreen();
    expect(screen.getByRole("button", { name: tr("auth.session.retry") })).toBeOnTheScreen();
  });
});

describe("login", () => {
  it("validates before calling the server", async () => {
    const { api, calls } = fakeServer({});
    await renderApp(api, "/login");
    await screen.findByRole("header", { name: tr("auth.login.title") });
    await press(tr("auth.login.submit"));
    expect(screen.getAllByText(tr("auth.validation.required"))).toHaveLength(2);
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
    await screen.findByRole("header", { name: tr("auth.login.title") });
    await type(tr("auth.email"), " ali@example.com ");
    await type(tr("auth.password"), "correct horse");
    await press(tr("auth.login.submit"));
    expect(await screen.findByRole("header", { name: "Bugün" })).toBeOnTheScreen();
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
    await screen.findByRole("header", { name: tr("auth.login.title") });
    await type(tr("auth.email"), "ali@example.com");
    await type(tr("auth.password"), "wrong");
    await press(tr("auth.login.submit"));
    expect(await screen.findByText(tr("auth.errors.badCredentials"))).toBeOnTheScreen();
    limited = true;
    await press(tr("auth.login.submit"));
    expect(
      await screen.findByText(tr("auth.errors.rateLimitedSeconds", { count: 12 })),
    ).toBeOnTheScreen();
  });

  it("explains a failed provider sign-in from the redirect", async () => {
    await renderApp(fakeServer({}).api, "/login?error=google");
    expect(await screen.findByText(tr("auth.providers.failed.google"))).toBeOnTheScreen();
  });

  it("links to register and forgot password", async () => {
    await renderApp(fakeServer({}).api, "/login");
    await screen.findByRole("header", { name: tr("auth.login.title") });
    await press(`${tr("auth.login.noAccount")} ${tr("auth.login.toRegister")}`);
    expect(
      await screen.findByRole("header", { name: tr("auth.register.title") }),
    ).toBeOnTheScreen();
    await press(`${tr("auth.register.haveAccount")} ${tr("auth.register.toLogin")}`);
    await screen.findByRole("header", { name: tr("auth.login.title") });
    await press(tr("auth.login.forgot"));
    expect(screen).toHavePathname("/forgot-password");
  });
});

describe("register", () => {
  it("creates the account and signs in", async () => {
    const { api, calls } = fakeServer({
      "POST /auth/register": () => json(201, { user, token: "tok", expires_at: "x" }),
    });
    await renderApp(api, "/register");
    await screen.findByRole("header", { name: tr("auth.register.title") });
    await type(tr("auth.displayName"), " Ali ");
    await type(tr("auth.email"), "ali@example.com");
    await type(tr("auth.password"), "correct horse");
    await press(tr("auth.register.submit"));
    expect(await screen.findByRole("header", { name: "Bugün" })).toBeOnTheScreen();
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
    await screen.findByRole("header", { name: tr("auth.register.title") });
    await type(tr("auth.displayName"), "Ali");
    await type(tr("auth.email"), "ali@example.com");
    await type(tr("auth.password"), "correct horse");
    await press(tr("auth.register.submit"));
    expect(await screen.findByText(tr("auth.errors.emailTaken"))).toBeOnTheScreen();
    conflict = false;
    await press(tr("auth.register.submit"));
    expect(await screen.findByText(tr("auth.validation.passwordShort"))).toBeOnTheScreen();
    expect(screen.queryByText(tr("auth.errors.emailTaken"))).toBeNull();
  });
});

describe("forgot password", () => {
  it("always shows the same confirmation", async () => {
    const { api, calls } = fakeServer({
      "POST /auth/password/forgot": () => new Response(null, { status: 202 }),
    });
    await renderApp(api, "/forgot-password");
    await screen.findByRole("header", { name: tr("auth.forgot.title") });
    await type(tr("auth.email"), "nobody@example.com");
    await press(tr("auth.forgot.submit"));
    expect(await screen.findByText(tr("auth.forgot.sent"))).toBeOnTheScreen();
    expect(calls[0]?.body).toEqual({ email: "nobody@example.com" });
  });
});

describe("reset password", () => {
  it("explains a missing token", async () => {
    await renderApp(fakeServer({}).api, "/reset-password");
    expect(await screen.findByText(tr("auth.reset.missingToken"))).toBeOnTheScreen();
  });

  it("sets the new password, forgets the session and returns to login", async () => {
    const store = memoryTokenStore("old");
    const { api, calls } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/password/reset": () => new Response(null, { status: 204 }),
    });
    await renderApp(api, "/reset-password?token=abc", { tokenStore: store });
    await screen.findByRole("header", { name: tr("auth.reset.title") });
    await type(tr("auth.newPassword"), "new password");
    await type(tr("auth.confirmPassword"), "new passwort");
    await press(tr("auth.reset.submit"));
    expect(screen.getByText(tr("auth.validation.mismatch"))).toBeOnTheScreen();
    await type(tr("auth.confirmPassword"), "new password");
    await press(tr("auth.reset.submit"));
    expect(await screen.findByText(tr("auth.reset.done"))).toBeOnTheScreen();
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
    await screen.findByRole("header", { name: tr("auth.reset.title") });
    await type(tr("auth.newPassword"), "new password");
    await type(tr("auth.confirmPassword"), "new password");
    await press(tr("auth.reset.submit"));
    expect(await screen.findByText(tr("auth.reset.invalidToken"))).toBeOnTheScreen();
    expect(screen.getByRole("button", { name: tr("auth.reset.requestNew") })).toBeOnTheScreen();
  });
});

describe("settings", () => {
  it("shows the account and signs out back to the login screen", async () => {
    const { api } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/logout": () => new Response(null, { status: 204 }),
    });
    await renderApp(api, "/settings", { tokenStore: memoryTokenStore("t") });
    await screen.findByText("Ali");
    expect(screen.getByText("ali@example.com")).toBeOnTheScreen();
    await press(tr("auth.session.signOut"));
    expect(await screen.findByRole("header", { name: tr("auth.login.title") })).toBeOnTheScreen();
  });
});
