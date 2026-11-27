import type {
  Item,
  ItemMembers,
  Routine,
  RoutineDay,
  RoutineRotationSchedule,
} from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor, within } from "expo-router/testing-library";

import {
  canMark,
  completionShare,
  heatLevel,
  heatmapStart,
  heatmapWeeks,
  weekStart,
} from "@/features/routines/checkin";
import { describePlan } from "@/features/routines/describe";
import {
  addReminder,
  buildRrule,
  emptyForm,
  isYearlyDate,
  nextOccurrences,
  normalizeTime,
  REMINDER_LIMIT,
  validateForm,
} from "@/features/routines/recurrence";
import { linesOn, movedUp, recentDays } from "@/features/routines/shared";
import { addDays, todayIn } from "@/lib/dates";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const at = "2026-11-23T12:00:00Z";

const item: Item = {
  id: "r1",
  kind: "routine",
  title: "Su iç",
  description: null,
  owner: { type: "user", user_id: "u1" },
  created_by: "u1",
  created_at: at,
  updated_at: at,
  last_touched_at: at,
  archived_at: null,
  deleted_at: null,
};

const routine: Routine = {
  item,
  timezone: "Europe/Istanbul",
  daily_target: 8,
  start_date: "2026-11-23",
  end_date: null,
  reminder_times: ["08:00", "20:30"],
  completion_mode: "pool",
  rotation: null,
  versions: [
    {
      id: "v1",
      rrule: "FREQ=WEEKLY;BYDAY=MO,TH",
      daily_target: 8,
      valid_from: "2026-11-23",
      valid_to: null,
    },
  ],
};

const form = (changes: Partial<ReturnType<typeof emptyForm>> = {}) => ({
  ...emptyForm("2026-11-23", "Europe/Istanbul"),
  title: "Su iç",
  ...changes,
});

const open = (server: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(server.api, url, { tokenStore: memoryTokenStore("t") });

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("routine plan model (R1-R3, R13)", () => {
  it("writes every recurrence kind as the server reads it (R1)", () => {
    expect(buildRrule(form(), 1)).toBe("FREQ=DAILY");
    expect(buildRrule(form(), 3)).toBe("FREQ=DAILY;INTERVAL=3");
    expect(buildRrule(form({ frequency: "weekly", weekdays: ["TH", "MO"] }), 2)).toBe(
      "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH",
    );
    expect(buildRrule(form({ frequency: "monthly", monthDays: [-1, 20, 15] }), 1)).toBe(
      "FREQ=MONTHLY;BYMONTHDAY=-1,15,20",
    );
    expect(
      buildRrule(
        form({
          frequency: "yearly",
          yearlyDates: [
            { month: 9, day: 1 },
            { month: 3, day: 8 },
          ],
        }),
        1,
      ),
    ).toBe("FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=8\nFREQ=YEARLY;BYMONTH=9;BYMONTHDAY=1");
    expect(buildRrule(form({ frequency: "weekly" }), 1)).toBe("");
  });

  it("previews the same days the server schedules", () => {
    const start = "2026-11-23"; // a Monday
    expect(nextOccurrences("FREQ=DAILY;INTERVAL=3", start, start, 3)).toEqual([
      "2026-11-23",
      "2026-11-26",
      "2026-11-29",
    ]);
    expect(nextOccurrences("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH", start, start, 4)).toEqual([
      "2026-11-23",
      "2026-11-26",
      "2026-12-07",
      "2026-12-10",
    ]);
    // The last day of the month, February included; the 31st is skipped in short months.
    expect(nextOccurrences("FREQ=MONTHLY;BYMONTHDAY=-1", "2027-01-15", "2027-01-15", 3)).toEqual([
      "2027-01-31",
      "2027-02-28",
      "2027-03-31",
    ]);
    expect(nextOccurrences("FREQ=MONTHLY;BYMONTHDAY=31", "2027-01-01", "2027-01-01", 2)).toEqual([
      "2027-01-31",
      "2027-03-31",
    ]);
    expect(
      nextOccurrences(
        "FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=8\nFREQ=YEARLY;BYMONTH=9;BYMONTHDAY=1",
        start,
        start,
        3,
      ),
    ).toEqual(["2027-03-08", "2027-09-01", "2028-03-08"]);
    // Nothing before the start, nothing after the end.
    expect(nextOccurrences("FREQ=DAILY", "2026-12-01", start, 1)).toEqual(["2026-12-01"]);
    expect(nextOccurrences("FREQ=DAILY", start, start, 5, "2026-11-24")).toEqual([
      "2026-11-23",
      "2026-11-24",
    ]);
    expect(nextOccurrences("FREQ=SECONDLY", start, start, 5)).toEqual([]);
  });

  it("builds the create request", () => {
    const result = validateForm(
      form({ dailyTarget: "8", reminders: ["08:00"], endDate: "2026-12-31", interval: "2" }),
    );
    expect(result.request).toEqual({
      title: "Su iç",
      timezone: "Europe/Istanbul",
      rrule: "FREQ=DAILY;INTERVAL=2",
      daily_target: 8,
      start_date: "2026-11-23",
      end_date: "2026-12-31",
      reminder_times: ["08:00"],
    });
  });

  it("says what is wrong with each field", () => {
    expect(
      validateForm({ ...form(), title: " ", interval: "0", dailyTarget: "x", startDate: "" })
        .errors,
    ).toEqual({
      title: "required",
      interval: "intervalRange",
      dailyTarget: "wholeNumber",
      startDate: "required",
    });
    expect(validateForm(form({ frequency: "weekly" })).errors).toEqual({ weekdays: "pickWeekday" });
    expect(validateForm(form({ frequency: "monthly" })).errors).toEqual({
      monthDays: "pickMonthDay",
    });
    expect(validateForm(form({ frequency: "yearly" })).errors).toEqual({ yearlyDates: "pickDate" });
    expect(validateForm(form({ dailyTarget: "101" })).errors).toEqual({
      dailyTarget: "targetRange",
    });
    expect(validateForm(form({ endDate: "2026-11-22" })).errors).toEqual({
      endDate: "endBeforeStart",
    });
  });

  it("reads reminder times and dates", () => {
    expect(normalizeTime("7:5")).toBe("07:05");
    expect(normalizeTime("20.30")).toBe("20:30");
    expect(normalizeTime("24:00")).toBeNull();
    expect(normalizeTime("abc")).toBeNull();
    expect(addReminder(["20:00"], "08:00")).toEqual(["08:00", "20:00"]);
    expect(addReminder(["08:00"], "08:00")).toEqual(["08:00"]);
    const full = Array.from(
      { length: REMINDER_LIMIT },
      (_, i) => `${String(i).padStart(2, "0")}:00`,
    );
    expect(addReminder(full, "23:30")).toHaveLength(REMINDER_LIMIT);
    expect(isYearlyDate({ month: 2, day: 29 })).toBe(true);
    expect(isYearlyDate({ month: 4, day: 31 })).toBe(false);
  });

  it("describes plans in words", () => {
    const t = i18n.t.bind(i18n);
    expect(describePlan("FREQ=DAILY", t, "tr")).toBe("Her gün");
    expect(describePlan("FREQ=DAILY;INTERVAL=3", t, "tr")).toBe("3 günde bir");
    expect(describePlan("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH", t, "tr")).toBe(
      "2 haftada bir: Pzt, Per",
    );
    expect(describePlan("FREQ=MONTHLY;BYMONTHDAY=15,-1", t, "tr")).toBe("Her ay: 15, Son gün");
    expect(describePlan("FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=8", t, "tr")).toBe("Her yıl: 8 Mart");
    expect(describePlan("nonsense", t, "tr")).toBe("Özel plan");
  });

  it("finds today in the routine's zone", () => {
    const late = new Date("2026-11-23T22:30:00Z");
    expect(todayIn("Europe/Istanbul", late)).toBe("2026-11-24");
    expect(todayIn("America/New_York", late)).toBe("2026-11-23");
    expect(todayIn("Not/AZone", late)).toBe("2026-11-23");
  });
});

describe("routines screen (R1-R3, R13)", () => {
  it("explains an empty list", async () => {
    await open(
      fakeServer({
        "GET /me": () => json(200, user),
        "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
      }),
      "/routines",
    );
    expect(await screen.findByText("Henüz rutinin yok")).toBeTruthy();
  });

  it("creates a weekly routine with reminders after showing its preview", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
      "POST /routines": () => json(201, routine),
      "GET /routines/r1": () => json(200, routine),
    });
    await open(server, "/routines");
    await fireEvent.press(await screen.findByRole("button", { name: "Yeni rutin" }));
    await fireEvent.changeText(screen.getByLabelText("Ad"), "Su iç");
    await fireEvent.press(screen.getByRole("button", { name: "Haftalık" }));
    expect(screen.getByText("Önizleme için planı tamamla.")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Pzt" }));
    await fireEvent.press(screen.getByRole("button", { name: "Per" }));
    await fireEvent.changeText(screen.getByLabelText("Günlük hedef (kaç kez)"), "8");
    await fireEvent.changeText(screen.getByLabelText("Başlangıç (YYYY-AA-GG)"), "2026-11-23");
    await fireEvent.changeText(screen.getByLabelText("Saat dilimi"), "Europe/Istanbul");
    await fireEvent.changeText(screen.getByLabelText("Hatırlatma saati (SS:DD)"), "8:00");
    await fireEvent.press(screen.getByRole("button", { name: "Saati ekle" }));
    await fireEvent.changeText(screen.getByLabelText("Hatırlatma saati (SS:DD)"), "25:00");
    await fireEvent.press(screen.getByRole("button", { name: "Saati ekle" }));
    expect(screen.getByText("Saati SS:DD olarak yaz.")).toBeTruthy();
    const preview = screen.getByLabelText("Sonraki tekrarlar");
    expect(within(preview).getByText("Her hafta: Pzt, Per")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(await screen.findByRole("header", { name: "Su iç" })).toBeTruthy();
    const post = server.calls.find((call) => call.method === "POST");
    expect(post?.body).toEqual({
      title: "Su iç",
      timezone: "Europe/Istanbul",
      rrule: "FREQ=WEEKLY;BYDAY=MO,TH",
      daily_target: 8,
      start_date: "2026-11-23",
      reminder_times: ["08:00"],
    });
  });

  it("adds yearly dates and the last day of the month", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
      "POST /routines": () => json(201, routine),
      "GET /routines/r1": () => json(200, routine),
    });
    await open(server, "/routines");
    await fireEvent.press(await screen.findByRole("button", { name: "Yeni rutin" }));
    await fireEvent.changeText(screen.getByLabelText("Ad"), "Fatura");
    await fireEvent.press(screen.getByRole("button", { name: "Aylık" }));
    await fireEvent.press(screen.getByRole("button", { name: "Son gün" }));
    expect(
      within(screen.getByLabelText("Sonraki tekrarlar")).getByText("Her ay: Son gün"),
    ).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Yıllık" }));
    await fireEvent.press(screen.getByRole("button", { name: "Mart" }));
    await fireEvent.changeText(screen.getByLabelText("Gün"), "8");
    await fireEvent.press(screen.getByRole("button", { name: "Tarihi ekle" }));
    await fireEvent.changeText(screen.getByLabelText("Gün"), "31");
    await fireEvent.press(screen.getByRole("button", { name: "Şubat" }));
    await fireEvent.press(screen.getByRole("button", { name: "Tarihi ekle" }));
    expect(screen.getByText("Geçerli bir ay ve gün seç.")).toBeTruthy();
    await fireEvent.changeText(screen.getByLabelText("Kaç yılda bir"), "2");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    await waitFor(() => {
      expect(server.calls.some((call) => call.method === "POST")).toBe(true);
    });
    const post = server.calls.find((call) => call.method === "POST");
    expect(post?.body).toMatchObject({
      title: "Fatura",
      rrule: "FREQ=YEARLY;INTERVAL=2;BYMONTH=3;BYMONTHDAY=8",
    });
  });

  it("shows a routine's plan, preview and reminders", async () => {
    await open(
      fakeServer({
        "GET /me": () => json(200, user),
        "GET /routines/r1": () => json(200, routine),
        "GET /routines/r1/completions": () => json(200, []),
        "GET /routines/r1/streak": () => json(200, { current: 0, today_pending: false }),
      }),
      "/routines/r1",
    );
    expect(await screen.findByRole("header", { name: "Su iç" })).toBeTruthy();
    expect(screen.getByText("Her hafta: Pzt, Per")).toBeTruthy();
    expect(screen.getByText("Günde 8 kez")).toBeTruthy();
    expect(screen.getByText("08:00, 20:30")).toBeTruthy();
    expect(screen.getByText("Saat dilimi: Europe/Istanbul")).toBeTruthy();
  });

  it("does not reveal a routine it cannot see", async () => {
    await open(
      fakeServer({
        "GET /me": () => json(200, user),
        "GET /routines/r1": () => json(404, { error: { code: "not_found", message: "not found" } }),
      }),
      "/routines/r1",
    );
    expect(await screen.findByText(i18n.t("items.detail.notFound"))).toBeTruthy();
  });
});

const rday = (date: string, done: number, target = 4, scheduled = true): RoutineDay => ({
  date,
  scheduled,
  target,
  done,
  complete: scheduled && done >= target,
});

describe("check-in and heatmap model (R4, R7, R8)", () => {
  it("darkens with the completion share and keeps unscheduled days apart (R8)", () => {
    expect(heatLevel(rday("2026-11-23", 0))).toBe(0);
    expect(heatLevel(rday("2026-11-23", 1))).toBe(1);
    expect(heatLevel(rday("2026-11-23", 2))).toBe(2);
    expect(heatLevel(rday("2026-11-23", 3))).toBe(3);
    expect(heatLevel(rday("2026-11-23", 4))).toBe(4);
    expect(heatLevel(rday("2026-11-23", 9))).toBe(4);
    expect(heatLevel(rday("2026-11-23", 0, 1, false))).toBe("off");
    // The shade never gets lighter as more is done.
    let last = 0;
    for (let done = 0; done <= 8; done += 1) {
      const level = heatLevel(rday("2026-11-23", done, 8));
      expect(level).not.toBe("off");
      expect(Number(level)).toBeGreaterThanOrEqual(last);
      last = Number(level);
    }
  });

  it("counts partial days (R4)", () => {
    expect(completionShare({ done: 5, target: 8 })).toBe(5 / 8);
    expect(completionShare({ done: 12, target: 8 })).toBe(1);
    expect(completionShare({ done: 0, target: 0 })).toBe(0);
  });

  it("marks only the last 3 days (R7)", () => {
    expect(canMark("2026-11-23", "2026-11-23")).toBe(true);
    expect(canMark("2026-11-20", "2026-11-23")).toBe(true);
    expect(canMark("2026-11-19", "2026-11-23")).toBe(false);
    expect(canMark("2026-11-24", "2026-11-23")).toBe(false);
    expect(canMark("2026-12-30", "2027-01-02")).toBe(true);
  });

  it("lays out weeks Monday first, up to today", () => {
    expect(weekStart("2026-11-26")).toBe("2026-11-23");
    expect(weekStart("2026-11-23")).toBe("2026-11-23");
    expect(weekStart("2026-11-29")).toBe("2026-11-23");
    expect(heatmapStart("2026-11-26", 2)).toBe("2026-11-16");
    const weeks = heatmapWeeks([rday("2026-11-16", 1), rday("2026-11-26", 4)], "2026-11-26", 2);
    expect(weeks).toHaveLength(2);
    expect(weeks[0]?.[0]?.date).toBe("2026-11-16");
    expect(weeks[0]?.[1]).toBeNull();
    expect(weeks[1]?.[3]?.date).toBe("2026-11-26");
    // Friday to Sunday are still ahead.
    expect(weeks[1]?.slice(4)).toEqual([null, null, null]);
  });
});

describe("routine check-in (R4, R8, R9)", () => {
  const daily: Routine = {
    ...routine,
    daily_target: 2,
    versions: [
      { ...routine.versions[0]!, rrule: "FREQ=DAILY", daily_target: 2, valid_from: "2026-01-01" },
    ],
    start_date: "2026-01-01",
  };

  function checkInServer() {
    const today = todayIn("Europe/Istanbul");
    const done = new Map<string, number>([[addDays(today, -10), 1]]);
    let streak = 0;
    const days = (from: string, to: string) => {
      const result: RoutineDay[] = [];
      for (let day = from; day <= to; day = addDays(day, 1)) {
        result.push(rday(day, done.get(day) ?? 0, 2));
      }
      return result;
    };
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /routines/r1": () => json(200, daily),
      "GET /routines/r1/completions": (call) => {
        const query = new URL(`http://x${call.path}`).searchParams;
        return json(200, days(query.get("from") ?? today, query.get("to") ?? today));
      },
      "GET /routines/r1/streak": () =>
        json(200, { current: streak, today_pending: (done.get(today) ?? 0) < 2 }),
      "POST /routines/r1/completions": (call) => {
        const date = (call.body as { date: string }).date;
        done.set(date, (done.get(date) ?? 0) + 1);
        if (date === today && done.get(date) === 2) {
          streak = 1;
        }
        return json(201, rday(date, done.get(date) ?? 0, 2));
      },
      "DELETE /routines/r1/completions": (call) => {
        const date = new URL(`http://x${call.path}`).searchParams.get("date") ?? today;
        done.set(date, Math.max((done.get(date) ?? 0) - 1, 0));
        return json(200, rday(date, done.get(date) ?? 0, 2));
      },
    });
    return { server, today };
  }

  it("shows partial progress with +1 and the streak once the day is complete", async () => {
    const { server, today } = checkInServer();
    await open(server, "/routines/r1");
    expect(await screen.findByText("0 / 2")).toBeTruthy();
    expect(await screen.findByText("Seri: 0 tekrar")).toBeTruthy();
    expect(screen.getByText("Bugünü tamamlarsan seri sürer.")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Bugün için +1" }));
    expect(await screen.findByText("1 / 2")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Bugün için +1" }));
    expect(await screen.findByText("2 / 2")).toBeTruthy();
    expect(await screen.findByText("Seri: 1 tekrar")).toBeTruthy();
    expect(screen.getByText("Tamamlandı")).toBeTruthy();
    const posts = server.calls.filter((call) => call.method === "POST");
    expect(posts.map((call) => call.body)).toEqual([{ date: today }, { date: today }]);
    await fireEvent.press(screen.getByRole("button", { name: "Geri al" }));
    expect(await screen.findByText("1 / 2")).toBeTruthy();
    expect(
      server.calls.some((call) => call.method === "DELETE" && call.path.includes(`date=${today}`)),
    ).toBe(true);
  });

  it("picks a day from the heatmap and refuses days older than 3", async () => {
    const { server, today } = checkInServer();
    await open(server, "/routines/r1");
    await screen.findByText("0 / 2");
    const old = addDays(today, -10);
    const oldLabel = new RegExp(`: 1 / 2$`);
    await fireEvent.press(screen.getByRole("button", { name: oldLabel }));
    expect(await screen.findByText("1 / 2")).toBeTruthy();
    expect(screen.getByText("Sadece son 3 gün işaretlenebilir.")).toBeTruthy();
    expect(screen.getByRole("button", { name: /için \+1$/ })).toBeDisabled();
    expect(server.calls.some((call) => call.method === "POST")).toBe(false);
    expect(old < today).toBe(true);
  });
});

describe("shared routine model (R5, R6)", () => {
  const data = {
    responsible: ["u2", "u1"],
    days: [
      { date: "2026-11-23", user_id: "u1", done: 2 },
      { date: "2026-11-23", user_id: "u3", done: 1 },
      { date: "2026-11-22", user_id: "u2", done: 1 },
    ],
  };

  it("lists responsible people first, in turn order, then anyone else who helped", () => {
    expect(linesOn(data, "2026-11-23", ["u1", "u2", "u3"])).toEqual([
      { userId: "u1", done: 2, responsible: true },
      { userId: "u2", done: 0, responsible: true },
      { userId: "u3", done: 1, responsible: false },
    ]);
    expect(linesOn(data, "2026-11-21", ["u1", "u2"])).toEqual([
      { userId: "u1", done: 0, responsible: true },
      { userId: "u2", done: 0, responsible: true },
    ]);
  });

  it("goes back day by day and moves a person up the turn order", () => {
    expect(recentDays("2026-11-23", 3)).toEqual(["2026-11-23", "2026-11-22", "2026-11-21"]);
    expect(movedUp(["a", "b", "c"], 2)).toEqual(["a", "c", "b"]);
    expect(movedUp(["a", "b"], 0)).toEqual(["a", "b"]);
  });
});

describe("shared routine (R5, R6)", () => {
  const today = todayIn("Europe/Istanbul");
  const shared: Routine = {
    ...routine,
    start_date: addDays(today, -2),
    daily_target: 1,
    completion_mode: "individual",
  };
  const members = (canManage: boolean): ItemMembers => ({
    members: [
      { user_id: "u1", display_name: "Ali", roles: ["owner"] },
      { user_id: "u2", display_name: "Ayşe", roles: ["assignee"] },
    ],
    pending: [],
    can_manage: canManage,
  });
  const schedule: RoutineRotationSchedule = {
    rotation: "every_occurrence",
    members: ["u1", "u2"],
    assignments: [
      { date: addDays(today, 1), user_id: "u1" },
      { date: addDays(today, 2), user_id: "u2" },
    ],
  };

  function sharedServer(over: Partial<Routine> = {}, canManage = true, swapStatus = 200) {
    return fakeServer({
      "GET /me": () => json(200, user),
      "GET /routines/r1": () => json(200, { ...shared, ...over }),
      "GET /routines/r1/completions": () => json(200, []),
      "GET /routines/r1/streak": () => json(200, { current: 0, today_pending: false }),
      "GET /items/r1/members": () => json(200, members(canManage)),
      "GET /routines/r1/completions/people": () =>
        json(200, {
          responsible: ["u1", "u2"],
          days: [{ date: today, user_id: "u2", done: 1 }],
        }),
      "GET /routines/r1/rotation": () => json(200, schedule),
      "PATCH /routines/r1": (call) => json(200, { ...shared, ...(call.body as Partial<Routine>) }),
      "POST /routines/r1/rotation/swaps": () =>
        swapStatus === 200
          ? json(200, schedule)
          : json(swapStatus, { error: { code: "forbidden", message: "no" } }),
    });
  }

  const writes = (server: ReturnType<typeof fakeServer>) =>
    server.calls.filter((call) => call.method !== "GET");

  it("shows who did it on each day against each person's target (R5)", async () => {
    await open(sharedServer(), "/routines/r1");
    expect(await screen.findByText("Kim, ne zaman yaptı")).toBeTruthy();
    expect(await screen.findByLabelText("Ayşe, 1 kez")).toBeTruthy();
    expect(screen.getAllByText("Sen: 0/1").length).toBeGreaterThan(0);
    // Only the days since the routine started: today and the two before it.
    expect(screen.getAllByText("Ayşe: 0/1").length).toBe(2);
  });

  it("is hidden for a routine nobody else is on", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /routines/r1": () => json(200, routine),
      "GET /routines/r1/completions": () => json(200, []),
      "GET /routines/r1/streak": () => json(200, { current: 0, today_pending: false }),
      "GET /items/r1/members": () =>
        json(200, { ...members(true), members: members(true).members.slice(0, 1) }),
    });
    await open(server, "/routines/r1");
    await screen.findByText("Günde 8 kez");
    await waitFor(() => {
      expect(server.calls.some((call) => call.path === "/items/r1/members")).toBe(true);
    });
    expect(screen.queryByText("Ortak rutin")).toBeNull();
    expect(server.calls.some((call) => call.path.includes("/completions/people"))).toBe(false);
  });

  it("lets a manager change the rotation and the turn order (R6)", async () => {
    const server = sharedServer({ completion_mode: "pool", rotation: "every_occurrence" });
    await open(server, "/routines/r1");
    await fireEvent.press(await screen.findByRole("button", { name: "Her hafta döner" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Ayşe sırada bir öne" }));
    await fireEvent.press(screen.getByRole("button", { name: "Herkes ayrı" }));
    await waitFor(() => {
      expect(writes(server).map((call) => call.body)).toEqual([
        { rotation: "weekly" },
        { rotation_order: ["u2", "u1"] },
        { completion_mode: "individual", rotation: null },
      ]);
    });
  });

  it("offers turns only with a shared pool (R5, R6)", async () => {
    const server = sharedServer();
    await open(server, "/routines/r1");
    expect(await screen.findByText("Nöbet yalnız ortak havuzda kullanılabilir.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Her hafta döner" })).toBeNull();
    await fireEvent.press(screen.getByRole("button", { name: "Ortak havuz" }));
    await waitFor(() => {
      expect(writes(server)[0]?.body).toEqual({ completion_mode: "pool" });
    });
  });

  it("hides the settings from someone who cannot manage", async () => {
    await open(
      sharedServer({ completion_mode: "pool", rotation: "weekly" }, false),
      "/routines/r1",
    );
    expect(await screen.findByText("Sıradaki nöbetler")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Ortak havuz" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Ayşe sırada bir öne" })).toBeNull();
  });

  it("swaps two picked turns and explains a refused swap (R6)", async () => {
    const server = sharedServer(
      { completion_mode: "pool", rotation: "every_occurrence" },
      false,
      403,
    );
    await open(server, "/routines/r1");
    const first = await screen.findByRole("button", { name: /· Sen$/ });
    expect(screen.queryByRole("button", { name: "Seçilenleri takas et" })).toBeNull();
    await fireEvent.press(first);
    await fireEvent.press(screen.getByRole("button", { name: /· Ayşe$/ }));
    await fireEvent.press(screen.getByRole("button", { name: "Seçilenleri takas et" }));
    await waitFor(() => {
      expect(writes(server)[0]?.body).toEqual({
        first: addDays(today, 1),
        second: addDays(today, 2),
      });
    });
    expect(await screen.findByText("Yalnız kendi nöbetini takas edebilirsin.")).toBeTruthy();
  });
});
