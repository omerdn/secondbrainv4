import { act, fireEvent, screen } from "expo-router/testing-library";

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
const noContent = () => new Response(null, { status: 204 });
const press = (name: string) => act(() => fireEvent.press(screen.getByRole("button", { name })));

beforeEach(resetCredentials);

describe("verify-email screen", () => {
  it("explains a missing token", async () => {
    await renderApp(fakeServer({}).api, "/verify-email");
    expect(await screen.findByText(tr("auth.verify.missingToken"))).toBeOnTheScreen();
    expect(screen.getByRole("button", { name: tr("auth.verify.toLogin") })).toBeOnTheScreen();
  });

  it("verifies once while signed out and offers sign-in", async () => {
    const verifies: Call[] = [];
    const { api } = fakeServer({
      "POST /auth/email/verify": (call) => {
        verifies.push(call);
        return noContent();
      },
    });
    await renderApp(api, "/verify-email?token=abc");
    expect(await screen.findByText(tr("auth.verify.done"))).toBeOnTheScreen();
    expect(verifies).toHaveLength(1);
    expect(verifies[0]?.body).toEqual({ token: "abc" });
    await press(tr("auth.verify.toLogin"));
    expect(screen).toHavePathname("/login");
  });

  it("explains an expired or used link", async () => {
    const { api } = fakeServer({
      "POST /auth/email/verify": () =>
        apiError(422, "validation_failed", [{ field: "token", code: "invalid", message: "" }]),
    });
    await renderApp(api, "/verify-email?token=used");
    expect(await screen.findByText(tr("auth.verify.invalidToken"))).toBeOnTheScreen();
  });

  it("offers a new link when a signed-in user's link failed", async () => {
    const { api } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/email/verify": () =>
        apiError(422, "validation_failed", [{ field: "token", code: "invalid", message: "" }]),
      "POST /auth/email/verification": noContent,
    });
    await renderApp(api, "/verify-email?token=old", { tokenStore: memoryTokenStore("t") });
    await screen.findByText(tr("auth.verify.invalidToken"));
    await press(tr("auth.verify.resend"));
    expect(await screen.findByText(tr("auth.verify.resent"))).toBeOnTheScreen();
  });

  it("marks the signed-in user verified, so the reminder disappears", async () => {
    const { api } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/email/verify": noContent,
    });
    await renderApp(api, "/verify-email?token=abc", { tokenStore: memoryTokenStore("t") });
    await screen.findByText(tr("auth.verify.done"));
    await press(tr("auth.verify.continue"));
    expect(await screen.findByRole("header", { name: "Bugün" })).toBeOnTheScreen();
    expect(screen.queryByTestId("verify-email-banner")).toBeNull();
  });
});

describe("verification reminder", () => {
  const banner = tr("auth.verify.banner", { email: user.email });

  it("is shown to unverified users and resends the link", async () => {
    const resends: Call[] = [];
    const { api } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/email/verification": (call) => {
        resends.push(call);
        return noContent();
      },
    });
    await renderApp(api, "/", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByText(banner)).toBeOnTheScreen();
    await press(tr("auth.verify.resend"));
    expect(await screen.findByText(tr("auth.verify.resent"))).toBeOnTheScreen();
    expect(resends).toHaveLength(1);
  });

  it("shows a rate limit", async () => {
    const { api } = fakeServer({
      "GET /me": () => json(200, user),
      "POST /auth/email/verification": () =>
        json(429, { error: { code: "rate_limited", message: "" } }, { "Retry-After": "60" }),
    });
    await renderApp(api, "/", { tokenStore: memoryTokenStore("t") });
    await screen.findByText(banner);
    await press(tr("auth.verify.resend"));
    expect(
      await screen.findByText(tr("auth.errors.rateLimitedSeconds", { count: 60 })),
    ).toBeOnTheScreen();
  });

  it("is not shown to verified users", async () => {
    const { api } = fakeServer({ "GET /me": () => json(200, { ...user, email_verified: true }) });
    await renderApp(api, "/", { tokenStore: memoryTokenStore("t") });
    await screen.findByRole("header", { name: "Bugün" });
    expect(screen.queryByTestId("verify-email-banner")).toBeNull();
  });
});
