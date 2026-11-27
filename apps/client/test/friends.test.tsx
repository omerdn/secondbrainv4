import type {
  Activity,
  ActivityReaction,
  BlockedUser,
  Friend,
  IncomingFriendRequest,
  OutgoingFriendRequest,
} from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor, within } from "expo-router/testing-library";
import { useWindowDimensions } from "react-native";

import { friendCompletions, reactionAction, reactionCount } from "@/features/friends/social";
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

jest.mock("react-native/Libraries/Utilities/useWindowDimensions", () => ({
  __esModule: true,
  default: jest.fn(),
}));

const at = "2026-11-27T09:00:00Z";

/** A fake social backend that follows what the screen does. */
const social = (extra: Record<string, (call: Call) => Response> = {}) => {
  let friends: Friend[] = [{ user_id: "u3", display_name: "Cem", since: at }];
  let incoming: IncomingFriendRequest[] = [
    { id: "r1", from_user: "u2", from_name: "Ayşe", created_at: at },
  ];
  let outgoing: OutgoingFriendRequest[] = [];
  let blocked: BlockedUser[] = [];
  const server = fakeServer({
    "GET /me": () => json(200, user),
    "GET /today": () => apiError(404, "not_found"),
    "GET /notifications": () => json(200, { notifications: [], unread: 0 }),
    "GET /friends": () => json(200, friends),
    "GET /friend-requests/incoming": () => json(200, incoming),
    "GET /friend-requests/outgoing": () => json(200, outgoing),
    "GET /blocks": () => json(200, blocked),
    "POST /friend-requests": (call) => {
      const { email } = call.body as { email: string };
      if (email === "tekrar@example.com") return apiError(409, "conflict");
      if (email === "kendim@example.com") return apiError(422, "validation_failed");
      const sent = {
        id: `o${outgoing.length}`,
        to_email: email.trim().toLowerCase(),
        created_at: at,
      };
      outgoing = [sent, ...outgoing];
      return json(201, sent);
    },
    "POST /friend-requests/r1/accept": () => {
      incoming = [];
      const friend = { user_id: "u2", display_name: "Ayşe", since: at };
      friends = [friend, ...friends];
      return json(200, friend);
    },
    "DELETE /friend-requests/o0": () => {
      outgoing = [];
      return json(204);
    },
    "DELETE /friends/u3": () => {
      friends = friends.filter((f) => f.user_id !== "u3");
      return json(204);
    },
    "PUT /blocks/u3": () => {
      friends = friends.filter((f) => f.user_id !== "u3");
      blocked = [{ user_id: "u3", display_name: "Cem", since: at }];
      return json(204);
    },
    "DELETE /blocks/u3": () => {
      blocked = [];
      return json(204);
    },
    ...extra,
  });
  return server;
};

const open = (server: ReturnType<typeof fakeServer>) =>
  renderApp(server.api, "/friends", { tokenStore: memoryTokenStore("t") });

beforeEach(async () => {
  resetCredentials();
  jest
    .mocked(useWindowDimensions)
    .mockReturnValue({ width: 1280, height: 900, scale: 2, fontScale: 1 });
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("friends screen (AK1)", () => {
  it("shows the request badge and accepts a request", async () => {
    const server = social();
    await open(server);
    expect(await screen.findByText("Ayşe seninle arkadaş olmak istiyor.")).toBeTruthy();
    expect(within(screen.getByTestId("nav-badge-friends")).getByText("1")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Ayşe isteğini kabul et" }));
    expect(await screen.findByRole("button", { name: "Ayşe profilini aç" })).toBeTruthy();
    await waitFor(() => {
      expect(screen.queryByTestId("nav-badge-friends")).toBeNull();
    });
  });

  it("sends a request by e-mail and takes it back", async () => {
    const server = social();
    await open(server);
    await fireEvent.changeText(await screen.findByLabelText("E-posta adresi"), "Bob@Example.com");
    await fireEvent.press(screen.getByRole("button", { name: "İstek gönder" }));
    expect(await screen.findByText("İstek gönderildi.")).toBeTruthy();
    expect(await screen.findByText("bob@example.com")).toBeTruthy();
    expect(server.calls.find((c) => c.method === "POST")?.body).toEqual({
      email: "Bob@Example.com",
    });
    await fireEvent.press(
      screen.getByRole("button", { name: "bob@example.com isteğini geri çek" }),
    );
    await waitFor(() => {
      expect(screen.queryByText("bob@example.com")).toBeNull();
    });
  });

  it("explains a refused request in Turkish", async () => {
    const server = social();
    await open(server);
    await fireEvent.changeText(
      await screen.findByLabelText("E-posta adresi"),
      "tekrar@example.com",
    );
    await fireEvent.press(screen.getByRole("button", { name: "İstek gönder" }));
    expect(
      await screen.findByText("Bu adrese yakın zamanda istek gönderdin ya da zaten arkadaşsınız."),
    ).toBeTruthy();
    await fireEvent.changeText(screen.getByLabelText("E-posta adresi"), "kendim@example.com");
    await fireEvent.press(screen.getByRole("button", { name: "İstek gönder" }));
    expect(
      await screen.findByText("Bu adrese istek gönderilemez. Adresi kontrol et."),
    ).toBeTruthy();
  });

  it("unfriends from the profile card", async () => {
    const server = social();
    await open(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Cem profilini aç" }));
    await fireEvent.press(screen.getByRole("button", { name: "Arkadaşlıktan çıkar" }));
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Cem profilini aç" })).toBeNull();
    });
    expect(server.calls.some((c) => c.method === "DELETE" && c.path === "/friends/u3")).toBe(true);
  });

  it("blocks from the profile card and unblocks", async () => {
    const server = social();
    await open(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Cem profilini aç" }));
    await fireEvent.press(screen.getByRole("button", { name: "Engelle" }));
    expect(await screen.findByRole("button", { name: "Cem engelini kaldır" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Cem profilini aç" })).toBeNull();
    await fireEvent.press(screen.getByRole("button", { name: "Cem engelini kaldır" }));
    await waitFor(() => {
      expect(screen.queryByText("Engellenenler")).toBeNull();
    });
  });
});

const completion = (over: Partial<Activity> = {}): Activity => ({
  id: "a1",
  actor_id: "u3",
  item_id: "t1",
  item_kind: "task",
  item_title: "Rapor",
  kind: "completed",
  related_item_id: null,
  member_user_id: null,
  member_role: null,
  created_at: at,
  reactions: [],
  ...over,
});

describe("social model (AK4)", () => {
  it("keeps friends' completions and entries only and toggles one's own reaction", () => {
    const feed = [
      completion(),
      completion({ id: "a2", actor_id: "u1" }),
      completion({ id: "a3", actor_id: "stranger" }),
      completion({ id: "a4", kind: "created" }),
      completion({ id: "a5", kind: "recorded", item_kind: "tracker" }),
    ];
    expect(friendCompletions(feed, new Set(["u3"]), "u1").map((e) => e.id)).toEqual(["a1", "a5"]);
    const mine = [{ emoji: "🎉", count: 2, mine: true }];
    expect(reactionAction(mine, "🎉")).toBe("unreact");
    expect(reactionAction(mine, "🔥")).toBe("react");
    expect(reactionCount(mine, "🎉")).toBe(2);
    expect(reactionCount(mine, "👏")).toBe(0);
  });
});

describe("nudges and reactions (AK3, AK4)", () => {
  const withSocial = (nudgeStatus = 204) => {
    let reactions: ActivityReaction[] = [];
    return social({
      "GET /activity": () => json(200, { items: [completion({ reactions })], next_cursor: null }),
      "PUT /activity/a1/reaction": (call) => {
        const { emoji } = call.body as { emoji: string };
        reactions = [{ emoji, count: 1, mine: true }];
        return json(200, reactions);
      },
      "DELETE /activity/a1/reaction": () => {
        reactions = [];
        return json(200, reactions);
      },
      "POST /friends/u3/nudges": () =>
        nudgeStatus === 204 ? json(204) : apiError(nudgeStatus, "rate_limited"),
    });
  };

  it("nudges a friend from the profile card and explains a second nudge", async () => {
    const server = withSocial(429);
    await open(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Cem profilini aç" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Cem kişisini dürt" }));
    expect(
      await screen.findByText("Bugün zaten dürttün; yarın tekrar deneyebilirsin."),
    ).toBeTruthy();
    expect(server.calls.some((c) => c.method === "POST" && c.path === "/friends/u3/nudges")).toBe(
      true,
    );
  });

  it("reacts to a friend's completion and takes it back", async () => {
    const server = withSocial();
    await open(server);
    expect(await screen.findByText('Cem, "Rapor" tamamladı')).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: '"Rapor" için 🎉 tepkisi' }));
    await waitFor(() => {
      expect(server.calls.find((c) => c.method === "PUT")?.body).toEqual({ emoji: "🎉" });
    });
    expect(await screen.findByText("🎉 1")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: '"Rapor" için 🎉 tepkisi' }));
    await waitFor(() => {
      expect(
        server.calls.some((c) => c.method === "DELETE" && c.path === "/activity/a1/reaction"),
      ).toBe(true);
    });
  });
});
