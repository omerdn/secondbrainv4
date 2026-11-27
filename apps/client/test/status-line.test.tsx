import type { Item } from "@secondbrain/api-types";
import { render } from "@testing-library/react-native";
import { act, screen } from "expo-router/testing-library";

import { StatusLineText, daysBetween, localToday } from "@/lib/items";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const at = "2026-11-26T12:00:00Z";
const goal = (id: string, title: string): Item => ({
  id,
  kind: "goal",
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

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("latest status line (S3)", () => {
  it("counts whole local days", () => {
    expect(daysBetween("2026-11-23", "2026-11-26")).toBe(3);
    expect(daysBetween("2026-11-26", "2026-11-26")).toBe(0);
    expect(localToday(new Date(2026, 0, 5, 23, 30))).toBe("2026-01-05");
  });

  it("reads like a log line", async () => {
    await render(
      <StatusLineText line={{ text: "Barre'da kaldım", date: "2026-11-23" }} today="2026-11-26" />,
    );
    expect(screen.getByText("Barre'da kaldım · 3 gün önce")).toBeTruthy();
    await render(<StatusLineText line={{ text: "Dün", date: "2026-11-25" }} today="2026-11-26" />);
    expect(screen.getByText("Dün · dün")).toBeTruthy();
    await render(<StatusLineText line={{ text: "Elle yazılan", date: null }} />);
    expect(screen.getByText("Elle yazılan")).toBeTruthy();
  });

  it("shows on cards with one request for the whole list, only where the server gives one", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /items/filter": () =>
        json(200, { items: [goal("g1", "Kitap yılı"), goal("g2", "Koşu")], next_cursor: null }),
      "GET /process-lines": () =>
        json(200, [{ item_id: "g1", line: { text: "12. kitaptayım", date: null } }]),
    });
    await renderApp(server.api, "/goals", { tokenStore: memoryTokenStore("t") });
    expect(await screen.findByText("12. kitaptayım")).toBeTruthy();
    const calls = server.calls.filter((call) => call.path.startsWith("/process-lines"));
    expect(calls).toHaveLength(1);
    expect(calls[0]?.path).toContain("g1");
    // The other card has no line: nothing is invented for it.
    expect(screen.getAllByTestId("status-line")).toHaveLength(1);
  });
});
