/**
 * Typed wrappers for the endpoints that exist today (identity, T0.3.x). Types come from
 * `@secondbrain/api-types` (generated from the Rust DTOs), so a server change that breaks the
 * contract fails the client typecheck.
 */

import type {
  AccessExplanation,
  AccessPreview,
  AccessPreviewRequest,
  AppleNativeRequest,
  Board,
  CreateBoardColumnRequest,
  CreateGoalRequest,
  CompleteRoutineRequest,
  CreateRoutineRequest,
  CreateSavedViewRequest,
  CreateSubtaskRequest,
  CreateTagRequest,
  CreateTaskRequest,
  CreateTrackerEntryRequest,
  CreateTrackerRequest,
  AuthResponse,
  CsrfResponse,
  CurrentUser,
  ForgotPasswordRequest,
  Goal,
  GoalProgress,
  GoogleNativeRequest,
  Health,
  Item,
  ItemFilter,
  ItemLink,
  ItemListState,
  LinkGoalRequest,
  LoginRequest,
  MoveBoardTaskRequest,
  Page,
  RegisterRequest,
  ResetPasswordRequest,
  Routine,
  RoutineDay,
  RoutineStreak,
  SavedView,
  SessionInfo,
  Subtask,
  Tag,
  Task,
  Tracker,
  TrackerEntry,
  TrackerSummary,
  UpdateBoardColumnRequest,
  UpdateGoalRequest,
  UpdateItemRequest,
  UpdateProfileRequest,
  UpdateSavedViewRequest,
  UpdateSubtaskRequest,
  UpdateTaskRequest,
  VerifyEmailRequest,
} from "@secondbrain/api-types";

import type { ApiClient } from "./client";

export interface MutationOptions {
  /** See {@link import("./client").RequestOptions.idempotencyKey}. */
  idempotencyKey?: string;
}

/** `ApiClient` plus one function per endpoint. */
export function endpoints(api: ApiClient) {
  return {
    health: () => api.get<Health>("/health"),
    auth: {
      register: (body: RegisterRequest) => api.post<AuthResponse>("/auth/register", body),
      login: (body: LoginRequest) => api.post<AuthResponse>("/auth/login", body),
      logout: () => api.post<void>("/auth/logout"),
      csrf: () => api.get<CsrfResponse>("/auth/csrf"),
      sessions: () => api.get<SessionInfo[]>("/auth/sessions"),
      revokeSession: (id: string) => api.delete(`/auth/sessions/${encodeURIComponent(id)}`),
      verifyEmail: (body: VerifyEmailRequest) => api.post<void>("/auth/email/verify", body),
      resendVerification: () => api.post<void>("/auth/email/verification"),
      forgotPassword: (body: ForgotPasswordRequest) =>
        api.post<void>("/auth/password/forgot", body),
      resetPassword: (body: ResetPasswordRequest) => api.post<void>("/auth/password/reset", body),
      /** Native apps only: exchanges a Google authorization code (PKCE) for a bearer session. */
      googleNative: (body: GoogleNativeRequest) =>
        api.post<AuthResponse>("/auth/google/native", body),
      /** iOS only: exchanges a Sign in with Apple credential for a bearer session. */
      appleNative: (body: AppleNativeRequest) => api.post<AuthResponse>("/auth/apple/native", body),
    },
    me: {
      get: () => api.get<CurrentUser>("/me"),
      update: (body: UpdateProfileRequest, options?: MutationOptions) =>
        api.patch<CurrentUser>("/me", body, options),
    },
    items: {
      get: (id: string) => api.get<Item>(`/items/${encodeURIComponent(id)}`),
      list: (state: ItemListState) => api.get<Item[]>("/items", { query: { state } }),
      /** Title and description; an empty description clears it. */
      update: (id: string, body: UpdateItemRequest, options?: MutationOptions) =>
        api.patch<Item>(`/items/${encodeURIComponent(id)}`, body, options),
      /** Back from the trash; owner only (PRD N4, V6). */
      restore: (id: string, options?: MutationOptions) =>
        api.post<Item>(`/items/${encodeURIComponent(id)}/restore`, undefined, options),
      unarchive: (id: string, options?: MutationOptions) =>
        api.post<Item>(`/items/${encodeURIComponent(id)}/unarchive`, undefined, options),
      /** Accessible items matching `filter`, newest first (PRD F1). */
      filter: (filter: ItemFilter, page: { cursor?: string; limit?: number } = {}) =>
        api.get<Page<Item>>("/items/filter", {
          query: { filter: JSON.stringify(filter), limit: page.limit, cursor: page.cursor },
        }),
      /** The caller's own tags on the item (PRD E2). */
      tags: (id: string) => api.get<Tag[]>(`/items/${encodeURIComponent(id)}/tags`),
      /** Links whose other end the caller can see (PRD V9). */
      links: (id: string) => api.get<ItemLink[]>(`/items/${encodeURIComponent(id)}/links`),
      attachTag: (id: string, tagId: string, options?: MutationOptions) =>
        api.put<void>(
          `/items/${encodeURIComponent(id)}/tags/${encodeURIComponent(tagId)}`,
          undefined,
          options,
        ),
      detachTag: (id: string, tagId: string, options?: MutationOptions) =>
        api.delete(`/items/${encodeURIComponent(id)}/tags/${encodeURIComponent(tagId)}`, options),
    },
    tags: {
      /** The tags the caller can see (PRD E4). */
      list: () => api.get<Tag[]>("/tags"),
      create: (body: CreateTagRequest, options?: MutationOptions) =>
        api.post<Tag>("/tags", body, options),
      /** Accessible items carrying the tag. */
      items: (id: string) => api.get<Item[]>(`/tags/${encodeURIComponent(id)}/items`),
    },
    tasks: {
      create: (body: CreateTaskRequest, options?: MutationOptions) =>
        api.post<Task>("/tasks", body, options),
      get: (id: string) => api.get<Task>(`/tasks/${encodeURIComponent(id)}`),
      /** Dates only; the title and description belong to the item. */
      update: (id: string, body: UpdateTaskRequest, options?: MutationOptions) =>
        api.patch<Task>(`/tasks/${encodeURIComponent(id)}`, body, options),
      addSubtask: (id: string, body: CreateSubtaskRequest, options?: MutationOptions) =>
        api.post<Subtask>(`/tasks/${encodeURIComponent(id)}/subtasks`, body, options),
      renameSubtask: (
        id: string,
        subtaskId: string,
        body: UpdateSubtaskRequest,
        options?: MutationOptions,
      ) =>
        api.patch<Subtask>(
          `/tasks/${encodeURIComponent(id)}/subtasks/${encodeURIComponent(subtaskId)}`,
          body,
          options,
        ),
      /** The last subtask cannot be deleted (PRD G2). */
      deleteSubtask: (id: string, subtaskId: string, options?: MutationOptions) =>
        api.delete(
          `/tasks/${encodeURIComponent(id)}/subtasks/${encodeURIComponent(subtaskId)}`,
          options,
        ),
      completeSubtask: (id: string, subtaskId: string, options?: MutationOptions) =>
        api.post<Subtask>(
          `/tasks/${encodeURIComponent(id)}/subtasks/${encodeURIComponent(subtaskId)}/complete`,
          undefined,
          options,
        ),
      reopenSubtask: (id: string, subtaskId: string, options?: MutationOptions) =>
        api.post<Subtask>(
          `/tasks/${encodeURIComponent(id)}/subtasks/${encodeURIComponent(subtaskId)}/reopen`,
          undefined,
          options,
        ),
    },
    board: {
      /** The caller's personal kanban (PRD G6). */
      get: () => api.get<Board>("/board"),
      createColumn: (body: CreateBoardColumnRequest, options?: MutationOptions) =>
        api.post<Board>("/board/columns", body, options),
      updateColumn: (id: string, body: UpdateBoardColumnRequest, options?: MutationOptions) =>
        api.patch<Board>(`/board/columns/${encodeURIComponent(id)}`, body, options),
      /** Its cards move to the inbox. */
      deleteColumn: (id: string, options?: MutationOptions) =>
        api.delete(`/board/columns/${encodeURIComponent(id)}`, options),
      moveTask: (taskId: string, body: MoveBoardTaskRequest, options?: MutationOptions) =>
        api.put<Board>(`/board/tasks/${encodeURIComponent(taskId)}`, body, options),
    },
    goals: {
      /** Both dates are required (PRD D1). */
      create: (body: CreateGoalRequest, options?: MutationOptions) =>
        api.post<Goal>("/goals", body, options),
      /** The goal with the linked items the caller can see (PRD V9). */
      get: (id: string) => api.get<Goal>(`/goals/${encodeURIComponent(id)}`),
      update: (id: string, body: UpdateGoalRequest, options?: MutationOptions) =>
        api.patch<Goal>(`/goals/${encodeURIComponent(id)}`, body, options),
      /** Links an item, or changes its weight (PRD D2, D3). */
      link: (id: string, itemId: string, body: LinkGoalRequest, options?: MutationOptions) =>
        api.put<Goal>(
          `/goals/${encodeURIComponent(id)}/links/${encodeURIComponent(itemId)}`,
          body,
          options,
        ),
      unlink: (id: string, itemId: string, options?: MutationOptions) =>
        api.delete<Goal>(
          `/goals/${encodeURIComponent(id)}/links/${encodeURIComponent(itemId)}`,
          options,
        ),
      /** Actual, pace and the breakdown as of the caller's day (PRD D3-D6). */
      progress: (id: string, today?: string) =>
        api.get<GoalProgress>(`/goals/${encodeURIComponent(id)}/progress`, { query: { today } }),
    },
    routines: {
      /** Creates the routine with its first plan version (PRD R1-R3, R13). */
      create: (body: CreateRoutineRequest, options?: MutationOptions) =>
        api.post<Routine>("/routines", body, options),
      get: (id: string) => api.get<Routine>(`/routines/${encodeURIComponent(id)}`),
      /** Adds to a day, today when `date` is absent; up to 3 days back (PRD R4, R7). */
      complete: (id: string, body: CompleteRoutineRequest, options?: MutationOptions) =>
        api.post<RoutineDay>(`/routines/${encodeURIComponent(id)}/completions`, body, options),
      /** Takes back the latest mark of a day. */
      undo: (id: string, date: string | undefined, options?: MutationOptions) =>
        api.delete<RoutineDay>(`/routines/${encodeURIComponent(id)}/completions`, {
          ...options,
          query: { date },
        }),
      /** Local days, both ends included, oldest first. */
      days: (id: string, query: { from?: string; to?: string } = {}) =>
        api.get<RoutineDay[]>(`/routines/${encodeURIComponent(id)}/completions`, { query }),
      streak: (id: string) => api.get<RoutineStreak>(`/routines/${encodeURIComponent(id)}/streak`),
    },
    trackers: {
      create: (body: CreateTrackerRequest, options?: MutationOptions) =>
        api.post<Tracker>("/trackers", body, options),
      get: (id: string) => api.get<Tracker>(`/trackers/${encodeURIComponent(id)}`),
      /** The current period: use, what is left and any overrun (PRD K2). */
      summary: (id: string) =>
        api.get<TrackerSummary>(`/trackers/${encodeURIComponent(id)}/summary`),
      /** Entries newest first; `from` and `to` are local days, both included. */
      entries: (
        id: string,
        query: { from?: string; to?: string; limit?: number; cursor?: string } = {},
      ) => api.get<Page<TrackerEntry>>(`/trackers/${encodeURIComponent(id)}/entries`, { query }),
      addEntry: (id: string, body: CreateTrackerEntryRequest, options?: MutationOptions) =>
        api.post<TrackerEntry>(`/trackers/${encodeURIComponent(id)}/entries`, body, options),
    },
    views: {
      /** The caller's saved views, pinned first (PRD F2). */
      list: () => api.get<SavedView[]>("/saved-views"),
      create: (body: CreateSavedViewRequest, options?: MutationOptions) =>
        api.post<SavedView>("/saved-views", body, options),
      update: (id: string, body: UpdateSavedViewRequest, options?: MutationOptions) =>
        api.patch<SavedView>(`/saved-views/${encodeURIComponent(id)}`, body, options),
      delete: (id: string, options?: MutationOptions) =>
        api.delete(`/saved-views/${encodeURIComponent(id)}`, options),
    },
    access: {
      /** "Why am I seeing this?" (PRD V11). */
      explanation: (itemId: string) =>
        api.get<AccessExplanation>(`/access/items/${encodeURIComponent(itemId)}/explanation`),
      /** Who would gain access if the action went ahead; changes nothing (PRD V7). */
      preview: (body: AccessPreviewRequest) => api.post<AccessPreview>("/access/preview", body),
    },
  };
}

export type Endpoints = ReturnType<typeof endpoints>;
