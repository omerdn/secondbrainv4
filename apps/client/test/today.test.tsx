import type { Item, RoutineDay, Today } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import { daysFrom, itemPath, parseThreshold, percent } from "@/features/today/model";
import { deviceTimeZone, todayIn } from "@/lib/dates";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const at = "2026-11-26T09:00:00Z";
const today = todayIn(deviceTimeZone());

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

const day = (done: number, target = 2): RoutineDay => ({
  date: today,
  scheduled: true,
  target,
  done,
  complete: done >= target,
});

const addDays = (base: string, delta: number) => {
  const d = new Date(`${base}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + delta);
  return d.toISOString().slice(0, 10);
};

const full = (): Today => ({
  today,
  routines: [{ item: item("r1", "routine", "Su iç"), timezone: "Europe/Istanbul", day: day(0) }],
  overdue: [{ item: item("t1", "task", "Rapor"), due_date: addDays(today, -2), done: 1, total: 3 }],
  upcoming: [{ item: item("t2", "task", "Sunum"), due_date: addDays(today, 1), done: 0, total: 1 }],
  goals: [
    {
      item: item("g1", "goal", "Kitap yılı"),
      start_date: "2026-11-01",
      end_date: "2026-12-31",
      actual: 0.3,
      expected: 0.4,
      pace: 0.75,
    },
  ],
  forgotten: [item("t3", "task", "Eski iş")],
  forgotten_days: 7,
  resume: [
    {
      item: item("k1", "tracker", "Gitar"),
      line: { text: "Barre'da kaldım", date: today },
    },
  ],
  session: {
    id: "s1",
    timer: {
      state: "paused",
      mode: "pomodoro",
      item_id: null,
      started_at: null,
      accumulated_ms: 10 * 60_000,
      planned_ms: 25 * 60_000,
    },
    server_time: new Date().toISOString(),
  },
});

const open = (server: ReturnType<typeof fakeServer>) =>
  renderApp(server.api, "/", { tokenStore: memoryTokenStore("t") });

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("today helpers", () => {
  it("routes items by kind and formats numbers", () => {
    expect(itemPath("task", "a b")).toBe("/tasks/a%20b");
    expect(itemPath("page", "p")).toBe("/pages/p");
    expect(itemPath("notebook", "n")).toBe("/items/n");
    expect(percent(0.756)).toBe(76);
    expect(percent(Number.NaN)).toBe(0);
    expect(daysFrom("2026-11-26", "2026-11-24")).toBe(-2);
    expect(daysFrom("2026-02-28", "2026-03-01")).toBe(1);
    expect(parseThreshold("10")).toBe(10);
    expect(parseThreshold("0")).toBeNull();
    expect(parseThreshold("366")).toBeNull();
  });
});

describe("today screen (H1-H6)", () => {
  it("shows every section", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /today": () => json(200, full()),
    });
    await open(server);
    expect(await screen.findByText("Bugünün rutinleri")).toBeTruthy();
    expect(screen.getByText("Su iç")).toBeTruthy();
    expect(screen.getByText("0 / 2")).toBeTruthy();
    expect(screen.getByText("Gecikenler")).toBeTruthy();
    expect(screen.getByText("2 gün gecikti · 1/3 alt kırılım")).toBeTruthy();
    expect(screen.getByText("Yarın · 0/1 alt kırılım")).toBeTruthy();
    expect(screen.getByText("Gerçekleşen %30 · tempo %75")).toBeTruthy();
    expect(screen.getByText("Barre'da kaldım · bugün")).toBeTruthy();
    expect(screen.getByText("7 gündür dokunulmayanlar")).toBeTruthy();
    expect(screen.getByText("Eski iş")).toBeTruthy();
    expect(screen.getByLabelText("Açık çalışma oturumu")).toBeTruthy();
    expect(screen.getByTestId("session-glance-clock")).toHaveTextContent("15:00");
    expect(server.calls.find((c) => c.path.startsWith("/today"))?.path).toBe(
      `/today?today=${today}`,
    );
  });

  it("shows empty sections and hides overdue when there is none", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /today": () =>
        json(200, {
          ...full(),
          routines: [],
          overdue: [],
          upcoming: [],
          goals: [],
          forgotten: [],
          resume: [],
          session: null,
        }),
    });
    await open(server);
    expect(await screen.findByText("Bugün planlı rutin yok.")).toBeTruthy();
    expect(screen.queryByText("Gecikenler")).toBeNull();
    expect(screen.getByText("Önümüzdeki 7 günde bitecek görev yok.")).toBeTruthy();
    expect(screen.getByText("Unutulan bir şey yok.")).toBeTruthy();
    expect(screen.queryByLabelText("Açık çalışma oturumu")).toBeNull();
  });

  it("marks and unmarks a routine right on the screen (H1)", async () => {
    let done = 0;
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /today": () => json(200, full()),
      "POST /routines/r1/completions": () => {
        done += 1;
        return json(200, day(done));
      },
      "DELETE /routines/r1/completions": () => {
        done -= 1;
        return json(200, day(done));
      },
    });
    await open(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Su iç rutinini işaretle" }));
    expect(await screen.findByText("1 / 2")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Su iç rutinini işaretle" }));
    expect(await screen.findByText("2 / 2 · tamam")).toBeTruthy();
    expect(server.calls.find((c) => c.method === "POST")?.body).toEqual({ date: today });
    await fireEvent.press(screen.getByRole("button", { name: "Su iç işaretini geri al" }));
    expect(await screen.findByText("1 / 2")).toBeTruthy();
    expect(server.calls.find((c) => c.method === "DELETE")?.path).toBe(
      `/routines/r1/completions?date=${today}`,
    );
  });

  it("changes the forgotten threshold (H4)", async () => {
    let days = 7;
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /today": () => json(200, { ...full(), forgotten_days: days }),
      "PUT /today/preferences": (call) => {
        days = (call.body as { forgotten_days: number }).forgotten_days;
        return json(200, { forgotten_days: days });
      },
    });
    await open(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Süreyi değiştir" }));
    await fireEvent.changeText(screen.getByLabelText("Gün"), "0");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
    expect(screen.getByText("1 ile 365 arasında bir tam sayı gir.")).toBeTruthy();
    await fireEvent.changeText(screen.getByLabelText("Gün"), "14");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
    expect(await screen.findByText("14 gündür dokunulmayanlar")).toBeTruthy();
    await waitFor(() => {
      expect(server.calls.find((c) => c.method === "PUT")?.body).toEqual({ forgotten_days: 14 });
    });
  });

  it("opens an item from a list", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /today": () => json(200, full()),
      "GET /items/g1": () => json(200, item("g1", "goal", "Kitap yılı")),
    });
    await open(server);
    await fireEvent.press(await screen.findByRole("link", { name: "Kitap yılı" }));
    await waitFor(() => {
      expect(server.calls.some((c) => c.path.startsWith("/goals/g1"))).toBe(true);
    });
  });
});
