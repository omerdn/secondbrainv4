import type { Item, Subtask, Task } from "@secondbrain/api-types";
import { act, fireEvent, screen, waitFor } from "expo-router/testing-library";

import {
  createRequest,
  datesPatch,
  draftErrors,
  draftOf,
  emptyDraft,
  isDefaultTaskFilter,
  itemPatch,
  taskFilter,
} from "@/features/tasks/model";
import { addDays, isDay } from "@/lib/dates";
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

const at = "2026-11-23T12:00:00Z";

const item: Item = {
  id: "t1",
  kind: "task",
  title: "Raporu yaz",
  description: null,
  owner: { type: "user", user_id: "u1" },
  created_by: "u1",
  created_at: at,
  updated_at: at,
  last_touched_at: at,
  archived_at: null,
  deleted_at: null,
};

const subtask = (id: string, title: string, done = false): Subtask => ({
  id,
  task_id: "t1",
  title,
  position: 0,
  done_at: done ? at : null,
  done_by: done ? "u1" : null,
  assignee_ids: [],
});

const task: Task = {
  item,
  start_date: "2026-11-01",
  due_date: null,
  subtasks: [subtask("s1", "Taslak"), subtask("s2", "Gözden geçir", true)],
  tags: [],
  pages: [],
  member_ids: ["u1"],
};

const open = (server: ReturnType<typeof fakeServer>, url: string) =>
  renderApp(server.api, url, { tokenStore: memoryTokenStore("t") });

const routes = {
  "GET /me": () => json(200, user),
  "GET /tasks/t1": () => json(200, task),
  "GET /items/t1/tags": () => json(200, []),
};

const mutation = (server: ReturnType<typeof fakeServer>) =>
  server.calls.filter((call) => call.method !== "GET");

/** The form's save button comes first, an open subtask editor's after it. */
const saveButton = (which: "first" | "last") => {
  const buttons = screen.getAllByRole("button", { name: "Kaydet" });
  const button = which === "first" ? buttons[0] : buttons[buttons.length - 1];
  if (button === undefined) {
    throw new Error("no save button");
  }
  return button;
};

beforeEach(async () => {
  resetCredentials();
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("task model (G1, G2)", () => {
  it("shares calendar helpers", () => {
    expect(isDay("2026-02-29")).toBe(false);
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
  });

  it("checks the title and the dates", () => {
    expect(draftErrors(emptyDraft)).toEqual({ title: "required" });
    expect(
      draftErrors({ ...emptyDraft, title: "a", startDate: "x", dueDate: "2026-13-01" }),
    ).toEqual({ startDate: "invalidDay", dueDate: "invalidDay" });
    expect(
      draftErrors({ ...emptyDraft, title: "a", startDate: "2026-11-02", dueDate: "2026-11-01" }),
    ).toEqual({ dueDate: "dueBeforeStart" });
  });

  it("leaves empty fields out of a new task, so G2 applies", () => {
    expect(createRequest({ ...emptyDraft, title: " Yaz " }, ["  "])).toEqual({ title: "Yaz" });
    expect(
      createRequest({ ...emptyDraft, title: "Yaz", dueDate: "2026-12-01" }, [" a ", "", "b"]),
    ).toEqual({ title: "Yaz", due_date: "2026-12-01", subtasks: [{ title: "a" }, { title: "b" }] });
  });

  it("patches only what changed and clears dates with null", () => {
    const same = draftOf(task);
    expect(itemPatch(task, same)).toBeNull();
    expect(datesPatch(task, same)).toBeNull();
    expect(itemPatch(task, { ...same, title: "Yeni" })).toEqual({ title: "Yeni" });
    expect(datesPatch(task, { ...same, startDate: "", dueDate: "2026-12-01" })).toEqual({
      start_date: null,
      due_date: "2026-12-01",
    });
  });
});

describe("tasks screen", () => {
  it("creates a task with subtasks and opens it", async () => {
    const server = fakeServer({
      ...routes,
      "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
      "POST /tasks": () => json(201, task),
    });
    await open(server, "/tasks");
    await fireEvent.press(await screen.findByRole("button", { name: "Yeni görev" }));
    await fireEvent.changeText(screen.getByLabelText("Başlık"), "Raporu yaz");
    await fireEvent.changeText(screen.getByLabelText("Bitiş (YYYY-AA-GG)"), "2026-12-01");
    await fireEvent.press(screen.getByRole("button", { name: "Alt kırılım ekle" }));
    await fireEvent.changeText(screen.getByLabelText("Alt kırılım 1"), "Taslak");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(await screen.findByRole("header", { name: "Raporu yaz" })).toBeTruthy();
    expect(mutation(server)[0]?.body).toEqual({
      title: "Raporu yaz",
      due_date: "2026-12-01",
      subtasks: [{ title: "Taslak" }],
    });
  });

  it("keeps the form when creating fails", async () => {
    const server = fakeServer({
      ...routes,
      "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
      "POST /tasks": () => apiError(500, "internal"),
    });
    await open(server, "/tasks");
    await fireEvent.press(await screen.findByRole("button", { name: "Yeni görev" }));
    await fireEvent.changeText(screen.getByLabelText("Başlık"), "Kalsın");
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(await screen.findByText("Kaydedilemedi. Tekrar dene.")).toBeTruthy();
    expect(screen.getByLabelText("Başlık").props.value).toBe("Kalsın");
  });

  it("does not send a task without a title", async () => {
    const server = fakeServer({
      ...routes,
      "GET /items/filter": () => json(200, { items: [], next_cursor: null }),
    });
    await open(server, "/tasks");
    await fireEvent.press(await screen.findByRole("button", { name: "Yeni görev" }));
    await fireEvent.press(screen.getByRole("button", { name: "Oluştur" }));
    expect(await screen.findByText("Bu alan gerekli.")).toBeTruthy();
    expect(mutation(server)).toEqual([]);
  });
});

describe("task list (G7, F1)", () => {
  const filterOf = (call: { path: string }) =>
    JSON.parse(new URL(`http://x${call.path}`).searchParams.get("filter") ?? "{}") as unknown;
  const page = (items: Item[], next_cursor: string | null = null) =>
    json(200, { items, next_cursor });
  const tag = { id: "g1", name: "İş", color: null, created_at: at, updated_at: at };
  const lists = (server: ReturnType<typeof fakeServer>) =>
    server.calls.filter((call) => call.path.startsWith("/items/filter"));

  it("always filters by task and adds the chosen state and tags", () => {
    expect(taskFilter({})).toEqual({ kinds: ["task"] });
    expect(taskFilter({ kinds: ["goal"], state: "active" })).toEqual({ kinds: ["task"] });
    expect(taskFilter({ state: "trash", tag_ids: ["b", "a"] })).toEqual({
      kinds: ["task"],
      tag_ids: ["a", "b"],
      state: "trash",
    });
    expect(isDefaultTaskFilter({ state: "active", tag_ids: [] })).toBe(true);
    expect(isDefaultTaskFilter({ state: "archived" })).toBe(false);
  });

  it("lists tasks and filters them by state and tag", async () => {
    const server = fakeServer({
      ...routes,
      "GET /tags": () => json(200, [tag]),
      "GET /items/filter": () => page([item]),
    });
    await open(server, "/tasks");
    expect(await screen.findByRole("button", { name: "Görev: Raporu yaz" })).toBeTruthy();
    expect(screen.queryByText("Tür")).toBeNull();
    await fireEvent.press(screen.getByRole("button", { name: "Arşiv" }));
    await waitFor(() => {
      expect(filterOf(lists(server).at(-1) ?? { path: "/" })).toEqual({
        kinds: ["task"],
        state: "archived",
      });
    });
    await fireEvent.press(await screen.findByRole("button", { name: "İş" }));
    await waitFor(() => {
      expect(filterOf(lists(server).at(-1) ?? { path: "/" })).toEqual({
        kinds: ["task"],
        tag_ids: ["g1"],
        state: "archived",
      });
    });
  });

  it("loads the next page", async () => {
    const second = { ...item, id: "t2", title: "İkinci" };
    const server = fakeServer({
      ...routes,
      "GET /items/filter": (call) =>
        call.path.includes("cursor=") ? page([second]) : page([item], "next"),
    });
    await open(server, "/tasks");
    await fireEvent.press(await screen.findByRole("button", { name: "Daha fazla yükle" }));
    expect(await screen.findByRole("button", { name: "Görev: İkinci" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Görev: Raporu yaz" })).toBeTruthy();
  });

  it("tells an empty list from a filter with no match", async () => {
    await open(fakeServer({ ...routes, "GET /items/filter": () => page([]) }), "/tasks");
    expect(await screen.findByText("Henüz görevin yok")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Çöp" }));
    expect(await screen.findByText("Bu filtreye uyan görev yok")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Filtreyi temizle" }));
    expect(await screen.findByText("Henüz görevin yok")).toBeTruthy();
  });

  it("offers a retry when the list fails", async () => {
    let fail = true;
    await open(
      fakeServer({
        ...routes,
        "GET /items/filter": () => (fail ? apiError(500, "internal") : page([item])),
      }),
      "/tasks",
    );
    const retry = await screen.findByRole("button", { name: "Tekrar dene" });
    fail = false;
    await fireEvent.press(retry);
    expect(await screen.findByRole("button", { name: "Görev: Raporu yaz" })).toBeTruthy();
  });
});

describe("task detail (G1, G2)", () => {
  it("checks and unchecks subtasks", async () => {
    const server = fakeServer({
      ...routes,
      "POST /tasks/t1/subtasks/s1/complete": () => json(200, subtask("s1", "Taslak", true)),
      "POST /tasks/t1/subtasks/s2/reopen": () => json(200, subtask("s2", "Gözden geçir")),
    });
    await open(server, "/tasks/t1");
    await fireEvent.press(await screen.findByRole("checkbox", { name: "Taslak" }));
    await waitFor(() => {
      expect(mutation(server).map((call) => call.path)).toContain("/tasks/t1/subtasks/s1/complete");
    });
    await waitFor(() => {
      expect(screen.getByRole("checkbox", { name: "Gözden geçir" })).toBeEnabled();
    });
    await fireEvent.press(screen.getByRole("checkbox", { name: "Gözden geçir" }));
    await waitFor(() => {
      expect(mutation(server).map((call) => call.path)).toContain("/tasks/t1/subtasks/s2/reopen");
    });
  });

  it("adds and renames a subtask", async () => {
    const server = fakeServer({
      ...routes,
      "POST /tasks/t1/subtasks": () => json(201, subtask("s3", "Yeni")),
      "PATCH /tasks/t1/subtasks/s1": () => json(200, subtask("s1", "Taslak 2")),
    });
    await open(server, "/tasks/t1");
    await fireEvent.changeText(await screen.findByLabelText("Yeni alt kırılım"), " Yeni ");
    await fireEvent.press(screen.getByRole("button", { name: "Alt kırılım ekle" }));
    await waitFor(() => {
      expect(mutation(server)[0]?.body).toEqual({ title: "Yeni" });
    });
    await fireEvent.press(await screen.findByRole("button", { name: "Düzenle: Taslak" }));
    await fireEvent.changeText(screen.getByLabelText("Alt kırılım adı"), "Taslak 2");
    await fireEvent.press(saveButton("last"));
    await waitFor(() => {
      expect(mutation(server).some((call) => call.method === "PATCH")).toBe(true);
    });
  });

  it("does not offer to delete the last subtask (G2)", async () => {
    await open(
      fakeServer({
        ...routes,
        "GET /tasks/t1": () => json(200, { ...task, subtasks: [subtask("s1", "Tek")] }),
      }),
      "/tasks/t1",
    );
    expect(await screen.findByRole("button", { name: "Sil: Tek" })).toBeDisabled();
    expect(screen.getByText("Görevde en az bir alt kırılım kalmalı.")).toBeTruthy();
  });

  it("saves only the changed fields and clears a date", async () => {
    const server = fakeServer({
      ...routes,
      "PATCH /items/t1": () => json(200, { ...item, title: "Yeni başlık" }),
      "PATCH /tasks/t1": () => json(200, task),
    });
    await open(server, "/tasks/t1");
    await fireEvent.changeText(await screen.findByLabelText("Başlık"), "Yeni başlık");
    await fireEvent.changeText(screen.getByLabelText("Başlangıç (YYYY-AA-GG)"), "");
    await fireEvent.press(saveButton("first"));
    await waitFor(() => {
      expect(mutation(server)).toHaveLength(2);
    });
    expect(mutation(server)[0]?.body).toEqual({ title: "Yeni başlık" });
    expect(mutation(server)[1]?.body).toEqual({ start_date: null });
  });

  it("keeps the edits when saving fails", async () => {
    await open(
      fakeServer({ ...routes, "PATCH /items/t1": () => apiError(500, "internal") }),
      "/tasks/t1",
    );
    await fireEvent.changeText(await screen.findByLabelText("Başlık"), "Kaybolmasın");
    await fireEvent.press(saveButton("first"));
    expect(await screen.findByText("Kaydedilemedi. Tekrar dene.")).toBeTruthy();
    expect(screen.getByLabelText("Başlık").props.value).toBe("Kaybolmasın");
  });

  it("explains a refused check (G4)", async () => {
    await open(
      fakeServer({
        ...routes,
        "POST /tasks/t1/subtasks/s1/complete": () => apiError(403, "forbidden"),
      }),
      "/tasks/t1",
    );
    await fireEvent.press(await screen.findByRole("checkbox", { name: "Taslak" }));
    expect(await screen.findByText("Bunu yapma yetkin yok.")).toBeTruthy();
  });

  it("opens the tag picker", async () => {
    await open(fakeServer({ ...routes, "GET /tags": () => json(200, []) }), "/tasks/t1");
    await fireEvent.press(await screen.findByRole("button", { name: "Etiketleri düzenle" }));
    expect(await screen.findByLabelText("Yeni etiket")).toBeTruthy();
  });

  it("looks the same for a hidden and a missing task (V9)", async () => {
    await open(fakeServer({ "GET /me": () => json(200, user) }), "/tasks/t1");
    expect(await screen.findByText(i18n.t("items.detail.notFound"))).toBeTruthy();
  });
});
