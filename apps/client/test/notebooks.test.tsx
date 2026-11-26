import type { Item, Notebook, NotebookPage, NotebookSummary } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import { TITLE_MAX_CHARS, titleError } from "@/features/notebooks/model";
import { i18n } from "@/lib/i18n";

import { fakeServer, json, memoryTokenStore, resetCredentials, user } from "./support/api";
import { renderApp } from "./support/router";

const at = "2026-11-26T12:00:00Z";

const item = (id: string, kind: string, title: string): Item => ({
  id,
  kind,
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

const mine = item("n1", "notebook", "Defterim");
const work = item("n2", "notebook", "İş");
const pageOf = (id: string, title: string, notebook = "n1"): NotebookPage => ({
  item: item(id, "page", title),
  notebook_id: notebook,
  process_of_item_id: null,
});
const summaries = (...extra: Item[]): NotebookSummary[] => [
  { item: mine, is_default: true },
  ...extra.map((notebook) => ({ item: notebook, is_default: false })),
];
const notebook = (of: Item, pages: NotebookPage[] = []): Notebook => ({
  item: of,
  is_default: of.id === "n1",
  pages,
});

const open = (server: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(server.api, url, { tokenStore: memoryTokenStore("t") });

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("notebook names (Y1)", () => {
  it("requires a name within the item limit", () => {
    expect(titleError("  ")).toBe("required");
    expect(titleError("x".repeat(TITLE_MAX_CHARS + 1))).toBe("tooLong");
    expect(titleError(" Defter ")).toBeNull();
  });
});

describe("notebooks screen (Y1)", () => {
  it("opens a notebook's pages and navigates to one", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /notebooks": () => json(200, summaries(work)),
      "GET /notebooks/n1": () =>
        json(200, notebook(mine, [pageOf("p1", "Okuma notları"), pageOf("p2", "Fikirler")])),
      "GET /pages/p1": () => json(200, pageOf("p1", "Okuma notları")),
    });
    await open(server, "/notebooks");
    expect(await screen.findByText("Defterim")).toBeTruthy();
    expect(screen.getByText("Varsayılan")).toBeTruthy();
    // Collapsed until opened: the pages are fetched on demand.
    expect(screen.queryByText("Okuma notları")).toBeNull();
    expect(server.calls.some((call) => call.path === "/notebooks/n1")).toBe(false);
    await fireEvent.press(screen.getByRole("button", { name: "Defterim defterini aç" }));
    expect(await screen.findByText("Fikirler")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Defterim defterini kapat" })).toBeTruthy();
    await fireEvent.press(screen.getByRole("link", { name: "Okuma notları" }));
    expect(await screen.findByRole("header", { name: "Okuma notları" })).toBeTruthy();
    expect(screen.getByText("Sayfa düzenleyici yakında burada olacak.")).toBeTruthy();
  });

  it("creates a notebook after checking the name, then a page in it", async () => {
    let created = false;
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /notebooks": () => json(200, created ? summaries(work) : summaries()),
      "POST /notebooks": () => {
        created = true;
        return json(201, notebook(work));
      },
      "GET /notebooks/n2": () => json(200, notebook(work)),
      "POST /notebooks/n2/pages": () => json(201, pageOf("p9", "Toplantılar", "n2")),
      "GET /pages/p9": () => json(200, pageOf("p9", "Toplantılar", "n2")),
    });
    await open(server, "/notebooks");
    await fireEvent.press(await screen.findByRole("button", { name: "Yeni defter" }));
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(screen.getByText("Bu alan gerekli.")).toBeTruthy();
    expect(server.calls.some((call) => call.method === "POST")).toBe(false);
    await fireEvent.changeText(screen.getByLabelText("Ad"), "  İş ");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    // The new notebook opens, empty.
    expect(await screen.findByText("Bu defterde henüz sayfa yok.")).toBeTruthy();
    expect(
      server.calls.find((call) => call.path === "/notebooks" && call.method === "POST")?.body,
    ).toEqual({ title: "İş" });

    await fireEvent.press(screen.getByRole("button", { name: "İş defterine sayfa ekle" }));
    await fireEvent.changeText(screen.getByLabelText("Ad"), "Toplantılar");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(await screen.findByRole("header", { name: "Toplantılar" })).toBeTruthy();
    expect(server.calls.find((call) => call.path === "/notebooks/n2/pages")?.body).toEqual({
      title: "Toplantılar",
    });
  });

  it("renames a notebook and deletes one only after confirming; the default stays", async () => {
    let list = summaries(work);
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /notebooks": () => json(200, list),
      "PATCH /items/n2": (call) => {
        const renamed = { ...work, title: (call.body as { title: string }).title };
        list = summaries(renamed);
        return json(200, renamed);
      },
      "DELETE /items/n2": () => {
        list = summaries();
        return json(204);
      },
    });
    await open(server, "/notebooks");
    expect(await screen.findByText("İş")).toBeTruthy();
    // The default notebook cannot be deleted.
    expect(screen.queryByRole("button", { name: "Defterim defterini sil" })).toBeNull();

    await fireEvent.press(screen.getByRole("button", { name: "İş adını değiştir" }));
    expect(screen.getByLabelText("Ad").props.value).toBe("İş");
    await fireEvent.changeText(screen.getByLabelText("Ad"), "Ofis");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
    expect(await screen.findByText("Ofis")).toBeTruthy();
    expect(server.calls.find((call) => call.method === "PATCH")?.body).toEqual({ title: "Ofis" });

    await fireEvent.press(screen.getByRole("button", { name: "Ofis defterini sil" }));
    expect(server.calls.some((call) => call.method === "DELETE")).toBe(false);
    await fireEvent.press(screen.getByRole("button", { name: "Çöpe at" }));
    await waitFor(() => {
      expect(screen.queryByText("Ofis")).toBeNull();
    });
    expect(server.calls.some((call) => call.method === "DELETE" && call.path === "/items/n2")).toBe(
      true,
    );
  });
});

describe("page screen", () => {
  it("renames a page and moves it to the trash", async () => {
    let current = pageOf("p1", "Okuma notları");
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /pages/p1": () => json(200, current),
      "PATCH /items/p1": (call) => {
        current = pageOf("p1", (call.body as { title: string }).title);
        return json(200, current.item);
      },
      "DELETE /items/p1": () => json(204),
      "GET /notebooks": () => json(200, summaries()),
    });
    await open(server, "/pages/p1");
    expect(await screen.findByRole("header", { name: "Okuma notları" })).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Yeniden adlandır" }));
    await fireEvent.changeText(screen.getByLabelText("Ad"), "Kitaplar");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
    expect(await screen.findByRole("header", { name: "Kitaplar" })).toBeTruthy();

    await fireEvent.press(screen.getByRole("button", { name: "Sil" }));
    await fireEvent.press(screen.getByRole("button", { name: "Çöpe at" }));
    expect(await screen.findByRole("button", { name: "Yeni defter" })).toBeTruthy();
    expect(server.calls.some((call) => call.method === "DELETE" && call.path === "/items/p1")).toBe(
      true,
    );
  });

  it("says when a page is not there", async () => {
    const server = fakeServer({
      "GET /me": () => json(200, user),
      "GET /pages/zz": () => json(404, { code: "not_found", message: "not found" }),
    });
    await open(server, "/pages/zz");
    expect(await screen.findByText(i18n.t("items.detail.notFound"))).toBeTruthy();
  });
});
