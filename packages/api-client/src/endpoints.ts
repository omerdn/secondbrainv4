/**
 * Typed wrappers for the endpoints that exist today (identity, T0.3.x). Types come from
 * `@secondbrain/api-types` (generated from the Rust DTOs), so a server change that breaks the
 * contract fails the client typecheck.
 */

import type {
  CreateTeamRequest,
  InviteToTeamRequest,
  Team,
  TeamDetail,
  TeamInvitation,
  ConsentRequest,
  LegalDocument,
  LegalDocumentKind,
  LegalDocumentSummary,
  AccountDeletion,
  DataExport,
  OwnedSharedItem,
  TransferOwnershipRequest,
  AccessExplanation,
  AccessPreview,
  AccessPreviewRequest,
  Activity,
  ActivityReaction,
  AppleNativeRequest,
  Board,
  CreateBoardColumnRequest,
  CreateGoalRequest,
  CreateNotebookRequest,
  CreatePageRequest,
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
  ItemStatusLine,
  JoinSessionRequest,
  LinkGoalRequest,
  LinkPreview,
  LoginRequest,
  MoveBoardTaskRequest,
  Notebook,
  NotebookPage,
  NotebookSummary,
  Page,
  RegisterRequest,
  ResetPasswordRequest,
  Routine,
  RoutineDay,
  RoutinePeopleDays,
  RoutineRotationSchedule,
  RoutineStreak,
  RoutineSwapRequest,
  SavedView,
  OpenSessionRequest,
  SessionInfo,
  SessionInvitation,
  SessionStats,
  SessionSummary,
  TimerCommand,
  Today,
  Calendar,
  SearchHit,
  BlockedUser,
  Friend,
  IncomingFriendRequest,
  OutgoingFriendRequest,
  Invitation,
  InvitationRole,
  ItemMembers,
  NotificationKind,
  NotificationList,
  Notification,
  NotificationPreference,
  PushPublicKey,
  PushSubscriptionRequest,
  RemovePushSubscriptionRequest,
  UpdateRoutineRequest,
  UpdateNotificationPreferenceRequest,
  HomePreferences,
  UpdateHomePreferences,
  WorkSession,
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
      /** Finishes a sign-in held for consent with every current legal text accepted (HS5). */
      consent: (body: ConsentRequest) => api.post<AuthResponse>("/auth/consent", body),
    },
    teams: {
      /** The caller's teams (TK1, T5.1.1). */
      list: () => api.get<Team[]>("/teams"),
      get: (id: string) => api.get<TeamDetail>(`/teams/${encodeURIComponent(id)}`),
      create: (body: CreateTeamRequest, options?: MutationOptions) =>
        api.post<Team>("/teams", body, options),
      invite: (id: string, body: InviteToTeamRequest, options?: MutationOptions) =>
        api.post<void>(`/teams/${encodeURIComponent(id)}/invitations`, body, options),
      /** Team invitations waiting for the caller (TK2). */
      invitations: () => api.get<TeamInvitation[]>("/team-invitations"),
      join: (id: string, options?: MutationOptions) =>
        api.post<Team>(`/teams/${encodeURIComponent(id)}/join`, undefined, options),
      decline: (id: string, options?: MutationOptions) =>
        api.post<void>(`/teams/${encodeURIComponent(id)}/decline`, undefined, options),
    },
    legal: {
      /** The legal texts in force (HS5, T4.3.3). */
      list: () => api.get<LegalDocumentSummary[]>("/legal"),
      /** One text in force, as Markdown. */
      get: (kind: LegalDocumentKind) =>
        api.get<LegalDocument>(`/legal/${encodeURIComponent(kind)}`),
    },
    me: {
      get: () => api.get<CurrentUser>("/me"),
      update: (body: UpdateProfileRequest, options?: MutationOptions) =>
        api.patch<CurrentUser>("/me", body, options),
    },
    account: {
      /** The pending account deletion, if any (PRD HS2, T4.3.1). */
      deletion: () => api.get<AccountDeletion>("/account/deletion"),
      /** Asks for the account to be deleted after 30 days. */
      requestDeletion: (options?: MutationOptions) =>
        api.post<AccountDeletion>("/account/deletion", undefined, options),
      /** Cancels a pending deletion. */
      cancelDeletion: (options?: MutationOptions) =>
        api.delete<AccountDeletion>("/account/deletion", options),
      /** Shared items the caller owns and who would inherit each (HS2.1). */
      sharedItems: () => api.get<OwnedSharedItem[]>("/account/shared-items"),
      /** The latest data export whose link still works, or `null` (HS3, T4.3.2). */
      latestExport: () => api.get<DataExport | null>("/account/exports"),
      /** Queues a data export (or returns the one being built). */
      requestExport: (options?: MutationOptions) =>
        api.post<DataExport>("/account/exports", undefined, options),
    },
    items: {
      get: (id: string) => api.get<Item>(`/items/${encodeURIComponent(id)}`),
      /** The owner hands a personal item to one of its members (HS2.1). */
      transferOwnership: (id: string, body: TransferOwnershipRequest, options?: MutationOptions) =>
        api.put<void>(`/items/${encodeURIComponent(id)}/owner`, body, options),
      /** Members and their roles; pending invitations for the owner (PRD N2, V5). */
      members: (id: string) => api.get<ItemMembers>(`/items/${encodeURIComponent(id)}/members`),
      /** Invites a friend; preview it first (PRD G5, V1, V7). */
      invite: (
        id: string,
        body: { to_user: string; role: InvitationRole },
        options?: MutationOptions,
      ) => api.post<Invitation>(`/items/${encodeURIComponent(id)}/invitations`, body, options),
      /** The owner removes anyone; a member removes only themselves (PRD V5). */
      removeMember: (id: string, userId: string, options?: MutationOptions) =>
        api.delete(
          `/items/${encodeURIComponent(id)}/members/${encodeURIComponent(userId)}`,
          options,
        ),
      list: (state: ItemListState) => api.get<Item[]>("/items", { query: { state } }),
      /** Title and description; an empty description clears it. */
      update: (id: string, body: UpdateItemRequest, options?: MutationOptions) =>
        api.patch<Item>(`/items/${encodeURIComponent(id)}`, body, options),
      /** Back from the trash; owner only (PRD N4, V6). */
      restore: (id: string, options?: MutationOptions) =>
        api.post<Item>(`/items/${encodeURIComponent(id)}/restore`, undefined, options),
      unarchive: (id: string, options?: MutationOptions) =>
        api.post<Item>(`/items/${encodeURIComponent(id)}/unarchive`, undefined, options),
      /** To the trash, restorable for 30 days; owner only (PRD N4, V6). */
      trash: (id: string, options?: MutationOptions) =>
        api.delete(`/items/${encodeURIComponent(id)}`, options),
      /** Accessible items matching `filter`, newest first (PRD F1). */
      filter: (filter: ItemFilter, page: { cursor?: string; limit?: number } = {}) =>
        api.get<Page<Item>>("/items/filter", {
          query: { filter: JSON.stringify(filter), limit: page.limit, cursor: page.cursor },
        }),
      /** The caller's own tags on the item (PRD E2). */
      tags: (id: string) => api.get<Tag[]>(`/items/${encodeURIComponent(id)}/tags`),
      /** Links whose other end the caller can see (PRD V9). */
      links: (id: string) => api.get<ItemLink[]>(`/items/${encodeURIComponent(id)}/links`),
      /** Links `id` to `targetId`: LINK on the source, VIEW on the target (PRD B1, V4). */
      link: (id: string, targetId: string, options?: MutationOptions) =>
        api.put<ItemLink>(
          `/items/${encodeURIComponent(id)}/links/${encodeURIComponent(targetId)}`,
          undefined,
          options,
        ),
      unlink: (id: string, targetId: string, options?: MutationOptions) =>
        api.delete(
          `/items/${encodeURIComponent(id)}/links/${encodeURIComponent(targetId)}`,
          options,
        ),
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
    embeds: {
      /** An allowed embed or a link card for a pasted link (PRD Y4, ADR-0013). */
      preview: (url: string) => api.get<LinkPreview>("/embeds/preview", { query: { url } }),
    },
    notebooks: {
      /** The caller's notebooks, default first; creates the default one once (PRD Y1). */
      list: () => api.get<NotebookSummary[]>("/notebooks"),
      create: (body: CreateNotebookRequest, options?: MutationOptions) =>
        api.post<Notebook>("/notebooks", body, options),
      /** The notebook with the pages the caller can see (PRD V2). */
      get: (id: string) => api.get<Notebook>(`/notebooks/${encodeURIComponent(id)}`),
      /** The page belongs to the notebook's owner (PRD Y5). */
      createPage: (id: string, body: CreatePageRequest, options?: MutationOptions) =>
        api.post<NotebookPage>(`/notebooks/${encodeURIComponent(id)}/pages`, body, options),
      page: (id: string) => api.get<NotebookPage>(`/pages/${encodeURIComponent(id)}`),
      /** Latest process page lines of the visible items among `ids`, at most 100 (PRD S3). */
      processLines: (ids: readonly string[]) =>
        api.get<ItemStatusLine[]>("/process-lines", { query: { ids: ids.join(",") } }),
    },
    activity: {
      /** Activity across the items the caller can view, newest first (PRD A2). */
      feed: (query: { limit?: number; cursor?: string } = {}) =>
        api.get<Page<Activity>>("/activity", { query }),
      /** Reacts to a friend's completion; replaces an earlier reaction (PRD AK4). */
      react: (id: string, emoji: string, options?: MutationOptions) =>
        api.put<ActivityReaction[]>(
          `/activity/${encodeURIComponent(id)}/reaction`,
          { emoji },
          options,
        ),
      unreact: (id: string, options?: MutationOptions) =>
        api.delete<ActivityReaction[]>(`/activity/${encodeURIComponent(id)}/reaction`, options),
    },
    friends: {
      /** The caller's friends, newest first (PRD AK1). */
      list: () => api.get<Friend[]>("/friends"),
      incoming: () => api.get<IncomingFriendRequest[]>("/friend-requests/incoming"),
      outgoing: () => api.get<OutgoingFriendRequest[]>("/friend-requests/outgoing"),
      /** The same answer whether or not the address has an account (PRD AK2). */
      send: (email: string, options?: MutationOptions) =>
        api.post<OutgoingFriendRequest>("/friend-requests", { email }, options),
      accept: (id: string, options?: MutationOptions) =>
        api.post<Friend>(`/friend-requests/${encodeURIComponent(id)}/accept`, undefined, options),
      decline: (id: string, options?: MutationOptions) =>
        api.post(`/friend-requests/${encodeURIComponent(id)}/decline`, undefined, options),
      cancel: (id: string, options?: MutationOptions) =>
        api.delete(`/friend-requests/${encodeURIComponent(id)}`, options),
      unfriend: (userId: string, options?: MutationOptions) =>
        api.delete(`/friends/${encodeURIComponent(userId)}`, options),
      /** At most once a day per friend; a second one is 429 (PRD AK3). */
      nudge: (userId: string, options?: MutationOptions) =>
        api.post(`/friends/${encodeURIComponent(userId)}/nudges`, undefined, options),
      blocked: () => api.get<BlockedUser[]>("/blocks"),
      block: (userId: string, options?: MutationOptions) =>
        api.put(`/blocks/${encodeURIComponent(userId)}`, undefined, options),
      unblock: (userId: string, options?: MutationOptions) =>
        api.delete(`/blocks/${encodeURIComponent(userId)}`, options),
    },
    notifications: {
      /** The caller's notifications, newest first, and the unread count (PRD BL1). */
      list: (before?: string) => api.get<NotificationList>("/notifications", { query: { before } }),
      read: (id: string, options?: MutationOptions) =>
        api.post<Notification>(`/notifications/${encodeURIComponent(id)}/read`, undefined, options),
      readAll: (options?: MutationOptions) =>
        api.post("/notifications/read-all", undefined, options),
      /** Every type's channels (PRD BL3). */
      preferences: () => api.get<NotificationPreference[]>("/notifications/preferences"),
      setPreference: (
        kind: NotificationKind,
        body: UpdateNotificationPreferenceRequest,
        options?: MutationOptions,
      ) =>
        api.put<NotificationPreference>(
          `/notifications/preferences/${encodeURIComponent(kind)}`,
          body,
          options,
        ),
      /** The VAPID public key for Web Push; `null` when push is off on the server (T4.2.4). */
      pushKey: () => api.get<PushPublicKey>("/notifications/push/key"),
      /** Stores this browser's push subscription for the caller. */
      subscribePush: (body: PushSubscriptionRequest, options?: MutationOptions) =>
        api.put<void>("/notifications/push/subscriptions", body, options),
      /** Forgets this browser's push subscription. */
      unsubscribePush: (body: RemovePushSubscriptionRequest, options?: MutationOptions) =>
        api.post<void>("/notifications/push/subscriptions/remove", body, options),
    },
    invitations: {
      accept: (id: string, options?: MutationOptions) =>
        api.post<Invitation>(`/invitations/${encodeURIComponent(id)}/accept`, undefined, options),
      decline: (id: string, options?: MutationOptions) =>
        api.post<Invitation>(`/invitations/${encodeURIComponent(id)}/decline`, undefined, options),
    },
    search: {
      /** Items the caller can view that match `q`, best first (PRD AR1, AR2). */
      query: (q: string) => api.get<SearchHit[]>("/search", { query: { q } }),
    },
    calendar: {
      /** Task dates, routine occurrences and goal ranges of `from..=to`, at most 62 days (C1). */
      get: (from: string, to: string) => api.get<Calendar>("/calendar", { query: { from, to } }),
    },
    today: {
      /** The Today screen on the caller's local day (PRD H1-H6). */
      get: (today?: string) => api.get<Today>("/today", { query: { today } }),
      preferences: () => api.get<HomePreferences>("/today/preferences"),
      /** The forgotten threshold in days, 1 to 365 (PRD H4). */
      setPreferences: (body: UpdateHomePreferences, options?: MutationOptions) =>
        api.put<HomePreferences>("/today/preferences", body, options),
    },
    sessions: {
      /** The open work session, opened if none is (PRD W1). */
      open: (body: OpenSessionRequest, options?: MutationOptions) =>
        api.post<WorkSession>("/sessions", body, options),
      /** The open work session; a 404 when there is none. */
      current: () => api.get<WorkSession>("/sessions/current"),
      get: (id: string) => api.get<WorkSession>(`/sessions/${encodeURIComponent(id)}`),
      /** Start, pause, resume, stop or switch the item (PRD W2, W3). */
      command: (id: string, body: TimerCommand, options?: MutationOptions) =>
        api.post<WorkSession>(`/sessions/${encodeURIComponent(id)}/timer`, body, options),
      /** "Oturumu kapat" (PRD W1). */
      close: (id: string, options?: MutationOptions) =>
        api.post<WorkSession>(`/sessions/${encodeURIComponent(id)}/close`, undefined, options),
      /** Past sessions, newest first (PRD W5). */
      history: (before?: string) => api.get<SessionSummary[]>("/sessions", { query: { before } }),
      /** Work and break time per local day and item (PRD W5). */
      stats: (range: { from?: string; to?: string } = {}) =>
        api.get<SessionStats>("/sessions/stats", { query: range }),
      /** A new session shared with friends: one timer, everyone's own log (PRD W6). */
      openShared: (body: OpenSessionRequest, options?: MutationOptions) =>
        api.post<WorkSession>("/sessions/shared", body, options),
      /** The open shared sessions the caller is in. */
      shared: () => api.get<WorkSession[]>("/sessions/shared"),
      invitations: () => api.get<SessionInvitation[]>("/sessions/invitations"),
      /** Friends only (PRD V1). */
      invite: (id: string, userId: string, options?: MutationOptions) =>
        api.post(`/sessions/${encodeURIComponent(id)}/invitations`, { user_id: userId }, options),
      join: (id: string, body: JoinSessionRequest, options?: MutationOptions) =>
        api.post<WorkSession>(`/sessions/${encodeURIComponent(id)}/join`, body, options),
      /** One's own segment ends; the last one out closes the session. */
      leave: (id: string, options?: MutationOptions) =>
        api.post(`/sessions/${encodeURIComponent(id)}/leave`, undefined, options),
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
      /** Time zone, end date, reminders, completion mode and rotation (PRD R5, R6, R13). */
      update: (id: string, body: UpdateRoutineRequest, options?: MutationOptions) =>
        api.patch<Routine>(`/routines/${encodeURIComponent(id)}`, body, options),
      /** Who did what on each day; local days, both ends included (PRD R5, R6). */
      people: (id: string, query: { from?: string; to?: string } = {}) =>
        api.get<RoutinePeopleDays>(`/routines/${encodeURIComponent(id)}/completions/people`, {
          query,
        }),
      /** Who is responsible for each occurrence; the next four weeks by default (PRD R6). */
      rotation: (id: string, query: { from?: string; to?: string } = {}) =>
        api.get<RoutineRotationSchedule>(`/routines/${encodeURIComponent(id)}/rotation`, {
          query,
        }),
      /** The people on two upcoming occurrences trade places (PRD R6). */
      swap: (id: string, body: RoutineSwapRequest, options?: MutationOptions) =>
        api.post<RoutineRotationSchedule>(
          `/routines/${encodeURIComponent(id)}/rotation/swaps`,
          body,
          options,
        ),
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
