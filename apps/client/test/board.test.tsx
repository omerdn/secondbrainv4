import type { Board, BoardCard } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import {
  applyMove,
  cardStepTarget,
  columnStepTarget,
  moveRequest,
  pressOpensCard,
  toDndBoard,
} from "@/features/tasks/boardModel";
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

// The gesture layer needs native modules. Dragging itself is verified on web (ADR-0024); its
// decisions are pure functions tested in `dnd-board.test.ts`.
jest.mock("@/features/tasks/dragLayer", () => ({
  useDragOffset: () => ({ x: { value: 0 }, y: { value: 0 } }),
  DragHandle: ({ children }: { children: unknown }) => children,
  DragOverlay: ({ children }: { children: unknown }) => children,
}));

const card = (id: string, title = id): BoardCard => ({
  task_id: id,
  title,
  start_date: null,
  due_date: null,
  subtasks_done: 0,
  subtasks_total: 1,
});

const board: Board = {
  columns: [
    { id: "inbox", name: "Inbox", is_inbox: true, cards: [card("a"), card("b"), card("c")] },
    { id: "doing", name: "Yapılıyor", is_inbox: false, cards: [card("d")] },
    { id: "done", name: "Bitti", is_inbox: false, cards: [] },
  ],
};

const titles = (next: Board, column: number) => next.columns[column]?.cards.map((c) => c.task_id);

const open = (server: ReturnType<typeof fakeServer>) =>
  renderApp(server.api, "/tasks?view=board", { tokenStore: memoryTokenStore("t") });

const routes = {
  "GET /me": () => json(200, user),
  "GET /board": () => json(200, board),
};

const mutations = (server: ReturnType<typeof fakeServer>) =>
  server.calls.filter((call) => call.method !== "GET");

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("board model (G6)", () => {
  it("maps the server board for the drag logic", () => {
    expect(toDndBoard(board)[0]).toEqual({
      id: "inbox",
      title: "Inbox",
      cardIds: ["a", "b", "c"],
    });
  });

  it("says which card a drop lands after", () => {
    expect(moveRequest(board, "c", { columnId: "inbox", index: 0 })).toEqual({
      column_id: "inbox",
    });
    expect(moveRequest(board, "a", { columnId: "inbox", index: 1 })).toEqual({
      column_id: "inbox",
      after_task_id: "b",
    });
    expect(moveRequest(board, "a", { columnId: "doing", index: 1 })).toEqual({
      column_id: "doing",
      after_task_id: "d",
    });
    expect(moveRequest(board, "a", { columnId: "done", index: 0 })).toEqual({
      column_id: "done",
    });
    expect(moveRequest(board, "x", { columnId: "done", index: 0 })).toBeNull();
  });

  it("shows the move before the server answers, keeping every card", () => {
    const next = applyMove(board, "a", { columnId: "doing", index: 0 });
    expect(titles(next, 0)).toEqual(["b", "c"]);
    expect(titles(next, 1)).toEqual(["a", "d"]);
  });

  it("keeps the inbox first when stepping columns", () => {
    const ids = ["inbox", "x", "y", "z"];
    expect(columnStepTarget(ids, 1, "left")).toBeNull();
    expect(columnStepTarget(ids, 2, "left")).toBe("inbox");
    expect(columnStepTarget(ids, 3, "left")).toBe("x");
    expect(columnStepTarget(ids, 1, "right")).toBe("y");
    expect(columnStepTarget(ids, 3, "right")).toBeNull();
    expect(columnStepTarget(ids, 0, "right")).toBeNull();
  });

  it("does not open a card from the click that ends a drag", () => {
    expect(pressOpensCard(false, null, 1_000)).toBe(true);
    expect(pressOpensCard(true, null, 1_000)).toBe(false);
    expect(pressOpensCard(false, 1_000, 1_100)).toBe(false);
    expect(pressOpensCard(false, 1_000, 1_350)).toBe(true);
  });

  it("steps a card within its column", () => {
    expect(cardStepTarget(board, "a", "up")).toBeNull();
    expect(cardStepTarget(board, "a", "down")).toEqual({ columnId: "inbox", index: 1 });
    expect(cardStepTarget(board, "c", "up")).toEqual({ columnId: "inbox", index: 1 });
    expect(cardStepTarget(board, "c", "down")).toBeNull();
    expect(cardStepTarget(board, "nope", "up")).toBeNull();
  });
});

describe("kanban screen (G6)", () => {
  it("shows the columns with the inbox first", async () => {
    await open(fakeServer(routes));
    expect(await screen.findByRole("header", { name: "Gelen kutusu (3)" })).toBeTruthy();
    expect(screen.getByRole("header", { name: "Yapılıyor (1)" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Görevi aç: a" })).toBeTruthy();
  });

  it("moves a card to another column without dragging", async () => {
    const moved = applyMove(board, "a", { columnId: "done", index: 0 });
    const server = fakeServer({ ...routes, "PUT /board/tasks/a": () => json(200, moved) });
    await open(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Taşı: a" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Bitti" }));
    await waitFor(() => {
      expect(mutations(server)[0]?.body).toEqual({ column_id: "done" });
    });
    expect(await screen.findByRole("header", { name: "Bitti (1)" })).toBeTruthy();
  });

  it("moves a card down one step", async () => {
    const moved = applyMove(board, "a", { columnId: "inbox", index: 1 });
    const server = fakeServer({ ...routes, "PUT /board/tasks/a": () => json(200, moved) });
    await open(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Taşı: a" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Aşağı" }));
    await waitFor(() => {
      expect(mutations(server)[0]?.body).toEqual({ column_id: "inbox", after_task_id: "b" });
    });
  });

  it("goes back to the real order when a move fails", async () => {
    const server = fakeServer({
      ...routes,
      "PUT /board/tasks/a": () => apiError(500, "internal"),
    });
    await open(server);
    await fireEvent.press(await screen.findByRole("button", { name: "Taşı: a" }));
    await fireEvent.press(await screen.findByRole("button", { name: "Bitti" }));
    expect(await screen.findByText("Değişiklik kaydedilemedi. Tekrar dene.")).toBeTruthy();
    expect(await screen.findByRole("header", { name: "Gelen kutusu (3)" })).toBeTruthy();
  });

  it("adds a column", async () => {
    const server = fakeServer({ ...routes, "POST /board/columns": () => json(201, board) });
    await open(server);
    await fireEvent.changeText(await screen.findByLabelText("Yeni sütun"), " Sonra ");
    await fireEvent.press(screen.getByRole("button", { name: "Sütun ekle" }));
    await waitFor(() => {
      expect(mutations(server)[0]?.body).toEqual({ name: "Sonra" });
    });
  });

  it("renames, reorders and deletes a column, but never the inbox", async () => {
    const server = fakeServer({
      ...routes,
      "PATCH /board/columns/done": () => json(200, board),
      "DELETE /board/columns/done": () => new Response(null, { status: 204 }),
    });
    await open(server);
    await screen.findByRole("header", { name: "Gelen kutusu (3)" });
    expect(screen.queryByRole("button", { name: "Sütunu düzenle: Inbox" })).toBeNull();
    await fireEvent.press(screen.getByRole("button", { name: "Sütunu düzenle: Bitti" }));
    await fireEvent.changeText(await screen.findByLabelText("Sütun adı"), "Tamam");
    await fireEvent.press(screen.getByRole("button", { name: "Adı kaydet" }));
    await waitFor(() => {
      expect(mutations(server)[0]?.body).toEqual({ name: "Tamam" });
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Sola" })).toBeEnabled();
    });
    await fireEvent.press(screen.getByRole("button", { name: "Sola" }));
    await waitFor(() => {
      expect(mutations(server)[1]?.body).toEqual({ after_column_id: "inbox" });
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Sütunu sil" })).toBeEnabled();
    });
    await fireEvent.press(screen.getByRole("button", { name: "Sütunu sil" }));
    await waitFor(() => {
      expect(mutations(server)[2]?.method).toBe("DELETE");
    });
  });

  it("offers a retry when the board fails to load", async () => {
    let fail = true;
    await open(
      fakeServer({
        "GET /me": () => json(200, user),
        "GET /board": () => (fail ? apiError(500, "internal") : json(200, board)),
      }),
    );
    const retry = await screen.findByRole("button", { name: "Tekrar dene" });
    fail = false;
    await fireEvent.press(retry);
    expect(await screen.findByRole("header", { name: "Gelen kutusu (3)" })).toBeTruthy();
  });
});
