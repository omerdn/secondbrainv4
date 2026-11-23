/**
 * Typed wrappers for the endpoints that exist today (identity, T0.3.x). Types come from
 * `@secondbrain/api-types` (generated from the Rust DTOs), so a server change that breaks the
 * contract fails the client typecheck.
 */

import type {
  AccessExplanation,
  AppleNativeRequest,
  AuthResponse,
  CsrfResponse,
  CurrentUser,
  ForgotPasswordRequest,
  GoogleNativeRequest,
  Health,
  Item,
  ItemLink,
  LoginRequest,
  RegisterRequest,
  ResetPasswordRequest,
  SessionInfo,
  Tag,
  UpdateProfileRequest,
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
      /** The caller's own tags on the item (PRD E2). */
      tags: (id: string) => api.get<Tag[]>(`/items/${encodeURIComponent(id)}/tags`),
      /** Links whose other end the caller can see (PRD V9). */
      links: (id: string) => api.get<ItemLink[]>(`/items/${encodeURIComponent(id)}/links`),
    },
    access: {
      /** "Why am I seeing this?" (PRD V11). */
      explanation: (itemId: string) =>
        api.get<AccessExplanation>(`/access/items/${encodeURIComponent(itemId)}/explanation`),
    },
  };
}

export type Endpoints = ReturnType<typeof endpoints>;
