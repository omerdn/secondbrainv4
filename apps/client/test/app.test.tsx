import { renderRouter, screen } from "expo-router/testing-library";

import { platformLabel } from "@/lib/app-info";
import NotFound from "../app/+not-found";
import { fakeServer, json, memoryTokenStore, user } from "./support/api";
import { renderApp } from "./support/router";

describe("routes", () => {
  it("renders the home screen at / when signed in", async () => {
    const { api } = fakeServer({ "GET /me": () => json(200, user) });
    await renderApp(api, "/", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByRole("header", { name: "Bugün" })).toBeOnTheScreen();
    expect(screen).toHavePathname("/");
  });

  it("renders the not-found screen for unknown paths", async () => {
    await renderRouter({ "+not-found": NotFound }, { initialUrl: "/nope" });
    expect(await screen.findByText("Page not found")).toBeOnTheScreen();
  });
});

describe("platformLabel", () => {
  it("names each platform", () => {
    expect(platformLabel("ios")).toBe("iOS");
    expect(platformLabel("android")).toBe("Android");
    expect(platformLabel("web")).toBe("Web");
    expect(platformLabel("windows")).toBe("windows");
  });
});
