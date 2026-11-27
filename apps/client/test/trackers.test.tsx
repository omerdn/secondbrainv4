import type { Item, Tracker, TrackerEntry, TrackerSummary } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import {
  addDays,
  barShare,
  dailySeries,
  emptyForm,
  formatAmount,
  isDay,
  normalizeDecimal,
  progressShare,
  sideBySide,
  validateForm,
} from "@/features/trackers/model";
import { i18n } from "@/lib/i18n";

import {
  apiError,
  fakeServer,
  json,
  memoryTokenStore,
  resetCredentials,
  user,
} from "./support/api";
import { renderApp } from "./support/router";

const at = "2026-11-23T12:00:00Z";

const item: Item = {
  id: "t1",
  kind: "tracker",
  title: "Bütçe",
  description: null,
  owner: { type: "user", user_id: "u1" },
  created_by: "u1",
  created_at: at,
  updated_at: at,
  last_touched_at: at,
  archived_at: null,
  deleted_at: null,
};

const tracker: Tracker = {
  item,
  unit: "TL",
  target_value: "100",
  period: { type: "month" },
  aggregation: "sum",
  direction: "limit",
  owner_kind: "standalone",
  created_at: at,
  updated_at: at,
};

const summary: TrackerSummary = {
  today: "2026-11-23",
  period_start: "2026-11-01",
  period_end: "2026-11-30",
  value: "30.5",
  target_value: "100",
  people: [],
  remaining: "69.5",
  exceeded_by: "0",
  over_limit: false,
};

const entry = (
  id: string,
  value: string,
  day: string,
  note: string | null = null,
): TrackerEntry => ({
  id,
  tracker_item_id: "t1",
  user_id: "u1",
  value,
  occurred_at: `${day}T12:00:00Z`,
  local_date: day,
  note,
  created_at: at,
});

const open = (server: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(server.api, url, { tokenStore: memoryTokenStore("t") });

const detailRoutes = {
  "GET /me": () => json(200, user),
  "GET /trackers/t1": () => json(200, tracker),
  "GET /trackers/t1/summary": () => json(200, summary),
  "GET /trackers/t1/entries": () =>
    json(200, { items: [entry("e1", "30.5", "2026-11-23", "Market")], next_cursor: null }),
};

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("tracker model (K1-K3)", () => {
  it("reads decimals with a dot or a comma", () => {
    expect(normalizeDecimal(" 12,5 ")).toBe("12.5");
    expect(normalizeDecimal("-3")).toBe("-3");
    for (const bad of ["", "abc", "1.23456", "1,2,3"]) {
      expect(normalizeDecimal(bad)).toBeNull();
    }
  });

  it("checks calendar days", () => {
    expect(isDay("2028-02-29")).toBe(true);
    expect(isDay("2026-02-29")).toBe(false);
    expect(isDay("26-1-1")).toBe(false);
  });

  it("moves days across month ends and clock changes", () => {
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
    expect(addDays("2026-03-29", 1)).toBe("2026-03-30");
    expect(addDays("2026-10-25", 1)).toBe("2026-10-26");
  });

  it("builds a request from a valid form", () => {
    const result = validateForm({ ...emptyForm, title: " Kitap ", unit: "sayfa", target: "300,5" });
    expect(result.errors).toBeUndefined();
    expect(result).toMatchObject({
      request: { title: "Kitap", target_value: "300.5", period: { type: "month" } },
    });
  });

  it("says what is wrong with each field", () => {
    expect(validateForm(emptyForm).errors).toEqual({
      title: "required",
      unit: "required",
      target: "required",
    });
    const filled = { ...emptyForm, title: "a", unit: "b" };
    expect(validateForm({ ...filled, target: "x" }).errors).toEqual({ target: "invalidNumber" });
    expect(validateForm({ ...filled, target: "0" }).errors).toEqual({ target: "positive" });
    expect(
      validateForm({
        ...filled,
        target: "1",
        period: "range",
        rangeStart: "2026-12-02",
        rangeEnd: "2026-12-01",
      }).errors,
    ).toEqual({ rangeEnd: "endBeforeStart" });
  });

  it("combines each day by the tracker's aggregation", () => {
    const entries = [
      entry("a", "0.1", "2026-11-23"),
      { ...entry("b", "0.2", "2026-11-23"), occurred_at: "2026-11-23T08:00:00Z" },
    ];
    const value = (aggregation: "sum" | "count" | "last" | "max") =>
      dailySeries(entries, aggregation, "2026-11-23", 2)[1]?.value;
    expect(value("sum")).toBe(0.3);
    expect(value("count")).toBe(2);
    expect(value("last")).toBe(0.1);
    expect(value("max")).toBe(0.2);
    expect(dailySeries(entries, "sum", "2026-11-23", 2)[0]).toEqual({
      day: "2026-11-22",
      value: null,
    });
  });

  it("keeps bars and progress between 0 and 1", () => {
    const series = [
      { day: "a", value: 4 },
      { day: "b", value: -2 },
      { day: "c", value: null },
    ];
    expect(barShare(4, series)).toBe(1);
    expect(barShare(-2, series)).toBe(0);
    expect(barShare(null, series)).toBe(0);
    expect(progressShare("150", "100")).toBe(1);
    expect(progressShare("-5", "100")).toBe(0);
    expect(progressShare(null, "100")).toBe(0);
  });

  it("uses the language's decimal mark without rounding", () => {
    expect(formatAmount("12.3456", "tr")).toBe("12,3456");
    expect(formatAmount("12.3456", "en")).toBe("12.3456");
  });
});

describe("trackers screen (K1)", () => {
  it("explains an empty list", async () => {
    await open(
      fakeServer({
        "GET /me": () => json(200, user),
        "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
      }),
      "/trackers",
    );
    expect(await screen.findByText("Henüz takibin yok")).toBeTruthy();
  });

  it("creates a tracker and opens it", async () => {
    const server = fakeServer({
      ...detailRoutes,
      "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
      "POST /trackers": () => json(201, tracker),
    });
    await open(server, "/trackers");
    await fireEvent.press(await screen.findByRole("button", { name: "Yeni takip" }));
    await fireEvent.changeText(screen.getByLabelText("Ad"), "Bütçe");
    await fireEvent.changeText(screen.getByLabelText("Birim"), "TL");
    await fireEvent.changeText(screen.getByLabelText("Hedef değer"), "100");
    await fireEvent.press(screen.getByRole("button", { name: "Sınırın altında kalmalı" }));
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(await screen.findByRole("header", { name: "Bütçe" })).toBeTruthy();
    const post = server.calls.find((call) => call.method === "POST");
    expect(post?.body).toEqual({
      title: "Bütçe",
      unit: "TL",
      target_value: "100",
      period: { type: "month" },
      aggregation: "sum",
      direction: "limit",
    });
  });

  it("does not send an invalid form", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
    });
    await open(server, "/trackers");
    await fireEvent.press(await screen.findByRole("button", { name: "Yeni takip" }));
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect((await screen.findAllByText("Bu alan gerekli.")).length).toBe(3);
    expect(server.calls.some((call) => call.method === "POST")).toBe(false);
  });
});

describe("tracker detail (K1-K3)", () => {
  it("shows use and what is left under a limit (K2)", async () => {
    await open(fakeServer(detailRoutes), "/trackers/t1");
    expect(await screen.findByText("30,5 TL / 100 TL kullanıldı")).toBeTruthy();
    expect(screen.getByText("Kalan: 69,5 TL")).toBeTruthy();
  });

  it("warns when the limit is passed (K2)", async () => {
    await open(
      fakeServer({
        ...detailRoutes,
        "GET /trackers/t1/summary": () =>
          json(200, {
            ...summary,
            value: "110.5",
            remaining: "0",
            exceeded_by: "10.5",
            over_limit: true,
          }),
      }),
      "/trackers/t1",
    );
    expect(await screen.findByText("Sınır 10,5 TL aşıldı")).toBeTruthy();
  });

  it("draws the history (K3)", async () => {
    await open(fakeServer(detailRoutes), "/trackers/t1");
    expect(await screen.findByLabelText("2026-11-23: 30,5 TL")).toBeTruthy();
    expect(screen.getByLabelText("2026-11-22: kayıt yok")).toBeTruthy();
    expect(screen.getByText("Market")).toBeTruthy();
  });

  it("adds an entry with a note and refreshes (K1)", async () => {
    let reads = 0;
    const server = fakeServer({
      ...detailRoutes,
      "GET /trackers/t1/summary": () => {
        reads += 1;
        return json(200, summary);
      },
      "POST /trackers/t1/entries": () => json(201, entry("e2", "5", "2026-11-23", "Kahve")),
    });
    await open(server, "/trackers/t1");
    await fireEvent.changeText(await screen.findByLabelText("Değer (TL)"), "5,5");
    await fireEvent.changeText(screen.getByLabelText("Not (isteğe bağlı)"), " Kahve ");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydı ekle" }));
    await waitFor(() => {
      expect(reads).toBeGreaterThan(1);
    });
    const post = server.calls.find((call) => call.method === "POST");
    expect(post?.body).toEqual({ value: "5.5", note: "Kahve" });
  });

  it("keeps what was typed when saving fails", async () => {
    const server = fakeServer({
      ...detailRoutes,
      "POST /trackers/t1/entries": () => apiError(500, "internal"),
    });
    await open(server, "/trackers/t1");
    const value = await screen.findByLabelText("Değer (TL)");
    await fireEvent.changeText(value, "7");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydı ekle" }));
    expect(await screen.findByText("Kaydedilemedi. Tekrar dene.")).toBeTruthy();
    expect(screen.getByLabelText("Değer (TL)").props.value).toBe("7");
  });

  it("looks the same for a hidden and a missing tracker (V9)", async () => {
    await open(fakeServer({ "GET /me": () => json(200, user) }), "/trackers/t1");
    expect(await screen.findByText(i18n.t("items.detail.notFound"))).toBeTruthy();
  });
});

describe("side-by-side progress (AK5)", () => {
  it("puts the reader first, then the others by value", () => {
    const people = [
      { user_id: "u3", value: "2" },
      { user_id: "u2", value: "9" },
      { user_id: "u1", value: "1" },
    ];
    expect(sideBySide(people, "u1").map((p) => p.user_id)).toEqual(["u1", "u2", "u3"]);
  });

  it("shows each person's value for a shared tracker and nothing for one person", async () => {
    await open(
      fakeServer({
        ...detailRoutes,
        "GET /trackers/t1/summary": () =>
          json(200, {
            ...summary,
            people: [
              { user_id: "u2", value: "20" },
              { user_id: "u1", value: "10.5" },
            ],
          }),
        "GET /items/t1/members": () =>
          json(200, {
            members: [
              { user_id: "u1", display_name: "Ali", roles: ["owner"] },
              { user_id: "u2", display_name: "Ayşe", roles: ["watcher"] },
            ],
            pending: [],
            can_manage: true,
          }),
      }),
      "/trackers/t1",
    );
    expect(await screen.findByText("Kişi bazında")).toBeTruthy();
    expect(await screen.findByLabelText("Ayşe: 20 TL")).toBeTruthy();
    expect(screen.getByLabelText("Sen: 10,5 TL")).toBeTruthy();
  });

  it("is hidden while only one person has entries", async () => {
    await open(
      fakeServer({
        ...detailRoutes,
        "GET /trackers/t1/summary": () =>
          json(200, { ...summary, people: [{ user_id: "u1", value: "30.5" }] }),
      }),
      "/trackers/t1",
    );
    await screen.findByText("30,5 TL / 100 TL kullanıldı");
    expect(screen.queryByText("Kişi bazında")).toBeNull();
  });
});
