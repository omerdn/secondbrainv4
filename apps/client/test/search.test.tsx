import type { Item, SearchHit } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";
import { useWindowDimensions } from "react-native";

import { isSearchShortcut } from "@/features/search/shortcut";
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

const at = "2026-11-27T09:00:00Z";
const item = (id: string, kind: string, title: string, archived = false): Item => ({
  id,
  kind,
  title,
  description: null,
  owner: { type: "user", user_id: "u1" },
  created_by: "u1",
  created_at: at,
  updated_at: at,
  last_touched_at: at,
  archived_at: archived ? at : null,
  deleted_at: null,
});

const hits: SearchHit[] = [
  { item: item("t1", "task", "Gitar kursu"), field: "title", snippet: null },
  { item: item("p1", "page", "Notlar"), field: "content", snippet: "yeni bir gitar teli taktım" },
  { item: item("r1", "routine", "Akor çalış", true), field: "tag", snippet: "gitarist" },
];

const searchServer = () =>
  fakeServer({
    "GET /me": () => json(200, user),
    "GET /today": () => json(404, { error: { code: "not_found", message: "not_found" } }),
    "GET /search": (call) => {
      const q = new URL(`http://x${call.path}`).searchParams.get("q") ?? "";
      return json(200, q === "gitar" ? hits : []);
    },
    "GET /items/t1": () => json(200, item("t1", "task", "Gitar kursu")),
  });

beforeEach(async () => {
  resetCredentials();
  setWidth(1280);
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("search shortcut", () => {
  const event = (
    key: string,
    mods: Partial<{ meta: boolean; ctrl: boolean; alt: boolean }> = {},
    typing = false,
  ) => ({
    key,
    metaKey: mods.meta ?? false,
    ctrlKey: mods.ctrl ?? false,
    altKey: mods.alt ?? false,
    typing,
  });

  it("opens on ⌘K, Ctrl+K and / outside text fields", () => {
    expect(isSearchShortcut(event("k", { meta: true }))).toBe(true);
    expect(isSearchShortcut(event("K", { ctrl: true }, true))).toBe(true);
    expect(isSearchShortcut(event("/"))).toBe(true);
    expect(isSearchShortcut(event("/", {}, true))).toBe(false);
    expect(isSearchShortcut(event("k"))).toBe(false);
    expect(isSearchShortcut(event("k", { ctrl: true, alt: true }))).toBe(false);
  });
});

describe("global search (AR1)", () => {
  it("searches as you type and opens a result", async () => {
    const server = searchServer();
    await renderApp(server.api, "/", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(await screen.findByRole("button", { name: "Ara" }));
    expect(screen.getByText("Başlık, açıklama, etiket ve sayfa içeriklerinde arar.")).toBeTruthy();
    await fireEvent.changeText(screen.getByLabelText("Ne arıyorsun?"), "gitar");
    expect(await screen.findByRole("link", { name: "Görev: Gitar kursu" })).toBeTruthy();
    expect(screen.getByText("yeni bir gitar teli taktım")).toBeTruthy();
    expect(screen.getByText("Rutin · Etiket: gitarist · Arşivde")).toBeTruthy();
    // Typing pauses before a request: one search for the whole word.
    expect(server.calls.filter((c) => c.path.startsWith("/search"))).toHaveLength(1);
    await fireEvent.press(screen.getByRole("link", { name: "Görev: Gitar kursu" }));
    await waitFor(() => {
      expect(server.calls.some((c) => c.path.startsWith("/tasks/t1"))).toBe(true);
    });
  });

  it("says when nothing matches", async () => {
    const server = searchServer();
    await renderApp(server.api, "/", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(await screen.findByRole("button", { name: "Ara" }));
    await fireEvent.changeText(screen.getByLabelText("Ne arıyorsun?"), "zümrüt");
    expect(await screen.findByText('"zümrüt" için sonuç yok.')).toBeTruthy();
  });

  it("has a touch entry on phones", async () => {
    setWidth(390);
    const server = searchServer();
    await renderApp(server.api, "/", { tokenStore: memoryTokenStore("t") });
    expect(screen.queryByTestId("sidebar")).toBeNull();
    await fireEvent.press(await screen.findByRole("button", { name: "Ara" }));
    expect(screen.getByLabelText("Ne arıyorsun?")).toBeTruthy();
  });
});
