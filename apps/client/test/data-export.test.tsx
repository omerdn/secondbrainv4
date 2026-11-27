import type { DataExport } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import { downloadUrl } from "@/features/account/DataExportCard";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const base: DataExport = {
  id: "e1",
  status: "pending",
  created_at: "2026-11-27T09:00:00Z",
  expires_at: "2026-11-28T09:00:00Z",
  json_path: null,
  markdown_path: null,
};
const ready: DataExport = {
  ...base,
  status: "ready",
  json_path: "/exports/tok/routune.json",
  markdown_path: "/exports/tok/routune.md",
};

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("data export (HS3)", () => {
  it("builds download addresses on the API", () => {
    expect(downloadUrl("/exports/tok/routune.md", "https://api.test/")).toBe(
      "https://api.test/exports/tok/routune.md",
    );
  });

  it("prepares an export and offers both files once it is ready", async () => {
    let current: DataExport | null = null;
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /today": () => json(404, { error: { code: "not_found", message: "not_found" } }),
      "GET /account/deletion": () => json(200, { requested_at: null, deletes_at: null }),
      "GET /account/exports": () => json(200, current),
      "POST /account/exports": () => {
        current = base;
        return json(202, base);
      },
    });
    await renderApp(server.api, "/settings", { tokenStore: memoryTokenStore("t") });
    await fireEvent.press(await screen.findByText("Dışa aktarmayı hazırla"));
    expect(await screen.findByText("Hazırlanıyor…")).toBeTruthy();
    current = ready;
    await waitFor(
      () => {
        expect(screen.getByText("JSON indir")).toBeTruthy();
      },
      { timeout: 6000 },
    );
    expect(screen.getByText("Markdown indir")).toBeTruthy();
  });
});
