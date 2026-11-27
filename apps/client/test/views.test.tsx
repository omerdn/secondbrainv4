import type { Friend, Item, SavedView, Tag } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";
import { useWindowDimensions } from "react-native";

import {
  cleanFilter,
  isEmptyFilter,
  sameFilter,
  toggled,
  withLastDays,
} from "@/features/views/filterModel";
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

jest.mock("react-native/Libraries/Utilities/useWindowDimensions", () => ({
  __esModule: true,
  default: jest.fn(),
}));

const setWidth = (width: number) => {
  jest.mocked(useWindowDimensions).mockReturnValue({ width, height: 900, scale: 2, fontScale: 1 });
};

const at = "2026-05-01T12:00:00Z";

const tag: Tag = { id: "t1", name: "İş", color: null, created_at: at, updated_at: at };

const friend: Friend = { user_id: "f1", display_name: "Ayşe", since: at };

const item = (id: string, title: string, kind = "task"): Item => ({
  id,
  kind,
  title,
  description: null,
  owner: { type: "user", user_id: "u1" },
  created_by: "u1",
  created_at: at,
  updated_at: at,
  last_touched_at: at,
  archived_at: null,
  deleted_at: null,
});

const view = (over: Partial<SavedView> = {}): SavedView => ({
  id: "v1",
  name: "Hedeflerim",
  filter: { kinds: ["goal"] },
  pinned: true,
  position: 0,
  created_at: at,
  updated_at: at,
  ...over,
});

const filterOf = (call: Call): unknown =>
  JSON.parse(new URL(`http://x${call.path}`).searchParams.get("filter") ?? "null");

const filterCalls = (calls: Call[]) =>
  calls.filter((call) => call.path.startsWith("/items/filter")).map(filterOf);

const writes = (calls: Call[]) => calls.filter((call) => call.method !== "GET");

function server(views: SavedView[] = [], extra: Record<string, (call: Call) => Response> = {}) {
  let stored = views;
  return fakeServer({
    "GET /me": () => json(200, user),
    "GET /tags": () => json(200, [tag]),
    "GET /friends": () => json(200, [friend]),
    "GET /saved-views": () => json(200, stored),
    "GET /items/filter": (call) => {
      const filter = filterOf(call) as { kinds?: string[] };
      return json(200, {
        items: filter.kinds?.includes("goal")
          ? [item("g1", "Maraton", "goal")]
          : [item("i1", "Rapor yaz")],
        next_cursor: null,
      });
    },
    "POST /saved-views": (call) => {
      const created = view({ id: "new", ...(call.body as Partial<SavedView>) });
      stored = [...stored, created];
      return json(201, created);
    },
    "PATCH /saved-views/v1": (call) => {
      stored = stored.map((entry) => ({ ...entry, ...(call.body as Partial<SavedView>) }));
      return json(200, stored[0]);
    },
    "DELETE /saved-views/v1": () => {
      stored = [];
      return new Response(null, { status: 204 });
    },
    ...extra,
  });
}

const open = (s: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(s.api, url, { tokenStore: memoryTokenStore("t") });

beforeEach(async () => {
  resetCredentials();
  setWidth(390);
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("filter model", () => {
  it("toggles values", () => {
    expect(toggled(undefined, "a")).toEqual(["a"]);
    expect(toggled(["a", "b"], "a")).toEqual(["b"]);
  });

  it("canonicalizes: no empty rows, sorted values, default state dropped", () => {
    expect(
      cleanFilter({
        kinds: ["goal", "task"].reverse(),
        tag_ids: [],
        people: ["p"],
        team_ids: ["x"],
        statuses: ["done"],
        date: { field: "created" },
        state: "active",
      }),
    ).toEqual({
      kinds: ["goal", "task"],
      people: ["p"],
      team_ids: ["x"],
      statuses: ["done"],
      date: { field: "created" },
    });
    expect(cleanFilter({ state: "trash" })).toEqual({ state: "trash" });
  });

  it("chooses and clears a relative date range", () => {
    expect(withLastDays(undefined, 7)).toEqual({ field: "updated", last_days: 7 });
    expect(withLastDays({ field: "created", last_days: 7 }, 30)).toEqual({
      field: "created",
      last_days: 30,
    });
    expect(withLastDays({ field: "created", last_days: 7 }, 7)).toBeUndefined();
  });

  it("compares and detects empty filters", () => {
    expect(sameFilter({ kinds: ["a", "b"] }, { kinds: ["b", "a"], state: "active" })).toBe(true);
    expect(sameFilter({ kinds: ["a"] }, {})).toBe(false);
    expect(isEmptyFilter({ kinds: [], state: "active" })).toBe(true);
    expect(isEmptyFilter({ state: "archived" })).toBe(false);
  });
});

describe("filter bar", () => {
  it("sends each choice to the server", async () => {
    const s = server();
    await open(s, "/views");
    expect(await screen.findByRole("button", { name: "Görev: Rapor yaz" })).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Hedef" }));
    expect(await screen.findByRole("button", { name: "Hedef: Maraton" })).toBeTruthy();
    await fireEvent.press(await screen.findByRole("button", { name: "İş" }));
    await fireEvent.press(screen.getByRole("button", { name: "Arşiv" }));
    await waitFor(() => {
      expect(filterCalls(s.calls)).toContainEqual({
        kinds: ["goal"],
        tag_ids: ["t1"],
        state: "archived",
      });
    });
  });

  it("filters by status, a relative date and a friend (F1, F3)", async () => {
    const s = server();
    await open(s, "/views");
    await screen.findByRole("button", { name: "Görev: Rapor yaz" });
    await fireEvent.press(screen.getByRole("button", { name: "Tamamlandı" }));
    await fireEvent.press(screen.getByRole("button", { name: "Son 7 gün" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Oluşturma" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Ayşe" }));
    await waitFor(() => {
      expect(filterCalls(s.calls)).toContainEqual({
        people: ["f1"],
        date: { field: "created", last_days: 7 },
        statuses: ["done"],
      });
    });
    await fireEvent.press(screen.getByRole("button", { name: "Son 7 gün" }));
    expect(screen.queryByRole("button", { name: "Oluşturma" })).toBeNull();
    await waitFor(() => {
      expect(filterCalls(s.calls)).toContainEqual({ people: ["f1"], statuses: ["done"] });
    });
  });

  it("offers only friends as people (F3)", async () => {
    const s = server([], { "GET /friends": () => json(200, []) });
    await open(s, "/views");
    await screen.findByRole("button", { name: "Görev: Rapor yaz" });
    expect(screen.queryByText("Kişi")).toBeNull();
    expect(s.calls.some((call) => call.path === "/friends")).toBe(true);
  });

  it("pages through results", async () => {
    let page = 0;
    const s = server([], {
      "GET /items/filter": () => {
        page += 1;
        return json(
          200,
          page === 1
            ? { items: [item("i1", "Birinci")], next_cursor: "abc" }
            : { items: [item("i2", "İkinci")], next_cursor: null },
        );
      },
    });
    await open(s, "/views");
    await fireEvent.press(await screen.findByRole("button", { name: "Daha fazla yükle" }));
    expect(await screen.findByRole("button", { name: "Görev: İkinci" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Görev: Birinci" })).toBeTruthy();
    expect(s.calls.some((call) => call.path.includes("cursor=abc"))).toBe(true);
    expect(screen.queryByRole("button", { name: "Daha fazla yükle" })).toBeNull();
  });

  it("shows empty and failed results", async () => {
    let fail = false;
    const s = server([], {
      "GET /items/filter": () =>
        fail ? apiError(403, "forbidden") : json(200, { items: [], next_cursor: null }),
    });
    await open(s, "/views");
    expect(await screen.findByText("Bu filtreye uyan nesne yok")).toBeTruthy();
    fail = true;
    await fireEvent.press(screen.getByRole("button", { name: "Hedef" }));
    expect(await screen.findByText("Sonuçlar yüklenemedi")).toBeTruthy();
    fail = false;
    await fireEvent.press(screen.getByRole("button", { name: "Tekrar dene" }));
    expect(await screen.findByText("Bu filtreye uyan nesne yok")).toBeTruthy();
  });
});

describe("saved views (F2)", () => {
  const saveAs = async (name: string) => {
    await fireEvent.press(screen.getByRole("button", { name: "Görünüm olarak kaydet" }));
    await fireEvent.changeText(await screen.findByLabelText("Görünüm adı"), name);
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
  };

  it("needs a filter before it can be saved", async () => {
    await open(server(), "/views");
    await screen.findByRole("button", { name: "Görev: Rapor yaz" });
    expect(screen.getByRole("button", { name: "Görünüm olarak kaydet" })).toBeDisabled();
  });

  it("saves the current filter, pinned by default, and opens it", async () => {
    const s = server();
    await open(s, "/views");
    await fireEvent.press(await screen.findByRole("button", { name: "Hedef" }));
    await saveAs("  Koşu ");
    await waitFor(() => {
      expect(writes(s.calls)[0]?.body).toEqual({
        name: "Koşu",
        pinned: true,
        filter: { kinds: ["goal"] },
      });
    });
    expect(await screen.findByRole("header", { name: "Koşu" })).toBeTruthy();
  });

  it("keeps the relative date, status and person in a saved view", async () => {
    const s = server();
    await open(s, "/views");
    await fireEvent.press(await screen.findByRole("button", { name: "Açık" }));
    await fireEvent.press(screen.getByRole("button", { name: "Son 30 gün" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Ayşe" }));
    await saveAs("Bu ay");
    await waitFor(() => {
      expect(writes(s.calls)[0]?.body).toEqual({
        name: "Bu ay",
        pinned: true,
        filter: {
          people: ["f1"],
          date: { field: "updated", last_days: 30 },
          statuses: ["open"],
        },
      });
    });
  });

  it("explains a taken name", async () => {
    const s = server([], { "POST /saved-views": () => apiError(409, "conflict") });
    await open(s, "/views");
    await fireEvent.press(await screen.findByRole("button", { name: "Hedef" }));
    await saveAs("Aynı");
    expect(await screen.findByText("Bu adda bir görünümün var.")).toBeTruthy();
  });

  it("opens a view with its filter", async () => {
    const s = server([view()]);
    await open(s, "/views?view=v1");
    expect(await screen.findByRole("header", { name: "Hedeflerim" })).toBeTruthy();
    expect(await screen.findByRole("button", { name: "Hedef: Maraton" })).toBeTruthy();
    expect(filterCalls(s.calls)).not.toContainEqual({});
  });

  it("pins and unpins", async () => {
    const s = server([view()]);
    await open(s, "/views?view=v1");
    await fireEvent.press(await screen.findByRole("button", { name: "Sabitlemeyi kaldır" }));
    await waitFor(() => {
      expect(writes(s.calls)[0]?.body).toEqual({ pinned: false });
    });
    expect(await screen.findByRole("button", { name: "Kenar çubuğuna sabitle" })).toBeTruthy();
  });

  it("saves changes only when the filter differs", async () => {
    const s = server([view()]);
    await open(s, "/views?view=v1");
    await screen.findByRole("header", { name: "Hedeflerim" });
    expect(screen.queryByRole("button", { name: "Değişiklikleri kaydet" })).toBeNull();
    await fireEvent.press(await screen.findByRole("button", { name: "Görev" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Değişiklikleri kaydet" }));
    await waitFor(() => {
      expect(writes(s.calls)[0]?.body).toEqual({ filter: { kinds: ["goal", "task"] } });
    });
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Değişiklikleri kaydet" })).toBeNull();
    });
  });

  it("asks before deleting", async () => {
    const s = server([view()]);
    await open(s, "/views?view=v1");
    await fireEvent.press(await screen.findByRole("button", { name: "Sil" }));
    expect(writes(s.calls)).toEqual([]);
    await fireEvent.press(screen.getByRole("button", { name: "Vazgeç" }));
    await fireEvent.press(screen.getByRole("button", { name: "Sil" }));
    await fireEvent.press(screen.getByRole("button", { name: "Evet, sil" }));
    await waitFor(() => {
      expect(writes(s.calls).map((call) => call.method)).toEqual(["DELETE"]);
    });
    expect(await screen.findByRole("header", { name: "Görünümler" })).toBeTruthy();
  });

  it("reports a failed change", async () => {
    const s = server([view()], { "PATCH /saved-views/v1": () => apiError(403, "forbidden") });
    await open(s, "/views?view=v1");
    await fireEvent.press(await screen.findByRole("button", { name: "Sabitlemeyi kaldır" }));
    expect(await screen.findByText("İşlem yapılamadı. Tekrar dene.")).toBeTruthy();
  });

  it("lists only pinned views in the sidebar and opens them", async () => {
    setWidth(1200);
    const s = server([view(), view({ id: "v2", name: "Gizli kalsın", pinned: false })]);
    await open(s, "/");
    const link = await screen.findByRole("link", { name: "Hedeflerim" });
    expect(screen.queryByRole("link", { name: "Gizli kalsın" })).toBeNull();
    await fireEvent.press(link);
    expect(await screen.findByRole("header", { name: "Hedeflerim" })).toBeTruthy();
  });
});
