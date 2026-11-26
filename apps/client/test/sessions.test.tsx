import type { Item, SessionTimer, WorkSession } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import {
  clockFace,
  clockOffset,
  formatClock,
  parseMinutes,
  presetStart,
  segmentLog,
} from "@/features/sessions/model";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const T0 = Date.parse("2026-11-26T09:00:00Z");
const iso = (ms: number) => new Date(ms).toISOString();
const MIN = 60_000;
const pomodoro = { work_ms: 25 * MIN, short_break_ms: 5 * MIN, long_break_ms: 15 * MIN };

const stopped: SessionTimer = {
  state: "stopped",
  mode: null,
  item_id: null,
  started_at: null,
  accumulated_ms: 0,
  planned_ms: null,
};

const item = (id: string, kind: string, title: string): Item => ({
  id,
  kind,
  title,
  description: null,
  owner: { type: "user", user_id: "u1" },
  created_by: "u1",
  created_at: iso(T0),
  updated_at: iso(T0),
  last_touched_at: iso(T0),
  archived_at: null,
  deleted_at: null,
});

const session = (timer: SessionTimer, extra: Partial<WorkSession> = {}): WorkSession => ({
  id: "s1",
  open: true,
  timer,
  pomodoro,
  segments: [],
  created_at: iso(T0),
  closed_at: null,
  server_time: new Date().toISOString(),
  ...extra,
});

describe("timer arithmetic (ARCHITECTURE §11)", () => {
  it("counts down from the server's clock, not the device's", () => {
    // The device is 2 minutes behind the server.
    const offset = clockOffset(iso(T0 + 10 * MIN), T0 + 8 * MIN);
    expect(offset).toBe(2 * MIN);
    const timer: SessionTimer = {
      state: "running",
      mode: "pomodoro",
      item_id: null,
      started_at: iso(T0),
      accumulated_ms: 3 * MIN,
      planned_ms: 25 * MIN,
    };
    const face = clockFace(timer, T0 + 8 * MIN + offset);
    expect(face).toEqual({ ms: 12 * MIN, countdown: true, overtime: false });
    expect(formatClock(face.ms, true)).toBe("12:00");
    // Past the plan: zero and overtime, never negative.
    expect(clockFace(timer, T0 + 40 * MIN)).toEqual({ ms: 0, countdown: true, overtime: true });
  });

  it("counts up for the stopwatch and freezes while paused", () => {
    const running: SessionTimer = {
      state: "running",
      mode: "stopwatch",
      item_id: null,
      started_at: iso(T0),
      accumulated_ms: 0,
      planned_ms: null,
    };
    expect(formatClock(clockFace(running, T0 + 65 * MIN + 5_000).ms)).toBe("1:05:05");
    const paused = { ...running, state: "paused" as const, started_at: null, accumulated_ms: MIN };
    expect(clockFace(paused, T0 + 99 * MIN).ms).toBe(MIN);
  });

  it("maps presets to start commands and checks lengths (W2)", () => {
    expect(presetStart("pomodoro", pomodoro)).toEqual({ mode: "pomodoro", planned_ms: 25 * MIN });
    expect(presetStart("longBreak", pomodoro)).toEqual({ mode: "break", planned_ms: 15 * MIN });
    expect(presetStart("stopwatch", pomodoro)).toEqual({ mode: "stopwatch" });
    expect(presetStart("break", pomodoro)).toEqual({ mode: "break" });
    expect(parseMinutes(" 25 ")).toBe(25);
    expect(parseMinutes("0")).toBeNull();
    expect(parseMinutes("241")).toBeNull();
    expect(parseMinutes("2.5")).toBeNull();
  });

  it("logs segments with the open one as of now (W4)", () => {
    const timer: SessionTimer = {
      state: "running",
      mode: "pomodoro",
      item_id: "i2",
      started_at: iso(T0 + 50 * MIN),
      accumulated_ms: 0,
      planned_ms: 25 * MIN,
    };
    const log = segmentLog(
      [
        {
          id: "a",
          mode: "stopwatch",
          item_id: "i1",
          started_at: iso(T0),
          ended_at: iso(T0 + 45 * MIN),
          active_ms: 45 * MIN,
        },
        {
          id: "b",
          mode: "break",
          item_id: null,
          started_at: iso(T0 + 45 * MIN),
          ended_at: iso(T0 + 50 * MIN),
          active_ms: 5 * MIN,
        },
        {
          id: "c",
          mode: "pomodoro",
          item_id: "i2",
          started_at: iso(T0 + 50 * MIN),
          ended_at: null,
          active_ms: 0,
        },
      ],
      timer,
      T0 + 60 * MIN,
    );
    expect(log.map((e) => [e.minutes, e.current])).toEqual([
      [45, false],
      [5, false],
      [10, true],
    ]);
  });
});

const open = (server: ReturnType<typeof fakeServer>) =>
  renderApp(server.api, "/session", { tokenStore: memoryTokenStore("t") });

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("session screen (W1-W4)", () => {
  it("opens a session with the chosen pomodoro lengths", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "POST /sessions": () => json(200, session(stopped)),
    });
    await open(server);
    await fireEvent.changeText(await screen.findByLabelText("Çalışma (dk)"), "50");
    await fireEvent.changeText(screen.getByLabelText("Kısa mola (dk)"), "0");
    await fireEvent.press(screen.getByRole("button", { name: "Oturum başlat" }));
    expect(screen.getByText("Süreler 1 ile 240 dakika arasında tam sayı olmalı.")).toBeTruthy();
    await fireEvent.changeText(screen.getByLabelText("Kısa mola (dk)"), "10");
    await fireEvent.press(screen.getByRole("button", { name: "Oturum başlat" }));
    expect(await screen.findByRole("button", { name: "Başlat" })).toBeTruthy();
    expect(server.calls.find((c) => c.method === "POST")?.body).toEqual({
      pomodoro: { work_ms: 50 * MIN, short_break_ms: 10 * MIN, long_break_ms: 15 * MIN },
    });
  });

  it("shows the remaining time from the server's state after a reload", async () => {
    const now = Date.now();
    const running: SessionTimer = {
      state: "running",
      mode: "pomodoro",
      item_id: "i1",
      started_at: iso(now - 10 * MIN),
      accumulated_ms: 0,
      planned_ms: 25 * MIN,
    };
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /sessions/current": () =>
        json(
          200,
          session(running, {
            server_time: iso(now),
            segments: [
              {
                id: "a",
                mode: "break",
                item_id: null,
                started_at: iso(now - 15 * MIN),
                ended_at: iso(now - 10 * MIN),
                active_ms: 5 * MIN,
              },
              {
                id: "b",
                mode: "pomodoro",
                item_id: "i1",
                started_at: iso(now - 10 * MIN),
                ended_at: null,
                active_ms: 0,
              },
            ],
          }),
        ),
      "GET /items/i1": () => json(200, item("i1", "task", "Rapor")),
      "POST /sessions/s1/timer": () =>
        json(
          200,
          session({ ...running, state: "paused", started_at: null, accumulated_ms: 10 * MIN }),
        ),
    });
    await open(server);
    const clock = await screen.findByLabelText("Süre");
    expect(clock).toHaveTextContent(/^1[45]:\d\d$/);
    expect(await screen.findByText("10 dk · Rapor · sürüyor")).toBeTruthy();
    expect(screen.getByText("5 dk mola")).toBeTruthy();
    // Running: the item cannot change until paused (W3).
    expect(screen.getByRole("button", { name: "Değiştir" })).toBeDisabled();
    await fireEvent.press(screen.getByRole("button", { name: "Duraklat" }));
    expect(await screen.findByRole("button", { name: "Devam et" })).toBeTruthy();
    expect(screen.getByLabelText("Süre")).toHaveTextContent("15:00");
    expect(server.calls.find((c) => c.method === "POST")?.body).toEqual({ command: "pause" });
  });

  it("starts every mode with its planned time (W2)", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /sessions/current": () => json(200, session(stopped)),
      "POST /sessions/s1/timer": (call) => {
        const body = call.body as { mode: "pomodoro" | "stopwatch" | "break"; planned_ms?: number };
        return json(
          200,
          session({
            state: "running",
            mode: body.mode,
            item_id: null,
            started_at: new Date().toISOString(),
            accumulated_ms: 0,
            planned_ms: body.planned_ms ?? null,
          }),
        );
      },
    });
    await open(server);
    // Stopped: the clock shows what the chosen preset would count.
    expect(await screen.findByLabelText("Süre")).toHaveTextContent("25:00");
    await fireEvent.press(screen.getByRole("radio", { name: "Uzun mola" }));
    expect(screen.getByLabelText("Süre")).toHaveTextContent("15:00");
    await fireEvent.press(screen.getByRole("radio", { name: "Kronometre" }));
    expect(screen.getByLabelText("Süre")).toHaveTextContent("00:00");
    await fireEvent.press(screen.getByRole("button", { name: "Başlat" }));
    expect(await screen.findByRole("button", { name: "Duraklat" })).toBeTruthy();
    expect(screen.getByLabelText("Süre")).toHaveTextContent(/^00:0\d$/);
    expect(server.calls.at(-1)?.body).toEqual({ command: "start", mode: "stopwatch" });
  });

  it("switches the item while paused and closes the session (W1, W3)", async () => {
    const paused: SessionTimer = {
      state: "paused",
      mode: "stopwatch",
      item_id: null,
      started_at: null,
      accumulated_ms: 5 * MIN,
      planned_ms: null,
    };
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /sessions/current": () => json(200, session(paused)),
      "GET /items/filter": () =>
        json(200, { items: [item("i1", "task", "Gitar kursu")], next_cursor: null }),
      "GET /items/i1": () => json(200, item("i1", "task", "Gitar kursu")),
      "POST /sessions/s1/timer": () =>
        json(200, session({ ...paused, item_id: "i1", accumulated_ms: 0 })),
      "POST /sessions/s1/close": () => json(200, session(stopped, { open: false })),
    });
    await open(server);
    expect(await screen.findByText("Bağlı nesne yok")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Nesne seç" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Görev: Gitar kursu" }));
    expect(await screen.findByText("Gitar kursu")).toBeTruthy();
    expect(server.calls.find((c) => c.path === "/sessions/s1/timer")?.body).toEqual({
      command: "switch_item",
      item_id: "i1",
    });
    await fireEvent.press(screen.getByRole("button", { name: "Oturumu kapat" }));
    expect(await screen.findByRole("button", { name: "Oturum başlat" })).toBeTruthy();
    await waitFor(() => {
      expect(server.calls.some((c) => c.path === "/sessions/s1/close")).toBe(true);
    });
  });
});
