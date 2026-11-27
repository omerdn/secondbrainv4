import type { Friend, Item, ItemMembers } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import { invitable, rolesFor } from "@/features/items/members";
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

const at = "2026-11-27T09:00:00Z";
const task = (owner: string): Item => ({
  id: "t1",
  kind: "task",
  title: "Ortak liste",
  description: null,
  owner: { type: "user", user_id: owner },
  created_by: owner,
  created_at: at,
  updated_at: at,
  last_touched_at: at,
  archived_at: null,
  deleted_at: null,
});
const friends: Friend[] = [
  { user_id: "u2", display_name: "Ayşe", since: at },
  { user_id: "u3", display_name: "Can", since: at },
];

/** A fake item backend whose members follow the screen. */
const backend = (owner: string, initial: ItemMembers) => {
  let members = initial;
  const server = fakeServer({
    "GET /me": () => json(200, user),
    "GET /items/t1": () => json(200, task(owner)),
    "GET /access/items/t1/explanation": () =>
      json(200, { permissions: ["view"], reasons: [{ kind: "owner" }] }),
    "GET /items/t1/tags": () => json(200, []),
    "GET /items/t1/links": () => json(200, []),
    "GET /items/t1/members": () => json(200, members),
    "GET /friends": () => json(200, friends),
    "POST /access/preview": () =>
      json(200, {
        items: [{ item_id: "t1", new_viewers: ["u3"] }],
        people: [{ id: "u3", display_name: null }],
      }),
    "POST /items/t1/invitations": (call) => {
      const body = call.body as { to_user: string; role: "assignee" | "watcher" };
      members = {
        ...members,
        pending: [
          ...members.pending,
          { invitation_id: "inv1", user_id: body.to_user, display_name: "Can", role: body.role },
        ],
      };
      return json(201, {});
    },
    "DELETE /items/t1/members/u2": () => {
      members = { ...members, members: members.members.filter((m) => m.user_id !== "u2") };
      return json(204);
    },
    "DELETE /items/t1/members/u1": () => {
      members = { ...members, members: members.members.filter((m) => m.user_id !== "u1") };
      return json(204);
    },
  });
  return server;
};

const owned: ItemMembers = {
  members: [
    { user_id: "u1", display_name: "Ali", roles: ["owner"] },
    { user_id: "u2", display_name: "Ayşe", roles: ["assignee"] },
  ],
  pending: [],
  can_manage: true,
};

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("member rules", () => {
  it("offers the roles the server allows and only friends not already in", () => {
    expect(rolesFor("task")).toEqual(["assignee", "watcher"]);
    expect(rolesFor("notebook")).toEqual(["editor", "viewer"]);
    expect(rolesFor("page")).toEqual([]);
    expect(invitable(friends, owned).map((f) => f.user_id)).toEqual(["u3"]);
  });
});

describe("member management (N2, V5, V7)", () => {
  it("invites a friend only after the access preview is confirmed", async () => {
    const server = backend("u1", owned);
    await renderApp(server.api, "/items/t1", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByText("Ali (sen)")).toBeTruthy();
    expect(screen.getByText("Sorumlu")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Üye ekle" }));
    await fireEvent.press(await screen.findByRole("radio", { name: "İzleyici" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Can kişisini davet et" }));
    // The preview names the person the screen picked; nothing is sent before confirming.
    expect(await screen.findByText("Can")).toBeTruthy();
    expect(server.calls.some((c) => c.path === "/items/t1/invitations")).toBe(false);
    expect(server.calls.find((c) => c.path === "/access/preview")?.body).toEqual({
      action: "add_member",
      item_id: "t1",
      user_id: "u3",
      role: "watcher",
    });
    await fireEvent.press(screen.getByRole("button", { name: "Davet et" }));
    expect(await screen.findByText("Davet bekliyor · İzleyici")).toBeTruthy();
    expect(server.calls.find((c) => c.path === "/items/t1/invitations")?.body).toEqual({
      to_user: "u3",
      role: "watcher",
    });
  });

  it("does not invite when the preview is cancelled", async () => {
    const server = backend("u1", owned);
    await renderApp(server.api, "/items/t1", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(await screen.findByRole("button", { name: "Üye ekle" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Can kişisini davet et" }));
    await screen.findByText("Can");
    await fireEvent.press(screen.getByRole("button", { name: "Vazgeç" }));
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Davet et" })).toBeNull();
    });
    expect(server.calls.some((c) => c.path === "/items/t1/invitations")).toBe(false);
  });

  it("lets the owner remove a member", async () => {
    const server = backend("u1", owned);
    await renderApp(server.api, "/items/t1", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(await screen.findByRole("button", { name: "Ayşe kişisini çıkar" }));
    await waitFor(() => {
      expect(screen.queryByText("Ayşe")).toBeNull();
    });
  });

  it("shows a member only how to leave", async () => {
    const asMember: ItemMembers = {
      members: [
        { user_id: "u2", display_name: "Ayşe", roles: ["owner"] },
        { user_id: "u1", display_name: "Ali", roles: ["watcher"] },
      ],
      pending: [],
      can_manage: false,
    };
    const server = backend("u2", asMember);
    await renderApp(server.api, "/items/t1", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByText("Ali (sen)")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Üye ekle" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Ayşe kişisini çıkar" })).toBeNull();
    await fireEvent.press(screen.getByRole("button", { name: "Ayrıl" }));
    await waitFor(() => {
      expect(
        server.calls.some((c) => c.method === "DELETE" && c.path === "/items/t1/members/u1"),
      ).toBe(true);
    });
  });

  it("refuses nothing it cannot show: a failed list hides the section", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /items/t1": () => json(200, task("u1")),
      "GET /access/items/t1/explanation": () => apiError(404, "not_found"),
      "GET /items/t1/tags": () => json(200, []),
      "GET /items/t1/links": () => json(200, []),
      "GET /items/t1/members": () => apiError(500, "internal"),
    });
    await renderApp(server.api, "/items/t1", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByRole("header", { name: "Ortak liste" })).toBeTruthy();
    expect(screen.queryByText("Üyeler")).toBeNull();
  });
});
