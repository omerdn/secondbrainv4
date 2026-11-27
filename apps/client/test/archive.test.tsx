import type { Item } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import { trashDaysLeft } from "@/features/archive";
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

const at = "2026-05-01T12:00:00Z";

const item = (id: string, title: string, over: Partial<Item> = {}): Item => ({
  id,
  kind: "task",
  title,
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

const daysAgo = (days: number) => new Date(Date.now() - days * 86_400_000).toISOString();

const open = (server: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(server.api, url, { tokenStore: memoryTokenStore("t") });

const writes = (calls: Call[]) =>
  calls.filter((call) => call.method !== "GET").map((call) => `${call.method} ${call.path}`);

function server(
  state: "archived" | "trash",
  initial: Item[],
  extra: Record<string, () => Response> = {},
) {
  let stored = initial;
  const back = () => {
    const restored = stored[0];
    stored = stored.slice(1);
    return json(200, { ...restored, archived_at: null, deleted_at: null });
  };
  return fakeServer({
    "GET /me": () => json(200, user),
    "GET /items": (call) =>
      call.path.includes(`state=${state}`) ? json(200, stored) : apiError(400, "bad_request"),
    "POST /items/i1/restore": back,
    "POST /items/i1/unarchive": back,
    ...extra,
  });
}

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("days left in the trash (N4)", () => {
  const trashed = new Date("2026-05-01T12:00:00Z");
  const after = (ms: number) => new Date(trashed.getTime() + ms);
  const day = 86_400_000;

  it("counts down from thirty, rounding up", () => {
    expect(trashDaysLeft(trashed.toISOString(), trashed)).toBe(30);
    expect(trashDaysLeft(trashed.toISOString(), after(1))).toBe(30);
    expect(trashDaysLeft(trashed.toISOString(), after(day))).toBe(29);
    expect(trashDaysLeft(trashed.toISOString(), after(29 * day + 1))).toBe(1);
  });

  it("never goes below zero", () => {
    expect(trashDaysLeft(trashed.toISOString(), after(30 * day))).toBe(0);
    expect(trashDaysLeft(trashed.toISOString(), after(40 * day))).toBe(0);
  });
});

describe("trash screen", () => {
  it("lists items with the days left and restores one", async () => {
    const s = server("trash", [
      item("i1", "Eski görev", { deleted_at: daysAgo(10) }),
      item("i2", "Diğer görev", { deleted_at: daysAgo(29.5) }),
    ]);
    await open(s, "/trash");
    expect(await screen.findByRole("button", { name: "Görev: Eski görev" })).toBeTruthy();
    expect(screen.getByText("20 gün kaldı")).toBeTruthy();
    expect(screen.getByText("1 gün kaldı")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Geri yükle: Eski görev" }));
    await waitFor(() => {
      expect(writes(s.calls)).toEqual(["POST /items/i1/restore"]);
    });
    await waitFor(() => {
      expect(screen.queryByText("Eski görev")).toBeNull();
    });
    expect(screen.getByText("Diğer görev")).toBeTruthy();
  });

  it("explains an empty trash", async () => {
    await open(server("trash", []), "/trash");
    expect(await screen.findByText("Çöp kutusu boş")).toBeTruthy();
  });

  it("reports a failed restore on that item and keeps it", async () => {
    const s = server("trash", [item("i1", "Eski görev", { deleted_at: daysAgo(1) })], {
      "POST /items/i1/restore": () => apiError(500, "internal"),
    });
    await open(s, "/trash");
    await fireEvent.press(await screen.findByRole("button", { name: "Geri yükle: Eski görev" }));
    expect(await screen.findByText("İşlem yapılamadı. Tekrar dene.")).toBeTruthy();
    expect(screen.getByText("Eski görev")).toBeTruthy();
  });
});

describe("archive screen", () => {
  it("lists items and takes one out of the archive", async () => {
    const s = server("archived", [item("i1", "Arşivli görev", { archived_at: at })]);
    await open(s, "/archive");
    await fireEvent.press(
      await screen.findByRole("button", { name: "Arşivden çıkar: Arşivli görev" }),
    );
    await waitFor(() => {
      expect(writes(s.calls)).toEqual(["POST /items/i1/unarchive"]);
    });
    expect(await screen.findByText("Arşiv boş")).toBeTruthy();
  });

  it("says when the viewer may not do it", async () => {
    const s = server("archived", [item("i1", "Ortak görev", { archived_at: at })], {
      "POST /items/i1/unarchive": () => apiError(403, "forbidden"),
    });
    await open(s, "/archive");
    await fireEvent.press(
      await screen.findByRole("button", { name: "Arşivden çıkar: Ortak görev" }),
    );
    expect(await screen.findByText("Bunu yapma yetkin yok.")).toBeTruthy();
  });

  it("offers a retry when the list fails", async () => {
    let fail = true;
    const s = fakeServer({
      "GET /me": () => json(200, user),
      "GET /items": () =>
        fail ? apiError(403, "forbidden") : json(200, [item("i1", "Geri geldi")]),
    });
    await open(s, "/archive");
    const retry = await screen.findByRole("button", { name: "Tekrar dene" });
    fail = false;
    await fireEvent.press(retry);
    expect(await screen.findByText("Geri geldi")).toBeTruthy();
  });

  it("opens an item", async () => {
    const s = server("archived", [item("i1", "Arşivli görev", { archived_at: at })], {
      "GET /items/i1": () => json(200, item("i1", "Arşivli görev", { archived_at: at })),
    });
    await open(s, "/archive");
    await fireEvent.press(await screen.findByRole("button", { name: "Görev: Arşivli görev" }));
    expect(await screen.findByRole("header", { name: "Arşivli görev" })).toBeTruthy();
  });
});
