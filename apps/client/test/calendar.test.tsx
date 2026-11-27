import type { Calendar, Item } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor, within } from "expo-router/testing-library";

import {
  endOfMonth,
  entriesByDay,
  goalsOn,
  rangeFor,
  shift,
  startOfWeek,
  weekday,
  weeksOf,
} from "@/features/calendar/model";
import { deviceTimeZone, todayIn } from "@/lib/dates";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const at = "2026-11-01T09:00:00Z";
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
  team_visibility: null,
  deleted_at: null,
});

const sample = (from: string, to: string): Calendar => ({
  from,
  to,
  tasks: [
    { item: item("t1", "task", "Rapor"), start_date: "2026-11-23", due_date: "2026-11-26" },
    { item: item("t2", "task", "Fatura"), start_date: null, due_date: "2026-11-26" },
    { item: item("t3", "task", "Plan"), start_date: "2026-11-24", due_date: null },
  ],
  routines: [
    {
      item: item("r1", "routine", "Koşu"),
      timezone: "Europe/Istanbul",
      dates: ["2026-11-24", "2026-11-26"],
    },
  ],
  goals: [{ item: item("g1", "goal", "Kasım"), start_date: "2026-11-01", end_date: "2026-11-30" }],
});

describe("calendar arithmetic (C1)", () => {
  it("knows weeks and months, Monday first", () => {
    expect(weekday("2026-11-23")).toBe(0);
    expect(weekday("2026-11-29")).toBe(6);
    expect(startOfWeek("2026-11-26")).toBe("2026-11-23");
    expect(endOfMonth("2026-02-10")).toBe("2026-02-28");
    expect(endOfMonth("2028-02-10")).toBe("2028-02-29");
    expect(rangeFor("day", "2026-11-26")).toEqual({ from: "2026-11-26", to: "2026-11-26" });
    expect(rangeFor("week", "2026-11-26")).toEqual({ from: "2026-11-23", to: "2026-11-29" });
    // November 2026 starts on a Sunday and ends on a Monday: six whole weeks.
    const month = rangeFor("month", "2026-11-15");
    expect(month).toEqual({ from: "2026-10-26", to: "2026-12-06" });
    expect(weeksOf(month.from, month.to)).toHaveLength(6);
  });

  it("steps by a day, a week or a month", () => {
    expect(shift("day", "2026-12-31", 1)).toBe("2027-01-01");
    expect(shift("week", "2026-11-26", -1)).toBe("2026-11-19");
    expect(shift("month", "2026-01-31", 1)).toBe("2026-02-01");
    expect(shift("month", "2026-01-15", -1)).toBe("2025-12-01");
  });

  it("puts each entry on its days", () => {
    const byDay = entriesByDay(sample("2026-11-23", "2026-11-29"), "2026-11-23", "2026-11-29");
    const kinds = (day: string) => (byDay.get(day) ?? []).map((e) => `${e.kind}:${e.item.title}`);
    expect(kinds("2026-11-23")).toEqual(["taskStart:Rapor"]);
    expect(kinds("2026-11-24")).toEqual(["taskOn:Plan", "routine:Koşu"]);
    expect(kinds("2026-11-26")).toEqual(["taskDue:Fatura", "taskDue:Rapor", "routine:Koşu"]);
    // The goal's first day is outside the range; its last day is too.
    expect(kinds("2026-11-29")).toEqual([]);
    expect(goalsOn(sample("2026-11-23", "2026-11-29"), "2026-11-29").map((g) => g.title)).toEqual([
      "Kasım",
    ]);
    expect(goalsOn(sample("2026-11-23", "2026-11-29"), "2026-12-01")).toEqual([]);
  });
});

const today = todayIn(deviceTimeZone());

const open = (server: ReturnType<typeof fakeServer>) =>
  renderApp(server.api, "/calendar", { tokenStore: memoryTokenStore("t") });

const calendarServer = () =>
  fakeServer({
    "GET /me": () => json(200, user),
    "GET /calendar": (call) => {
      const query = new URL(`http://x${call.path}`).searchParams;
      return json(200, sample(query.get("from") ?? "", query.get("to") ?? ""));
    },
    "GET /items/t2": () => json(200, item("t2", "task", "Fatura")),
  });

const lastRange = (server: ReturnType<typeof fakeServer>) => {
  const call = server.calls.filter((c) => c.path.startsWith("/calendar")).at(-1);
  const query = new URL(`http://x${call?.path ?? ""}`).searchParams;
  return { from: query.get("from"), to: query.get("to") };
};

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("calendar screen (C1)", () => {
  it("opens on this month and navigates between months", async () => {
    const server = calendarServer();
    await open(server);
    expect(await screen.findByRole("radio", { name: "Ay", selected: true })).toBeTruthy();
    await waitFor(() => {
      expect(lastRange(server)).toEqual(rangeFor("month", today));
    });
    await fireEvent.press(screen.getByRole("button", { name: "Sonraki" }));
    await waitFor(() => {
      expect(lastRange(server)).toEqual(rangeFor("month", shift("month", today, 1)));
    });
    await fireEvent.press(screen.getByRole("button", { name: "Bugün" }));
    await waitFor(() => {
      expect(lastRange(server)).toEqual(rangeFor("month", today));
    });
  });

  it("switches to the week and day views and opens a day from the month", async () => {
    const server = calendarServer();
    await open(server);
    await fireEvent.press(await screen.findByRole("radio", { name: "Hafta" }));
    await waitFor(() => {
      expect(lastRange(server)).toEqual(rangeFor("week", today));
    });
    await fireEvent.press(screen.getByRole("button", { name: "Önceki" }));
    await waitFor(() => {
      expect(lastRange(server)).toEqual(rangeFor("week", shift("week", today, -1)));
    });
    await fireEvent.press(screen.getByRole("radio", { name: "Gün" }));
    await waitFor(() => {
      expect(lastRange(server).from).toBe(lastRange(server).to);
    });

    // Back to the month of the sample: a cell press opens that day (no hover).
    await fireEvent.press(screen.getByRole("radio", { name: "Ay" }));
    const cell = await screen.findByRole("button", { name: /^26 Kasım, \d+ kayıt$/ });
    await fireEvent.press(cell);
    await waitFor(() => {
      expect(lastRange(server)).toEqual({ from: "2026-11-26", to: "2026-11-26" });
    });
  });

  it("lists a day's entries and running goals, and opens an item", async () => {
    const server = calendarServer();
    await open(server);
    await fireEvent.press(await screen.findByRole("radio", { name: "Gün" }));
    // Go to 26 November from wherever today is, through the month grid.
    await fireEvent.press(screen.getByRole("radio", { name: "Ay" }));
    await fireEvent.press(await screen.findByRole("button", { name: /^26 Kasım, \d+ kayıt$/ }));
    expect(await screen.findByRole("link", { name: "Görev bitiyor: Rapor" })).toBeTruthy();
    expect(screen.getByRole("link", { name: "Rutin: Koşu" })).toBeTruthy();
    expect(screen.getByText("Süren hedefler")).toBeTruthy();
    await fireEvent.press(screen.getByRole("link", { name: "Görev bitiyor: Fatura" }));
    await waitFor(() => {
      expect(server.calls.some((c) => c.path.startsWith("/tasks/t2"))).toBe(true);
    });
  });

  it("shows +N when a month cell has more than three entries", async () => {
    const many = (from: string, to: string): Calendar => ({
      ...sample(from, to),
      tasks: ["a", "b", "c", "d", "e"].map((id) => ({
        item: item(id, "task", `İş ${id}`),
        start_date: null,
        due_date: today,
      })),
      routines: [],
      goals: [],
    });
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /calendar": (call) => {
        const query = new URL(`http://x${call.path}`).searchParams;
        return json(200, many(query.get("from") ?? "", query.get("to") ?? ""));
      },
    });
    await open(server);
    const cell = await screen.findByRole("button", { name: /, 5 kayıt$/ });
    expect(within(cell).getByText("+2 daha")).toBeTruthy();
    expect(within(cell).getByText("İş a")).toBeTruthy();
    expect(within(cell).queryByText("İş d")).toBeNull();
  });
});
