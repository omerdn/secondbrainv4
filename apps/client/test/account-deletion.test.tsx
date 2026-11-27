import type { AccountDeletion } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import { longDate } from "@/features/account/AccountDeletionCard";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const none: AccountDeletion = { requested_at: null, deletes_at: null };
const pending: AccountDeletion = {
  requested_at: "2026-11-27T09:00:00Z",
  deletes_at: "2026-12-27T09:00:00Z",
};

const account = (initial: AccountDeletion) => {
  let deletion = initial;
  let shared = [
    {
      item_id: "i1",
      kind: "task",
      title: "Ortak rapor",
      heir_id: "u2",
      heir_name: "Bora",
    },
  ];
  return fakeServer({
    "GET /me": () => json(200, user),
    "GET /today": () => json(404, { error: { code: "not_found", message: "not_found" } }),
    "GET /account/deletion": () => json(200, deletion),
    "POST /account/deletion": () => {
      deletion = pending;
      return json(200, deletion);
    },
    "DELETE /account/deletion": () => {
      deletion = none;
      return json(200, deletion);
    },
    "GET /account/shared-items": () => json(200, shared),
    "PUT /items/i1/owner": () => {
      shared = [];
      return new Response(null, { status: 204 });
    },
  });
};

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("account deletion (HS2)", () => {
  it("formats the deletion day", () => {
    expect(longDate("2026-12-27T09:00:00Z", "tr")).toContain("2026");
  });

  it("asks for confirmation, shows the shared items and the day, and can be cancelled", async () => {
    const server = account(none);
    await renderApp(server.api, "/settings", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(await screen.findByText("Hesabımı sil"));
    expect(await screen.findByText(/30 gün sonra silinir/u)).toBeTruthy();
    expect(await screen.findByText("Ortak rapor")).toBeTruthy();
    expect(screen.getByText("Devralacak kişi: Bora")).toBeTruthy();
    expect(server.calls.some((c) => c.method === "POST")).toBe(false);
    await fireEvent.press(screen.getByText("Evet, hesabımı sil"));
    expect(await screen.findByText(/tarihinde silinecek/u)).toBeTruthy();
    await fireEvent.press(screen.getByText("Şimdi devret"));
    await waitFor(() => {
      expect(server.calls.find((c) => c.path === "/items/i1/owner")?.body).toEqual({
        user_id: "u2",
      });
    });
    await waitFor(() => {
      expect(screen.queryByText("Ortak rapor")).toBeNull();
    });
    await fireEvent.press(screen.getByText("Silmekten vazgeç"));
    expect(await screen.findByText("Hesabımı sil")).toBeTruthy();
  });
});
