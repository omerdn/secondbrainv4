import type { Item, ItemLink, Tag } from "@secondbrain/api-types";
import { ApiProvider } from "@secondbrain/api-client/react";
import { ThemeProvider } from "@secondbrain/ui";
import { QueryClient } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react-native";
import { act } from "expo-router/testing-library";

import { i18n } from "@/lib/i18n";
import { ItemCard, ItemDetail, kindLabel, type Loadable, roleLabels } from "@/lib/items";

import {
  apiError,
  fakeServer,
  json,
  memoryTokenStore,
  resetCredentials,
  user,
} from "./support/api";
import { renderApp } from "./support/router";

const at = "2026-05-01T12:00:00Z";

const item = (over: Partial<Item> = {}): Item => ({
  id: "i1",
  kind: "task",
  title: "Plan the trip",
  description: null,
  owner: { type: "user", user_id: "u1" },
  created_by: "u1",
  created_at: at,
  updated_at: at,
  last_touched_at: at,
  archived_at: null,
  team_visibility: null,
  deleted_at: null,
  ...over,
});

const tag = (name: string): Tag => ({
  id: `t-${name}`,
  name,
  team_id: null,
  color: null,
  created_at: at,
  updated_at: at,
});

const link = (target: Item): ItemLink => ({
  id: `l-${target.id}`,
  direction: "outgoing",
  item: target,
  created_at: at,
});

const ready = <T,>(value: T): Loadable<T> => ({ status: "ready", value });

// Cards look up their status line (S3); the fake server has none.
const testApi = fakeServer({}).api;
const testQueries = new QueryClient({ defaultOptions: { queries: { retry: false } } });
const wrap = (ui: React.ReactElement) => (
  <ApiProvider api={testApi} queryClient={testQueries} idempotencyKey={() => "key"}>
    <ThemeProvider>{ui}</ThemeProvider>
  </ApiProvider>
);
const inTheme = (ui: React.ReactElement) => render(wrap(ui));

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("labels", () => {
  it("maps kinds, with every module under one label", () => {
    for (const kind of ["task", "routine", "goal", "tracker", "notebook", "page"]) {
      expect(kindLabel(kind)).toBe(kind);
    }
    expect(kindLabel("module:budget")).toBe("module");
  });

  it("lists roles once, keeping the server's order", () => {
    expect(
      roleLabels([
        { kind: "assignee" },
        { kind: "team_role", team_id: "x", role: "member" },
        { kind: "team_role", team_id: "y", role: "admin" },
        { kind: "linked", item_id: "a" },
        { kind: "linked", item_id: "b" },
      ]),
    ).toEqual(["assignee", "team", "linked"]);
  });
});

describe("ItemCard", () => {
  it("shows kind, title, description and tags", async () => {
    await inTheme(<ItemCard item={item({ description: "Book tickets" })} tags={[tag("İş")]} />);
    expect(screen.getByText("Görev")).toBeTruthy();
    expect(screen.getByText("Plan the trip")).toBeTruthy();
    expect(screen.getByText("Book tickets")).toBeTruthy();
    expect(screen.getByText("İş")).toBeTruthy();
  });

  it("is a labelled button only when it can be opened", async () => {
    const onPress = jest.fn();
    const { unmount } = await inTheme(<ItemCard item={item()} onPress={onPress} />);
    await fireEvent.press(screen.getByRole("button", { name: "Görev: Plan the trip" }));
    expect(onPress).toHaveBeenCalledTimes(1);
    await unmount();
    await inTheme(<ItemCard item={item()} />);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("marks archived and trashed items, the trash first", async () => {
    const { unmount } = await inTheme(<ItemCard item={item({ archived_at: at })} />);
    expect(screen.getByText("Arşivde")).toBeTruthy();
    await unmount();
    await inTheme(<ItemCard item={item({ archived_at: at, deleted_at: at })} />);
    expect(screen.getByText("Çöpte")).toBeTruthy();
    expect(screen.queryByText("Arşivde")).toBeNull();
  });
});

describe("ItemDetail", () => {
  const props = {
    item: item(),
    ownedByMe: true,
    roles: ready(["owner"] as const),
    tags: ready([] as Tag[]),
    links: ready([] as ItemLink[]),
    onOpenItem: () => undefined,
  };

  it("names the owner", async () => {
    const { rerender } = await inTheme(<ItemDetail {...props} />);
    expect(screen.getByText("Sen")).toBeTruthy();
    await rerender(wrap(<ItemDetail {...props} ownedByMe={false} />));
    expect(screen.getByText("Başka bir kişi")).toBeTruthy();
    await rerender(
      wrap(
        <ItemDetail
          {...props}
          ownedByMe={false}
          item={item({ owner: { type: "team", team_id: "x" } })}
        />,
      ),
    );
    expect(screen.getByText("Takım")).toBeTruthy();
  });

  it("shows empty sections", async () => {
    await inTheme(<ItemDetail {...props} roles={ready([])} />);
    expect(screen.getByText("Rol yok")).toBeTruthy();
    expect(screen.getByText("Etiket yok")).toBeTruthy();
    expect(screen.getByText("Bağlantı yok")).toBeTruthy();
  });

  it("shows loading, and a retry for one failed section", async () => {
    const retry = jest.fn();
    await inTheme(
      <ItemDetail
        {...props}
        roles={{ status: "loading" }}
        tags={{ status: "error", retry }}
        links={ready([link(item({ id: "i2", title: "Linked" }))])}
      />,
    );
    expect(screen.getByText("Yükleniyor")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Tekrar dene" }));
    expect(retry).toHaveBeenCalledTimes(1);
    // The other sections keep working.
    expect(screen.getByText("Linked")).toBeTruthy();
  });

  it("opens a linked item", async () => {
    const onOpenItem = jest.fn();
    await inTheme(
      <ItemDetail
        {...props}
        onOpenItem={onOpenItem}
        links={ready([link(item({ id: "i2", kind: "goal", title: "Linked" }))])}
      />,
    );
    await fireEvent.press(screen.getByRole("button", { name: "Hedef: Linked" }));
    expect(onOpenItem).toHaveBeenCalledWith("i2");
  });
});

describe("item detail route", () => {
  const routes = {
    "GET /me": () => json(200, user),
    "GET /items/i1": () => json(200, item({ description: "Book tickets" })),
    "GET /items/i2": () => json(200, item({ id: "i2", kind: "goal", title: "Linked goal" })),
    "GET /access/items/i1/explanation": () =>
      json(200, { permissions: ["view"], reasons: [{ kind: "owner" }] }),
    "GET /access/items/i2/explanation": () =>
      json(200, { permissions: ["view"], reasons: [{ kind: "watcher" }] }),
    "GET /items/i1/tags": () => json(200, [tag("İş")]),
    "GET /items/i2/tags": () => json(200, []),
    "GET /items/i1/links": () =>
      json(200, [link(item({ id: "i2", kind: "goal", title: "Linked goal" }))]),
    "GET /items/i2/links": () => json(200, []),
  };
  const open = (server: ReturnType<typeof fakeServer>, url: string) =>
    renderApp(server.api, url, { tokenStore: memoryTokenStore("t") });

  it("loads the item with its roles, tags and links, and follows a link", async () => {
    await open(fakeServer(routes), "/items/i1");
    expect(await screen.findByRole("header", { name: "Plan the trip" })).toBeTruthy();
    expect(screen.getByText("Book tickets")).toBeTruthy();
    expect(await screen.findByText("Sahip")).toBeTruthy();
    expect(await screen.findByText("İş")).toBeTruthy();
    await fireEvent.press(await screen.findByRole("button", { name: "Hedef: Linked goal" }));
    expect(await screen.findByRole("header", { name: "Linked goal" })).toBeTruthy();
    expect(await screen.findByText("İzleyici")).toBeTruthy();
  });

  it("treats a hidden item like one that does not exist, and asks for nothing else", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /items/secret": () => apiError(404, "not_found"),
    });
    await open(server, "/items/secret");
    expect(await screen.findByRole("header", { name: "Nesne bulunamadı" })).toBeTruthy();
    expect(server.calls.some((call) => call.path.includes("/secret/"))).toBe(false);
  });

  it("offers a retry when loading fails", async () => {
    let fail = true;
    const server = fakeServer({
      ...routes,
      "GET /items/i1": () =>
        fail ? apiError(403, "forbidden") : json(200, item({ title: "Back again" })),
    });
    await open(server, "/items/i1");
    const retry = await screen.findByRole("button", { name: "Tekrar dene" });
    fail = false;
    await fireEvent.press(retry);
    expect(await screen.findByRole("header", { name: "Back again" })).toBeTruthy();
  });
});
