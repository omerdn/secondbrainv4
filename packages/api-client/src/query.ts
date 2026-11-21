import { QueryClient } from "@tanstack/react-query";

import { ApiError } from "./errors";

/** Query retries for retryable failures (network, 5xx, 429). */
export const MAX_QUERY_RETRIES = 2;

/**
 * Whether a failed query should be retried: only {@link ApiError.retryable} errors, at most
 * {@link MAX_QUERY_RETRIES} times. 4xx answers are final.
 */
export function shouldRetryQuery(failureCount: number, error: unknown): boolean {
  return error instanceof ApiError && error.retryable && failureCount < MAX_QUERY_RETRIES;
}

/**
 * The app's QueryClient (ARCHITECTURE §2: server state lives in TanStack Query, invalidated by
 * WebSocket events in T0.5.5).
 *
 * - Online-only (ADR-0003): `networkMode: "always"` so requests are attempted and fail visibly
 *   instead of being paused for an offline queue; the connectivity layer handles reconnects.
 * - Mutations are not retried automatically: the user sees the error, keeps the form and
 *   re-submits with the same idempotency key.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        networkMode: "always",
        staleTime: 30_000,
        retry: shouldRetryQuery,
      },
      mutations: {
        networkMode: "always",
        retry: false,
      },
    },
  });
}
