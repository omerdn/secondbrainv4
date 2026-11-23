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
  CreateSavedViewRequest,
  CreateTagRequest,
  CreateTrackerEntryRequest,
  CreateTrackerRequest,
  AuthResponse,
  CsrfResponse,
  CurrentUser,
  ForgotPasswordRequest,
  GoogleNativeRequest,
  Health,
  Item,
  ItemFilter,
  ItemLink,
  ItemListState,
  LoginRequest,
  Page,
  RegisterRequest,
  ResetPasswordRequest,
  SavedView,
  SessionInfo,
  Tag,
  Tracker,
  TrackerEntry,
  TrackerSummary,
  UpdateProfileRequest,
  UpdateSavedViewRequest,
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
