import { existsSync } from "node:fs";
import { join } from "node:path";

import { act, fireEvent, screen } from "expo-router/testing-library";
import { useWindowDimensions } from "react-native";

import { activeTab, destinations, isActive, moreItems, settings, tabs } from "@/features/shell";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

jest.mock("react-native/Libraries/Utilities/useWindowDimensions", () => ({
  __esModule: true,
  default: jest.fn(),
}));

const setWidth = (width: number) => {
  jest.mocked(useWindowDimensions).mockReturnValue({ width, height: 900, scale: 2, fontScale: 1 });
};

beforeEach(() => {
  resetCredentials();
  setWidth(390);
});

const signedIn = (url: string) =>
  renderApp(fakeServer({ "GET /me": () => json(200, user) }).api, url, {
    tokenStore: memoryTokenStore("t"),
  });

const header = (name: string) => screen.findByRole("header", { name });

describe("navigation model", () => {
  it("matches routes by prefix, with / matching only itself", () => {
    expect(isActive("/", "/")).toBe(true);
    expect(isActive("/tasks", "/")).toBe(false);
    expect(isActive("/tasks/42", "/tasks")).toBe(true);
    expect(isActive("/tasksx", "/tasks")).toBe(false);
  });

  it("has at most five bottom tabs, the rest under More", () => {
    expect(tabs.length).toBeLessThanOrEqual(5);
    expect(tabs.at(-1)?.key).toBe("more");
    const tabbed = new Set(tabs.map((t) => t.key));
    for (const d of destinations) {
      expect(tabbed.has(d.key) !== moreItems.some((m) => m.key === d.key)).toBe(true);
    }
    expect(moreItems.at(-1)).toBe(settings);
  });

  it("highlights More for destinations without a tab", () => {
    expect(activeTab("/")).toBe("today");
    expect(activeTab("/tasks/1")).toBe("tasks");
    expect(activeTab("/goals")).toBe("more");
    expect(activeTab("/settings")).toBe("more");
    expect(activeTab("/more")).toBe("more");
    expect(activeTab("/elsewhere")).toBeNull();
  });

  it("has a route file for every destination", () => {
    for (const d of [...destinations, settings]) {
      const file = d.href === "/" ? "index" : d.href.slice(1);
      expect(existsSync(join(__dirname, "..", "app", "(app)", `${file}.tsx`))).toBe(true);
    }
  });
});

describe("phone layout (< md)", () => {
  it("shows bottom tabs and no sidebar", async () => {
    await signedIn("/");
    await header("Bugün");
    expect(screen.getByTestId("tab-bar")).toBeOnTheScreen();
    expect(screen.getAllByRole("tab")).toHaveLength(tabs.length);
    expect(screen.queryByTestId("sidebar")).toBeNull();
    expect(screen.getByRole("tab", { name: "Bugün" })).toBeSelected();
  });

  it("switches sections from the tab bar", async () => {
    await signedIn("/");
    await header("Bugün");
    await act(() => fireEvent.press(screen.getByRole("tab", { name: "Görevler" })));
    expect(await header("Görevler")).toBeOnTheScreen();
    expect(screen).toHavePathname("/tasks");
    expect(screen.getByRole("tab", { name: "Görevler" })).toBeSelected();
  });

  it("reaches the other sections through More", async () => {
    await signedIn("/");
    await header("Bugün");
    await act(() => fireEvent.press(screen.getByRole("tab", { name: "Daha fazla" })));
    expect(await header("Daha fazla")).toBeOnTheScreen();
    await act(() => fireEvent.press(screen.getByRole("link", { name: "Hedefler" })));
    expect(await header("Hedefler")).toBeOnTheScreen();
    expect(screen.getByRole("tab", { name: "Daha fazla" })).toBeSelected();
  });
});

describe("wide layout (≥ md)", () => {
  it("shows the sidebar with every section, the account and settings", async () => {
    setWidth(1280);
    await signedIn("/");
    await header("Bugün");
    expect(screen.getByTestId("sidebar")).toBeOnTheScreen();
    expect(screen.queryByTestId("tab-bar")).toBeNull();
    for (const d of [...destinations, settings]) {
      expect(screen.getByRole("link", { name: i18n.t(`nav.${d.key}`) })).toBeOnTheScreen();
    }
    expect(screen.getByText("Ali")).toBeOnTheScreen();
    expect(screen.getByRole("link", { name: "Bugün" })).toBeSelected();
  });

  it("navigates from the sidebar", async () => {
    setWidth(800);
    await signedIn("/");
    await header("Bugün");
    await act(() => fireEvent.press(screen.getByRole("link", { name: "Takvim" })));
    expect(await header("Takvim")).toBeOnTheScreen();
    expect(screen.getByRole("link", { name: "Takvim" })).toBeSelected();
    expect(screen.getByRole("link", { name: "Bugün" })).not.toBeSelected();
  });
});
