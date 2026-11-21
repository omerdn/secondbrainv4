import { act, fireEvent, screen } from "expo-router/testing-library";

import { DEFAULT_LANGUAGE, i18n, pickLanguage, resolveLanguage } from "@/lib/i18n";
import { en } from "@/lib/i18n/locales/en";
import { tr } from "@/lib/i18n/locales/tr";

import {
  type Call,
  fakeServer,
  json,
  memoryTokenStore,
  resetCredentials,
  user,
} from "./support/api";
import { renderApp } from "./support/router";

beforeEach(resetCredentials);
afterEach(async () => {
  await act(() => i18n.changeLanguage("tr"));
});

describe("language choice", () => {
  it("reduces BCP 47 tags to a supported language", () => {
    expect(resolveLanguage("tr")).toBe("tr");
    expect(resolveLanguage("en-US")).toBe("en");
    expect(resolveLanguage("EN_gb")).toBe("en");
    expect(resolveLanguage("de-DE")).toBeNull();
    expect(resolveLanguage("")).toBeNull();
    expect(resolveLanguage(null)).toBeNull();
  });

  it("prefers the profile, then the device languages, then Turkish", () => {
    expect(pickLanguage({ userLocale: "en", deviceLocales: ["tr-TR"] })).toBe("en");
    expect(pickLanguage({ userLocale: "fr", deviceLocales: ["de-DE", "en-US"] })).toBe("en");
    expect(pickLanguage({ deviceLocales: ["de-DE"] })).toBe("tr");
    expect(pickLanguage({ userLocale: null, deviceLocales: [] })).toBe(DEFAULT_LANGUAGE);
    expect(DEFAULT_LANGUAGE).toBe("tr");
  });
});

/** `a.b.c` → value for every leaf. */
function flatten(value: object, prefix = ""): Map<string, string> {
  const out = new Map<string, string>();
  for (const [key, child] of Object.entries(value)) {
    const path = prefix === "" ? key : `${prefix}.${key}`;
    if (typeof child === "string") out.set(path, child);
    else for (const [k, v] of flatten(child as object, path)) out.set(k, v);
  }
  return out;
}

const placeholders = (text: string) =>
  [...text.matchAll(/\{\{(\w+)\}\}/gu)].map((m) => m[1]).sort();

describe("catalogs", () => {
  const trKeys = flatten(tr);
  const enKeys = flatten(en);

  it("have the same keys", () => {
    expect([...enKeys.keys()].sort()).toEqual([...trKeys.keys()].sort());
  });

  it("have no empty texts and the same placeholders", () => {
    for (const [key, text] of trKeys) {
      const other = enKeys.get(key) ?? "";
      expect(text.trim()).not.toBe("");
      expect(other.trim()).not.toBe("");
      expect([key, placeholders(other)]).toEqual([key, placeholders(text)]);
    }
  });

  it("pluralise English and interpolate", () => {
    const t = i18n.getFixedT("en");
    expect(t("auth.errors.rateLimitedSeconds", { count: 1 })).toContain("1 second.");
    expect(t("auth.errors.rateLimitedSeconds", { count: 30 })).toContain("30 seconds.");
    expect(i18n.getFixedT("tr")("auth.errors.rateLimitedSeconds", { count: 30 })).toContain(
      "30 saniye",
    );
  });
});

describe("UI language", () => {
  it("is Turkish by default", async () => {
    await renderApp(fakeServer({}).api, "/login");
    expect(await screen.findByRole("header", { name: "Giriş yap" })).toBeOnTheScreen();
  });

  it("follows an English device when signed out", async () => {
    await renderApp(fakeServer({}).api, "/login", { devices: () => ["en-US"] });
    expect(await screen.findByRole("header", { name: "Sign in" })).toBeOnTheScreen();
    expect(screen.getByRole("button", { name: "Forgot password" })).toBeOnTheScreen();
  });

  it("follows the signed-in user's profile locale", async () => {
    const { api } = fakeServer({ "GET /me": () => json(200, { ...user, locale: "en-US" }) });
    await renderApp(api, "/", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByRole("header", { name: "Today" })).toBeOnTheScreen();
    expect(screen.getByRole("tab", { name: "Tasks" })).toBeOnTheScreen();
  });

  it("is switched from settings, saved to the profile", async () => {
    const patches: Call[] = [];
    const { api } = fakeServer({
      "GET /me": () => json(200, user),
      "PATCH /me": (call) => {
        patches.push(call);
        return json(200, { ...user, locale: "en" });
      },
    });
    await renderApp(api, "/settings", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByRole("header", { name: "Ayarlar" })).toBeOnTheScreen();
    expect(screen.getByRole("radio", { name: "Türkçe" })).toBeChecked();

    await act(() => fireEvent.press(screen.getByRole("radio", { name: "English" })));
    expect(await screen.findByRole("header", { name: "Settings" })).toBeOnTheScreen();
    expect(screen.getByRole("radio", { name: "English" })).toBeChecked();
    expect(patches).toHaveLength(1);
    expect(patches[0]?.body).toEqual({ locale: "en" });
    expect(patches[0]?.headers["Idempotency-Key"]).toBe("key");
  });
});
