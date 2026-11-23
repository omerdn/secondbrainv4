import type { CreateTaskRequest, Task, UpdateTaskRequest } from "@secondbrain/api-types";

import { isDay } from "@/lib/dates";

export interface TaskDraft {
  title: string;
  description: string;
  startDate: string;
  dueDate: string;
}

export const emptyDraft: TaskDraft = { title: "", description: "", startDate: "", dueDate: "" };

export type DraftField = "title" | "startDate" | "dueDate";
export type DraftError = "required" | "invalidDay" | "dueBeforeStart";
export type DraftErrors = Partial<Record<DraftField, DraftError>>;

/** What is wrong with the draft, by field. Dates are optional (PRD G1). */
export function draftErrors(draft: TaskDraft): DraftErrors {
  const errors: DraftErrors = {};
  const start = draft.startDate.trim();
  const due = draft.dueDate.trim();
  if (draft.title.trim() === "") {
    errors.title = "required";
  }
  if (start !== "" && !isDay(start)) {
    errors.startDate = "invalidDay";
  }
  if (due !== "" && !isDay(due)) {
    errors.dueDate = "invalidDay";
  } else if (due !== "" && isDay(start) && due < start) {
    errors.dueDate = "dueBeforeStart";
  }
  return errors;
}

/** The request for a new task. Blank subtask rows are dropped; none left means G2 applies. */
export function createRequest(draft: TaskDraft, subtasks: readonly string[]): CreateTaskRequest {
  const titles = subtasks.map((title) => title.trim()).filter((title) => title !== "");
  const description = draft.description.trim();
  const start = draft.startDate.trim();
  const due = draft.dueDate.trim();
  return {
    title: draft.title.trim(),
    ...(description === "" ? {} : { description }),
    ...(start === "" ? {} : { start_date: start }),
    ...(due === "" ? {} : { due_date: due }),
    ...(titles.length === 0 ? {} : { subtasks: titles.map((title) => ({ title })) }),
  };
}

export function draftOf(task: Task): TaskDraft {
  return {
    title: task.item.title,
    description: task.item.description ?? "",
    startDate: task.start_date ?? "",
    dueDate: task.due_date ?? "",
  };
}

/** Only the dates that changed; a cleared field becomes `null`. `null` when nothing changed. */
export function datesPatch(task: Task, draft: TaskDraft): UpdateTaskRequest | null {
  const start = draft.startDate.trim();
  const due = draft.dueDate.trim();
  const patch: UpdateTaskRequest = {};
  if (start !== (task.start_date ?? "")) {
    patch.start_date = start === "" ? null : start;
  }
  if (due !== (task.due_date ?? "")) {
    patch.due_date = due === "" ? null : due;
  }
  return Object.keys(patch).length === 0 ? null : patch;
}

/** Only the item fields that changed. `null` when nothing changed. */
export function itemPatch(
  task: Task,
  draft: TaskDraft,
): { title?: string; description?: string } | null {
  const title = draft.title.trim();
  const description = draft.description.trim();
  const patch: { title?: string; description?: string } = {};
  if (title !== task.item.title) {
    patch.title = title;
  }
  if (description !== (task.item.description ?? "")) {
    patch.description = description;
  }
  return Object.keys(patch).length === 0 ? null : patch;
}
