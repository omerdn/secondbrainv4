import type { Item, Tag } from "@secondbrain/api-types";
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

const at = "2026-05-01T12:00:00Z";

const tag = (id: string, name: string): Tag => ({
  id,
  name,
  color: null,
  created_at: at,
  updated_at: at,
});

const item = (id: string, title: string): Item => ({
  id,
  kind: "task",
  title,
  description: null,
  owner: { type: "user", user_id: "u1" },
  created_by: "u1",
  created_at: at,
  updated_at: at,
  last_touched_at: at,
  archived_at: null,
  deleted_at: null,
});

const open = (server: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(server.api, url, { tokenStore: memoryTokenStore("t") });

const mutations = (calls: Call[]) =>
  calls.filter((call) => call.method !== "GET").map((call) => `${call.method} ${call.path}`);

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("tags screen (E4)", () => {
  const routes = {
    "GET /me": () => json(200, user),
    "GET /tags": () => json(200, [tag("a", "İş"), tag("b", "Ev")]),
    "GET /tags/a/items": () => json(200, [item("i1", "Rapor yaz")]),
    "GET /tags/b/items": () => json(200, []),
    "GET /items/i1": () => json(200, item("i1", "Rapor yaz")),
    "GET /access/items/i1/explanation": () => json(200, { permissions: [], reasons: [] }),
    "GET /items/i1/tags": () => json(200, []),
    "GET /items/i1/links": () => json(200, []),
  };

  it("lists the tags and asks for a choice", async () => {
    await open(fakeServer(routes), "/tags");
    expect(await screen.findByRole("button", { name: "İş" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Ev" })).toBeTruthy();
    expect(screen.getByText("Nesnelerini görmek için bir etiket seç.")).toBeTruthy();
  });

  it("shows the items of the chosen tag, and clears on a second press", async () => {
    await open(fakeServer(routes), "/tags");
    await fireEvent.press(await screen.findByRole("button", { name: "İş" }));
    expect(await screen.findByRole("button", { name: "Görev: Rapor yaz" })).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Ev" }));
    expect(await screen.findByText("Bu etikette nesne yok")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Ev" }));
    expect(await screen.findByText("Nesnelerini görmek için bir etiket seç.")).toBeTruthy();
  });

  it("opens an item from the list", async () => {
    await open(fakeServer(routes), "/tags?tag=a");
    await fireEvent.press(await screen.findByRole("button", { name: "Görev: Rapor yaz" }));
    expect(await screen.findByRole("header", { name: "Rapor yaz" })).toBeTruthy();
  });

  it("explains an empty list", async () => {
    await open(
      fakeServer({ "GET /me": () => json(200, user), "GET /tags": () => json(200, []) }),
      "/tags",
    );
    expect(await screen.findByText("Henüz etiketin yok")).toBeTruthy();
  });

  it("offers a retry when the list fails", async () => {
    let fail = true;
    await open(
      fakeServer({
        "GET /me": () => json(200, user),
        "GET /tags": () => (fail ? apiError(403, "forbidden") : json(200, [tag("a", "İş")])),
      }),
      "/tags",
    );
    const retry = await screen.findByRole("button", { name: "Tekrar dene" });
    fail = false;
    await fireEvent.press(retry);
    expect(await screen.findByRole("button", { name: "İş" })).toBeTruthy();
  });
});

describe("tag picker", () => {
  function picker(extra: Record<string, () => Response> = {}) {
    let attached: Tag[] = [tag("a", "İş")];
    const all = [tag("a", "İş"), tag("b", "Ev")];
    return fakeServer({
      "GET /me": () => json(200, user),
      "GET /items/i1": () => json(200, item("i1", "Rapor yaz")),
      "GET /access/items/i1/explanation": () =>
        json(200, { permissions: ["view"], reasons: [{ kind: "owner" }] }),
      "GET /items/i1/links": () => json(200, []),
      "GET /items/i1/tags": () => json(200, attached),
      "GET /tags": () => json(200, all),
      "PUT /items/i1/tags/b": () => {
        attached = [...attached, tag("b", "Ev")];
        return new Response(null, { status: 204 });
      },
      "DELETE /items/i1/tags/a": () => {
        attached = attached.filter((t) => t.id !== "a");
        return new Response(null, { status: 204 });
      },
      "POST /tags": () => {
        all.push(tag("c", "Yeni"));
        return json(201, tag("c", "Yeni"));
      },
      "PUT /items/i1/tags/c": () => {
        attached = [...attached, tag("c", "Yeni")];
        return new Response(null, { status: 204 });
      },
      ...extra,
    });
  }

  const openPicker = async (server: ReturnType<typeof fakeServer>) => {
    await open(server, "/items/i1");
    await fireEvent.press(await screen.findByRole("button", { name: "Etiketleri düzenle" }));
    await screen.findByRole("header", { name: "Etiketler" });
  };

  it("attaches and detaches with an idempotency key", async () => {
    const server = picker();
    await openPicker(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Ev" }));
    await waitFor(() => {
      expect(mutations(server.calls)).toEqual(["PUT /items/i1/tags/b"]);
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Ev", selected: true })).toBeTruthy();
    });
    await fireEvent.press(screen.getByRole("button", { name: "İş", selected: true }));
    await waitFor(() => {
      expect(mutations(server.calls)).toContain("DELETE /items/i1/tags/a");
    });
    const writes = server.calls.filter((call) => call.method !== "GET");
    for (const call of writes) {
      expect(call.headers["Idempotency-Key"] ?? call.headers["idempotency-key"]).toBeTruthy();
    }
  });

  it("creates a tag and attaches it", async () => {
    const server = picker();
    await openPicker(server);
    await fireEvent.changeText(screen.getByLabelText("Yeni etiket"), "  Yeni ");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur ve tak" }));
    await waitFor(() => {
      expect(mutations(server.calls)).toEqual(["POST /tags", "PUT /items/i1/tags/c"]);
    });
    expect(server.calls.find((call) => call.method === "POST")?.body).toEqual({ name: "Yeni" });
  });

  it("explains a taken name", async () => {
    const server = picker({ "POST /tags": () => apiError(409, "conflict") });
    await openPicker(server);
    await fireEvent.changeText(screen.getByLabelText("Yeni etiket"), "İş");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur ve tak" }));
    expect(await screen.findByText("Bu adda bir etiketin var.")).toBeTruthy();
  });

  it("reports a failed change", async () => {
    const server = picker({ "PUT /items/i1/tags/b": () => apiError(409, "conflict") });
    await openPicker(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Ev" }));
    expect(await screen.findByText("İşlem yapılamadı. Tekrar dene.")).toBeTruthy();
  });

  it("is not offered for an item in the trash", async () => {
    const server = picker({
      "GET /items/i1": () => json(200, { ...item("i1", "Rapor yaz"), deleted_at: at }),
    });
    await open(server, "/items/i1");
    await screen.findByRole("header", { name: "Rapor yaz" });
    expect(screen.queryByRole("button", { name: "Etiketleri düzenle" })).toBeNull();
  });
});
