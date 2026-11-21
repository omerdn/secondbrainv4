import { renderRouter, screen } from "expo-router/testing-library";

import { platformLabel } from "@/lib/app-info";
import Home from "../app/index";
import NotFound from "../app/+not-found";

describe("routes", () => {
  it("renders the home screen at /", async () => {
    await renderRouter({ index: Home, "+not-found": NotFound }, { initialUrl: "/" });
    expect(await screen.findByRole("header")).toHaveTextContent("Second Brain");
    expect(screen).toHavePathname("/");
  });

  it("renders the not-found screen for unknown paths", async () => {
    await renderRouter({ index: Home, "+not-found": NotFound }, { initialUrl: "/nope" });
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
