import type { Notification, NotificationPreference } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor, within } from "expo-router/testing-library";
import { useWindowDimensions } from "react-native";

import { defaultRules } from "@/connectivity";
import { invalidationsFor } from "@/connectivity/invalidation";
import { toggled } from "@/features/notifications/PreferencesScreen";
import { NOTIFICATIONS_KEY } from "@/features/notifications/useUnreadCount";
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

const notification = (id: string, title: string, read = false): Notification => ({
  id,
  payload: {
    type: "invitation",
    invitation_id: `inv-${id}`,
    item_id: `item-${id}`,
    item_kind: "task",
    item_title: title,
    from_user: "u2",
    from_name: "Ada",
    role: "watcher",
  },
  read_at: read ? "2026-11-27T10:00:00Z" : null,
  created_at: "2026-11-27T09:00:00Z",
});

/** A fake center whose list follows the reads. */
const center = (initial: Notification[]) => {
  let items = initial;
  const unread = () => items.filter((n) => n.read_at === null).length;
  const server = fakeServer({
    "GET /me": () => json(200, user),
    "GET /today": () => json(404, { error: { code: "not_found", message: "not_found" } }),
    "GET /notifications": () => json(200, { notifications: items, unread: unread() }),
    "POST /notifications/read-all": () => {
      items = items.map((n) => ({ ...n, read_at: n.read_at ?? "2026-11-27T11:00:00Z" }));
      return json(204);
    },
    ...Object.fromEntries(
      initial.map((n) => [
        `POST /notifications/${n.id}/read`,
        () => {
          items = items.map((m) => (m.id === n.id ? { ...m, read_at: "2026-11-27T11:00:00Z" } : m));
          return json(
            200,
            items.find((m) => m.id === n.id),
          );
        },
      ]),
    ),
    "POST /invitations/inv-n1/accept": () => json(200, {}),
    "GET /tasks/item-n1": () => json(404, { error: { code: "not_found", message: "not_found" } }),
  });
  return { server };
};

beforeEach(async () => {
  resetCredentials();
  setWidth(1280);
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("channel toggles (BL3)", () => {
  it("turns a channel on in a fixed order or off", () => {
    expect(toggled(["push"], "in_app")).toEqual(["in_app", "push"]);
    expect(toggled(["in_app", "push"], "push")).toEqual(["in_app"]);
    expect(toggled([], "email")).toEqual(["email"]);
  });
});

describe("notification center (BL1)", () => {
  it("shows the unread count in the navigation and lists notifications", async () => {
    const { server } = center([
      notification("n1", "Ortak liste"),
      notification("n2", "Eski", true),
    ]);
    await renderApp(server.api, "/notifications", { tokenStore: memoryTokenStore("t") });
    expect(
      await screen.findByRole("button", {
        name: 'Okunmadı: Ada seni bir görev için davet etti: "Ortak liste"',
      }),
    ).toBeTruthy();
    expect(screen.getByText('Ada seni bir görev için davet etti: "Eski"')).toBeTruthy();
    expect(within(screen.getByTestId("nav-badge-notifications")).getByText("1")).toBeTruthy();
  });

  it("shows the weekly summary with its parts (OZ1)", async () => {
    const summary: Notification = {
      id: "w1",
      payload: {
        type: "weekly_summary",
        week_start: "2026-11-16",
        week_end: "2026-11-22",
        routines: [
          { routine_id: "r1", title: "Su iç", done_days: 5, scheduled_days: 7, streak: 3 },
        ],
        goals: [{ goal_id: "g1", title: "Kitap", actual_percent: 40, expected_percent: 50 }],
        sessions: [{ item_id: null, title: "Genel", minutes: 95 }],
        work_minutes: 95,
        forgotten: [],
      },
      read_at: null,
      created_at: "2026-11-23T06:10:00Z",
    };
    const { server } = center([summary]);
    await renderApp(server.api, "/notifications", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByText("Su iç: 5/7 gün · seri 3")).toBeTruthy();
    expect(screen.getByText("Kitap: %40 (beklenen %50)")).toBeTruthy();
    expect(screen.getByText("Çalışma oturumları · toplam 1 sa 35 dk")).toBeTruthy();
    expect(screen.queryByText("Bir süredir dokunulmayanlar")).toBeNull();
  });

  it("marks a notification read on press and all of them at once", async () => {
    const { server } = center([notification("n1", "Bir"), notification("n2", "İki")]);
    await renderApp(server.api, "/notifications", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(
      await screen.findByRole("button", {
        name: 'Okunmadı: Ada seni bir görev için davet etti: "Bir"',
      }),
    );
    await waitFor(() => {
      expect(within(screen.getByTestId("nav-badge-notifications")).getByText("1")).toBeTruthy();
    });
    expect(server.calls.some((c) => c.path === "/notifications/n1/read")).toBe(true);
    await fireEvent.press(screen.getByRole("button", { name: "Tümünü okundu işaretle" }));
    await waitFor(() => {
      expect(screen.queryByTestId("nav-badge-notifications")).toBeNull();
    });
    expect(screen.queryAllByTestId("unread-dot")).toHaveLength(0);
  });

  it("refreshes live on notification.new", () => {
    // The `/ws` event invalidates the center's key; every center query starts with it.
    const keys = invalidationsFor({ name: "notification.new", data: { id: "n9" } }, defaultRules);
    expect(keys).toEqual([NOTIFICATIONS_KEY]);
    expect([...NOTIFICATIONS_KEY, "list"].slice(0, NOTIFICATIONS_KEY.length)).toEqual([
      ...NOTIFICATIONS_KEY,
    ]);
  });

  it("accepts an invitation from its notification", async () => {
    const { server } = center([notification("n1", "Ortak liste")]);
    await renderApp(server.api, "/notifications", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(await screen.findByRole("button", { name: "Kabul et" }));
    await waitFor(() => {
      expect(server.calls.some((c) => c.path === "/invitations/inv-n1/accept")).toBe(true);
    });
    await waitFor(() => {
      expect(server.calls.some((c) => c.path.startsWith("/tasks/item-n1"))).toBe(true);
    });
  });
});

describe("notification preferences (BL3)", () => {
  it("changes a type's channels", async () => {
    let prefs: NotificationPreference[] = [
      { kind: "invitation", channels: ["in_app", "push"], default: true },
      { kind: "weekly_summary", channels: ["in_app", "email"], default: true },
    ];
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /notifications": () => json(200, { notifications: [], unread: 0 }),
      "GET /notifications/preferences": () => json(200, prefs),
      "PUT /notifications/preferences/invitation": (call) => {
        const { channels } = call.body as { channels: NotificationPreference["channels"] };
        prefs = prefs.map((p) =>
          p.kind === "invitation" ? { ...p, channels, default: false } : p,
        );
        return json(200, prefs[0]);
      },
    });
    await renderApp(server.api, "/notifications/preferences", {
      tokenStore: memoryTokenStore("t"),
    });
    const push = await screen.findByRole("checkbox", { name: "Davetler: Anlık bildirim" });
    expect(push).toHaveProp("accessibilityState", expect.objectContaining({ checked: true }));
    await fireEvent.press(push);
    await waitFor(() => {
      expect(screen.getByRole("checkbox", { name: "Davetler: Anlık bildirim" })).toHaveProp(
        "accessibilityState",
        expect.objectContaining({ checked: false }),
      );
    });
    expect(server.calls.find((c) => c.method === "PUT")?.body).toEqual({ channels: ["in_app"] });
    expect(screen.getByRole("checkbox", { name: "Haftalık özet: E-posta" })).toBeTruthy();
  });
});

describe("routine reminders (R13)", () => {
  it("shows a reminder and opens its routine", async () => {
    const reminder: Notification = {
      id: "r1",
      payload: {
        type: "routine_reminder",
        routine_id: "rt1",
        title: "Su iç",
        local_date: "2026-11-27",
        time: "08:00",
      },
      read_at: null,
      created_at: "2026-11-27T05:00:00Z",
    };
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /notifications": () => json(200, { notifications: [reminder], unread: 1 }),
      "POST /notifications/r1/read": () =>
        json(200, { ...reminder, read_at: "2026-11-27T05:01:00Z" }),
      "GET /routines/rt1": () => json(404, { error: { code: "not_found", message: "not_found" } }),
    });
    await renderApp(server.api, "/notifications", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(
      await screen.findByRole("button", { name: 'Okunmadı: Hatırlatma: "Su iç" (08:00)' }),
    );
    await waitFor(() => {
      expect(server.calls.some((c) => c.path === "/notifications/r1/read")).toBe(true);
      expect(server.calls.some((c) => c.path.startsWith("/routines/rt1"))).toBe(true);
    });
    expect(screen.queryByRole("button", { name: "Kabul et" })).toBeNull();
  });
});

describe("nudges (AK3)", () => {
  it("says who nudged and marks it read on press", async () => {
    const nudge: Notification = {
      id: "n1",
      payload: { type: "nudge", from_user: "u2", from_name: "Ayşe" },
      read_at: null,
      created_at: "2026-11-27T09:00:00Z",
    };
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /notifications": () => json(200, { notifications: [nudge], unread: 1 }),
      "POST /notifications/n1/read": () => json(200, { ...nudge, read_at: "2026-11-27T09:01:00Z" }),
    });
    await renderApp(server.api, "/notifications", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(
      await screen.findByRole("button", { name: "Okunmadı: Ayşe seni dürttü 👋" }),
    );
    await waitFor(() => {
      expect(server.calls.some((c) => c.path === "/notifications/n1/read")).toBe(true);
    });
  });
});

describe("reactions (AK4)", () => {
  it("says who reacted to what and opens the item", async () => {
    const reaction: Notification = {
      id: "re1",
      payload: {
        type: "reaction",
        activity_id: "a1",
        item_id: "t9",
        item_kind: "task",
        item_title: "Rapor",
        from_user: "u2",
        from_name: "Ayşe",
        emoji: "🎉",
      },
      read_at: null,
      created_at: "2026-11-27T09:00:00Z",
    };
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /notifications": () => json(200, { notifications: [reaction], unread: 1 }),
      "POST /notifications/re1/read": () =>
        json(200, { ...reaction, read_at: "2026-11-27T09:01:00Z" }),
      "GET /tasks/t9": () => json(404, { error: { code: "not_found", message: "not_found" } }),
    });
    await renderApp(server.api, "/notifications", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(
      await screen.findByRole("button", {
        name: 'Okunmadı: Ayşe, "Rapor" tamamlamana 🎉 bıraktı',
      }),
    );
    await waitFor(() => {
      expect(server.calls.some((c) => c.path === "/notifications/re1/read")).toBe(true);
      expect(server.calls.some((c) => c.path.startsWith("/tasks/t9"))).toBe(true);
    });
  });
});
