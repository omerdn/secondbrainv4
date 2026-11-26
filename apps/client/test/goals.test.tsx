import type { Goal, GoalProgress, Item } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import {
  emptyGoalForm,
  goalErrors,
  goalRequest,
  linkCandidates,
  normalizeWeight,
  paceTone,
  percent,
  weightShares,
} from "@/features/goals/model";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const at = "2026-11-26T12:00:00Z";

const item = (id: string, kind: string, title: string): Item => ({
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

const goalItem = item("g1", "goal", "Kitap yılı");
const task = item("t1", "task", "Rapor yaz");
const routine = item("r1", "routine", "Her gün oku");
const page = item("p1", "page", "Notlar");

const goal = (links: Goal["links"] = []): Goal => ({
  item: goalItem,
  start_date: "2026-12-01",
  end_date: "2026-12-31",
  links,
});

const link = (target: Item, weight = "1") => ({
  item: target,
  weight,
  contributes: target.kind !== "page",
});

const noProgress: GoalProgress = {
  today: "2026-11-26",
  actual: 0,
  expected: 0,
  pace: 1,
  items: [],
};

const open = (server: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(server.api, url, { tokenStore: memoryTokenStore("t") });

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("goal model (D1-D3)", () => {
  it("requires both dates in order (D1)", () => {
    expect(goalErrors(emptyGoalForm)).toEqual({
      title: "required",
      startDate: "required",
      endDate: "required",
    });
    const filled = { ...emptyGoalForm, title: "Kitap" };
    expect(goalErrors({ ...filled, startDate: "2026-12-01", endDate: "2026-13-01" })).toEqual({
      endDate: "invalidDay",
    });
    expect(goalErrors({ ...filled, startDate: "2026-12-02", endDate: "2026-12-01" })).toEqual({
      endDate: "endBeforeStart",
    });
    expect(goalRequest({ ...filled, startDate: "2026-12-01" })).toBeNull();
    expect(
      goalRequest({ ...filled, startDate: " 2026-12-01", endDate: "2026-12-01", description: " " }),
    ).toEqual({ title: "Kitap", start_date: "2026-12-01", end_date: "2026-12-01" });
  });

  it("reads weights as the server does (D3)", () => {
    expect(normalizeWeight("2,5")).toBe("2.5");
    expect(normalizeWeight(" 100 ")).toBe("100");
    expect(normalizeWeight("1.50")).toBe("1.5");
    for (const bad of ["0", "-1", "100.01", "1.005", "abc", ""]) {
      expect(normalizeWeight(bad)).toBeNull();
    }
  });

  it("offers only items that can still be linked (D2)", () => {
    const goalLike = item("g2", "goal", "Başka hedef");
    const trashed = { ...item("t2", "task", "Silinen"), deleted_at: at };
    const all = [task, routine, page, goalLike, trashed];
    expect(linkCandidates(all, [link(task)], "").map((i) => i.id)).toEqual(["r1", "p1"]);
    expect(linkCandidates(all, [], "OKU").map((i) => i.id)).toEqual(["r1"]);
  });

  it("shares weights only among the items that count", () => {
    const shares = weightShares([link(task, "1"), link(routine, "3"), link(page)]);
    expect(shares.get("t1")).toBe(0.25);
    expect(shares.get("r1")).toBe(0.75);
    expect(shares.has("p1")).toBe(false);
  });
});

describe("goal progress display (D4, D6)", () => {
  it("rounds shares to percentages and reads the pace", () => {
    expect(percent(0.426)).toBe(43);
    expect(percent(-1)).toBe(0);
    expect(percent(1.5)).toBe(150);
    expect(paceTone(1)).toBe("ahead");
    expect(paceTone(1.4)).toBe("ahead");
    expect(paceTone(0.85)).toBe("onTrack");
    expect(paceTone(0.5)).toBe("behind");
  });

  it("shows both indicators together and each item's contribution", async () => {
    const progress: GoalProgress = {
      today: "2026-11-26",
      actual: 0.3,
      expected: 0.4,
      pace: 0.75,
      items: [
        {
          item: routine,
          weight: "3",
          done: 6,
          total: 15,
          expected_to_date: 8,
          actual: 0.4,
          expected: 8 / 15,
          share: 0.75,
          contribution: 0.3,
          not_started: false,
        },
        {
          item: task,
          weight: "1",
          done: 0,
          total: 0,
          expected_to_date: 0,
          actual: 0,
          expected: 0,
          share: 0,
          contribution: 0,
          not_started: true,
        },
      ],
    };
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /goals/g1": () => json(200, goal([link(routine, "3"), link(task)])),
      "GET /goals/g1/progress": () => json(200, progress),
    });
    await open(server, "/goals/g1");
    expect(await screen.findByText("Gerçekleşen: %30")).toBeTruthy();
    expect(screen.getByText("Tempo: plana göre %75")).toBeTruthy();
    expect(screen.getByText("Plandan geridesin · Bugüne kadar beklenen: %40")).toBeTruthy();
    expect(screen.getByText("Gerçekleşen %40 · beklenen %53")).toBeTruthy();
    expect(screen.getByText("6 / 15 · beklenen 8")).toBeTruthy();
    expect(screen.getByText("Hedefe katkısı: %30 (payı %75)")).toBeTruthy();
    expect(screen.getByText("Henüz başlamadı; hesaba katılmıyor.")).toBeTruthy();
    // The caller's own day goes with the request.
    const call = server.calls.find((c) => c.path.startsWith("/goals/g1/progress"));
    expect(call?.path).toMatch(/today=\d{4}-\d{2}-\d{2}/);
  });

  it("offers a retry when progress cannot load", async () => {
    await open(
      fakeServer({
        "GET /me": () => json(200, user),
        "GET /goals/g1": () => json(200, goal()),
        "GET /goals/g1/progress": () => json(500, { error: { code: "internal", message: "x" } }),
      }),
      "/goals/g1",
    );
    expect(await screen.findByText("İlerleme yüklenemedi")).toBeTruthy();
  });
});

describe("goals screens (D1-D3)", () => {
  it("validates the dates before creating, then opens the goal", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
      "POST /goals": () => json(201, goal()),
      "GET /goals/g1": () => json(200, goal()),
    });
    await open(server, "/goals");
    expect(await screen.findByText("Henüz hedefin yok")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Yeni hedef" }));
    await fireEvent.changeText(screen.getByLabelText("Ad"), "Kitap yılı");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(screen.getAllByText("Bu alan gerekli.")).toHaveLength(2);
    expect(server.calls.some((call) => call.method === "POST")).toBe(false);
    await fireEvent.changeText(screen.getByLabelText("Başlangıç (YYYY-AA-GG)"), "2026-12-01");
    await fireEvent.changeText(screen.getByLabelText("Bitiş (YYYY-AA-GG)"), "2026-12-31");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(await screen.findByRole("header", { name: "Kitap yılı" })).toBeTruthy();
    expect(server.calls.find((call) => call.method === "POST")?.body).toEqual({
      title: "Kitap yılı",
      start_date: "2026-12-01",
      end_date: "2026-12-31",
    });
  });

  it("links an item, changes its weight and removes it", async () => {
    let current = goal([link(page)]);
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /goals/g1": () => json(200, current),
      "GET /goals/g1/progress": () => json(200, noProgress),
      "GET /items/filter": () => json(200, { items: [task, routine, page], next_cursor: null }),
      "PUT /goals/g1/links/t1": (call) => {
        const weight = (call.body as { weight?: string }).weight ?? "1";
        current = goal([link(page), link(task, weight)]);
        return json(200, current);
      },
      "DELETE /goals/g1/links/t1": () => {
        current = goal([link(page)]);
        return json(200, current);
      },
    });
    await open(server, "/goals/g1");
    expect(await screen.findByRole("header", { name: "Kitap yılı" })).toBeTruthy();
    expect(screen.getByText("Sayfalar ilerlemeye etki etmez.")).toBeTruthy();
    // A page takes no weight (D2).
    expect(screen.queryByLabelText("Notlar ağırlığı")).toBeNull();

    await fireEvent.press(screen.getByRole("button", { name: "Nesne bağla" }));
    // Already linked items are not offered again.
    expect(await screen.findByRole("button", { name: "Görev: Rapor yaz" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Sayfa: Notlar" })).toBeNull();
    await fireEvent.press(screen.getByRole("button", { name: "Görev: Rapor yaz" }));
    expect(await screen.findByLabelText("Rapor yaz ağırlığı")).toBeTruthy();
    expect(screen.getByText("Payı: %100")).toBeTruthy();

    await fireEvent.changeText(screen.getByLabelText("Rapor yaz ağırlığı"), "0");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
    expect(
      screen.getByText("0'dan büyük, en fazla 100 ve en fazla 2 ondalık olmalı."),
    ).toBeTruthy();
    await fireEvent.changeText(screen.getByLabelText("Rapor yaz ağırlığı"), "2,5");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
    await waitFor(() => {
      expect(server.calls.filter((call) => call.method === "PUT").map((call) => call.body)).toEqual(
        [{}, { weight: "2.5" }],
      );
    });

    await fireEvent.press(screen.getByRole("button", { name: "Rapor yaz bağlantısını kaldır" }));
    await waitFor(() => {
      expect(screen.queryByLabelText("Rapor yaz ağırlığı")).toBeNull();
    });
    expect(server.calls.some((call) => call.method === "DELETE")).toBe(true);
  });

  it("does not reveal a goal it cannot see", async () => {
    await open(
      fakeServer({
        "GET /me": () => json(200, user),
        "GET /goals/g1": () => json(404, { error: { code: "not_found", message: "not found" } }),
      }),
      "/goals/g1",
    );
    expect(await screen.findByText(i18n.t("items.detail.notFound"))).toBeTruthy();
  });
});
