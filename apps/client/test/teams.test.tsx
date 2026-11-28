import type { NotebookSummary, Tag, Team, TeamDetail, TeamMatrix } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

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

const at = "2026-11-28T09:00:00Z";

const team = (role: Team["role"], over: Partial<Team> = {}): Team => ({
  id: "t1",
  name: "Ekip",
  color: "#16a34a",
  logo_url: null,
  created_at: at,
  role,
  ...over,
});

const detail = (role: Team["role"]): TeamDetail => ({
  team: team(role),
  members: [
    { user_id: "u1", display_name: "Ali", role, active: true },
    { user_id: "u2", display_name: "Ayşe", role: "member", active: true },
    { user_id: "u3", display_name: "Mehmet", role: "manager", active: true },
    { user_id: "u4", display_name: "Deniz", role: "member", active: false },
  ],
});

const matrix: TeamMatrix = {
  roles: [
    { role: "admin", item: ["view", "edit", "share", "link", "manage", "delete"], team: [] },
    { role: "manager", item: ["view", "edit", "share", "link", "manage"], team: ["assign"] },
    { role: "member", item: ["view", "edit"], team: ["create_items"] },
    { role: "guest", item: ["view"], team: [] },
  ],
};

const teamTag: Tag = {
  id: "tag1",
  team_id: "t1",
  name: "Sprint",
  color: null,
  created_at: at,
  updated_at: at,
};

const notebook: NotebookSummary = {
  item: {
    id: "n1",
    kind: "notebook",
    title: "Toplantılar",
    description: null,
    owner: { type: "team", team_id: "t1" },
    team_visibility: "assignees",
    created_by: "u1",
    created_at: at,
    updated_at: at,
    last_touched_at: at,
    archived_at: null,
    deleted_at: null,
  },
  is_default: false,
  shared: false,
};

const writes = (calls: Call[]) =>
  calls.filter((call) => call.method !== "GET").map((call) => `${call.method} ${call.path}`);

function server(role: Team["role"], extra: Record<string, (call: Call) => Response> = {}) {
  return fakeServer({
    "GET /me": () => json(200, user),
    "GET /teams": () => json(200, [team(role)]),
    "GET /team-invitations": () => json(200, []),
    "GET /teams/t1": () => json(200, detail(role)),
    "GET /teams/t1/matrix": () => json(200, matrix),
    "GET /tags": () => json(200, [teamTag]),
    "GET /notebooks": () => json(200, [notebook]),
    ...extra,
  });
}

const open = (s: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(s.api, url, { tokenStore: memoryTokenStore("t") });

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("teams list (TK1, TK2, TK7)", () => {
  it("lists the teams; settings only for admins and managers", async () => {
    await open(server("member"), "/teams");
    expect(await screen.findByText("# Ekip")).toBeTruthy();
    expect(screen.getByText("Üye")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Ekip ayarları" })).toBeNull();
    expect(screen.getByRole("button", { name: "Ekip takımından ayrıl" })).toBeTruthy();
  });

  it("joins a team from an invitation", async () => {
    const s = server("admin", {
      "GET /team-invitations": () =>
        json(200, [
          { team_id: "t2", team_name: "Ofis", role: "member", invited_by: "u9", invited_at: at },
        ]),
      "POST /teams/t2/join": () => json(200, team("member", { id: "t2", name: "Ofis" })),
    });
    await open(s, "/teams");
    expect(await screen.findByText("Ofis takımına Üye olarak davet edildin.")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Katıl" }));
    await waitFor(() => {
      expect(writes(s.calls)).toEqual(["POST /teams/t2/join"]);
    });
  });

  it("asks before leaving and explains the last admin", async () => {
    const s = server("admin", {
      "POST /teams/t1/leave": () => apiError(422, "validation_failed"),
    });
    await open(s, "/teams");
    await fireEvent.press(await screen.findByRole("button", { name: "Ekip takımından ayrıl" }));
    expect(writes(s.calls)).toEqual([]);
    await fireEvent.press(screen.getByRole("button", { name: "Evet, ayrıl" }));
    expect(
      await screen.findByText("Takımın son admini ayrılamaz ya da rolünü bırakamaz."),
    ).toBeTruthy();
    expect(writes(s.calls)).toEqual(["POST /teams/t1/leave"]);
  });

  it("creates a team with a color", async () => {
    const s = server("admin", {
      "POST /teams": (call) => json(201, team("admin", call.body as Partial<Team>)),
    });
    await open(s, "/teams");
    await screen.findByText("# Ekip");
    const create = () => screen.getAllByRole("button", { name: "Takım oluştur" });
    await fireEvent.changeText(screen.getByLabelText("Takım adı"), " Ofis ");
    await fireEvent.press(screen.getByRole("button", { name: "#0ea5e9" }));
    await fireEvent.press(create()[0]!);
    await waitFor(() => {
      expect(s.calls.find((call) => call.method === "POST")?.body).toEqual({
        name: "Ofis",
        color: "#0ea5e9",
      });
    });
  });
});

describe("team settings (TK5)", () => {
  it("is not shown to members", async () => {
    await open(server("member"), "/teams/t1");
    expect(
      await screen.findByText("Takım ayarlarını yalnız takımın admin ve yöneticileri görür."),
    ).toBeTruthy();
    expect(screen.queryByText("Üyeler")).toBeNull();
  });

  it("is not shown outside the team", async () => {
    await open(server("admin", { "GET /teams/t1": () => apiError(404, "not_found") }), "/teams/t1");
    expect(
      await screen.findByText("Takım ayarlarını yalnız takımın admin ve yöneticileri görür."),
    ).toBeTruthy();
  });

  it("lets an admin change roles, remove members and invite by e-mail", async () => {
    const s = server("admin", {
      "PUT /teams/t1/members/u2/role": () => new Response(null, { status: 204 }),
      "DELETE /teams/t1/members/u3": () => new Response(null, { status: 204 }),
      "POST /teams/t1/invitations/by-email": () => new Response(null, { status: 204 }),
    });
    await open(s, "/teams/t1");
    expect(await screen.findByText("Üyeler")).toBeTruthy();
    expect(screen.getByText("Davet bekliyor")).toBeTruthy();
    // Ayşe's row: the role chips follow her name.
    const managerChips = screen.getAllByRole("button", { name: "Yönetici" });
    await fireEvent.press(managerChips[1]!);
    await fireEvent.press(
      screen.getByRole("button", { name: "Mehmet adlı kişiyi takımdan çıkar" }),
    );
    await fireEvent.changeText(
      screen.getByLabelText("Davet edilecek e-posta adresi"),
      " can@example.com ",
    );
    await fireEvent.press(screen.getByRole("button", { name: "Davet et" }));
    expect(await screen.findByText("Bu adresin bir hesabı varsa davet gönderildi.")).toBeTruthy();
    expect(writes(s.calls)).toEqual([
      "PUT /teams/t1/members/u2/role",
      "DELETE /teams/t1/members/u3",
      "POST /teams/t1/invitations/by-email",
    ]);
    const invite = s.calls.find((call) => call.path.endsWith("by-email"));
    expect(invite?.body).toEqual({ email: "can@example.com", role: "member" });
    expect((s.calls.find((call) => call.method === "PUT") as Call).body).toEqual({
      role: "manager",
    });
  });

  it("lets a manager remove members but not managers, nor change roles", async () => {
    await open(server("manager"), "/teams/t1");
    expect(await screen.findByText("Üyeler")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Ayşe adlı kişiyi takımdan çıkar" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Mehmet adlı kişiyi takımdan çıkar" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Misafir" })).toBeTruthy(); // invite role only
    expect(await screen.findByText("Yetkileri yalnız adminler değiştirir.")).toBeTruthy();
  });

  it("edits the permission matrix (TK3)", async () => {
    const s = server("admin", {
      "PUT /teams/t1/matrix": (call) => json(200, call.body),
    });
    await open(s, "/teams/t1");
    expect(await screen.findByText("Yetkiler")).toBeTruthy();
    // "Bağlama" (link) chips in role order: manager, member, guest.
    const link = await screen.findAllByRole("button", { name: "Bağlama" });
    await fireEvent.press(link[1]!);
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
    await waitFor(() => {
      expect(writes(s.calls)).toEqual(["PUT /teams/t1/matrix"]);
    });
    const saved = s.calls.find((call) => call.method === "PUT")?.body as TeamMatrix;
    expect(saved.roles.find((row) => row.role === "member")?.item).toEqual([
      "view",
      "edit",
      "link",
    ]);
  });

  it("manages team tags and notebooks (E3, Y2)", async () => {
    const s = server("admin", {
      "POST /teams/t1/tags": (call) =>
        json(201, { ...teamTag, id: "tag2", ...(call.body as object) }),
      "DELETE /tags/tag1": () => new Response(null, { status: 204 }),
      "POST /teams/t1/notebooks": (call) =>
        json(201, {
          item: { ...notebook.item, ...(call.body as object) },
          is_default: false,
          pages: [],
        }),
    });
    await open(s, "/teams/t1");
    expect(await screen.findByText("# Sprint")).toBeTruthy();
    expect(await screen.findByText("Toplantılar")).toBeTruthy();
    expect(screen.getAllByText("Seçili kişiler").length).toBeGreaterThan(0);
    await fireEvent.press(screen.getByRole("button", { name: "Sprint etiketini sil" }));
    await fireEvent.changeText(screen.getByLabelText("Yeni takım etiketi"), "Acil");
    await fireEvent.press(screen.getByRole("button", { name: "Etiket ekle" }));
    await fireEvent.changeText(screen.getByLabelText("Yeni takım defteri"), "Planlar");
    const chosen = screen.getAllByRole("button", { name: "Seçili kişiler" });
    await fireEvent.press(chosen[chosen.length - 1]!);
    await fireEvent.press(screen.getByRole("button", { name: "Defter oluştur" }));
    await waitFor(() => {
      expect(writes(s.calls)).toEqual([
        "DELETE /tags/tag1",
        "POST /teams/t1/tags",
        "POST /teams/t1/notebooks",
      ]);
    });
    const created = s.calls.find((call) => call.path === "/teams/t1/notebooks");
    expect(created?.body).toEqual({ title: "Planlar", visibility: "assignees" });
  });
});

describe("team dashboard (TK6)", () => {
  const task = (id: string, title: string) => ({ ...notebook.item, id, kind: "task", title });
  const board = {
    today: "2026-11-28",
    goals: [
      {
        item: { ...task("g1", "Çeyrek"), kind: "goal" },
        start_date: "2026-11-01",
        end_date: "2026-12-31",
        actual: 0.4,
        expected: 0.5,
        pace: 0.8,
      },
    ],
    routines: [{ item: { ...task("r1", "Stand-up"), kind: "routine" }, scheduled: 7, complete: 5 }],
    adherence_days: 7,
    overdue: [{ item: task("k1", "Rapor"), due_date: "2026-11-24", done: 1, total: 3 }],
  };
  const event = {
    id: "e1",
    actor_id: "u2",
    item_id: "k1",
    item_kind: "task",
    item_title: "Rapor",
    kind: "completed",
    related_item_id: null,
    member_user_id: null,
    member_role: null,
    created_at: at,
    reactions: [],
  };

  it("opens from the list for every member", async () => {
    const s = server("member", {
      "GET /teams/t1/dashboard": () => json(200, board),
      "GET /activity": () => json(200, { items: [], next_cursor: null }),
    });
    await open(s, "/teams");
    await fireEvent.press(await screen.findByRole("button", { name: "Ekip panosu" }));
    expect(await screen.findByText("Aktif hedefler")).toBeTruthy();
  });

  it("shows goals, routine adherence, overdue tasks and the team's activity", async () => {
    const s = server("member", {
      "GET /teams/t1/dashboard": () => json(200, board),
      "GET /activity": () => json(200, { items: [event], next_cursor: null }),
    });
    await open(s, "/teams/t1/dashboard");
    expect(await screen.findByText("Gerçekleşen %40 · tempo %80")).toBeTruthy();
    expect(screen.getByText("Rutin uyumu (son 7 gün)")).toBeTruthy();
    expect(screen.getByText("5/7 gün tamamlandı")).toBeTruthy();
    expect(screen.getByText("Son tarih 2026-11-24 · 1/3 alt görev")).toBeTruthy();
    expect(await screen.findByText("Ayşe, Rapor nesnesini tamamladı")).toBeTruthy();
    const feed = s.calls.find((call) => call.path.startsWith("/activity"));
    const filter = new URL(`http://x${feed?.path ?? ""}`).searchParams.get("filter");
    expect(JSON.parse(filter ?? "null")).toEqual({ team_ids: ["t1"] });
    await fireEvent.press(screen.getByRole("link", { name: "Rapor" }));
    await waitFor(() => {
      expect(s.calls.some((call) => call.path === "/tasks/k1")).toBe(true);
    });
  });

  it("says when there is nothing, and shows nothing outside the team", async () => {
    const empty = { ...board, goals: [], routines: [], overdue: [] };
    await open(
      server("member", {
        "GET /teams/t1/dashboard": () => json(200, empty),
        "GET /activity": () => json(200, { items: [], next_cursor: null }),
      }),
      "/teams/t1/dashboard",
    );
    expect(await screen.findByText("Takımın aktif hedefi yok.")).toBeTruthy();
    expect(screen.getByText("Geciken görev yok.")).toBeTruthy();
  });

  it("is not shown to someone outside the team", async () => {
    await open(
      server("member", {
        "GET /teams/t1": () => apiError(404, "not_found"),
        "GET /teams/t1/dashboard": () => json(200, board),
        "GET /activity": () => json(200, { items: [event], next_cursor: null }),
      }),
      "/teams/t1/dashboard",
    );
    expect(await screen.findByText("Bu panoyu yalnız takımın üyeleri görür.")).toBeTruthy();
    expect(screen.queryByText("Rapor")).toBeNull();
  });
});
