import {
  QueryClientProvider,
  type QueryClient,
  useMutation,
  type UseMutationOptions,
  type UseMutationResult,
} from "@tanstack/react-query";
import { createContext, type ReactNode, useContext, useMemo, useRef } from "react";

import type { ApiError } from "./errors";
import type { Endpoints } from "./endpoints";
import { keyForSubmission, newIdempotencyKey, type PendingKey } from "./idempotency";

interface ApiContextValue {
  api: Endpoints;
  idempotencyKey: () => string;
}

const ApiContext = createContext<ApiContextValue | null>(null);

export interface ApiProviderProps {
  api: Endpoints;
  queryClient: QueryClient;
  /**
   * Key generator for {@link useIdempotentMutation}. Defaults to Web Crypto; React Native
   * (Hermes) has none, so the app passes one backed by `expo-crypto`.
   */
  idempotencyKey?: () => string;
  children: ReactNode;
}

/** Provides the API and the QueryClient to the app. */
export function ApiProvider({
  api,
  queryClient,
  idempotencyKey = newIdempotencyKey,
  children,
}: ApiProviderProps) {
  const value = useMemo(() => ({ api, idempotencyKey }), [api, idempotencyKey]);
  return (
    <ApiContext.Provider value={value}>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </ApiContext.Provider>
  );
}

function useApiContext(): ApiContextValue {
  const value = useContext(ApiContext);
  if (value === null) {
    throw new Error("useApi must be used inside <ApiProvider>");
  }
  return value;
}

/** The typed endpoints from {@link ApiProvider}. */
export function useApi(): Endpoints {
  return useApiContext().api;
}

/** Stable serialization of mutation variables, used to recognise a re-submission. */
function serialize(value: unknown): string {
  return JSON.stringify(value, (_key, v: unknown) =>
    v !== null && typeof v === "object" && !Array.isArray(v)
      ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => a.localeCompare(b)))
      : v,
  );
}

/**
 * `useMutation` whose function receives an idempotency key. Re-submitting the same variables
 * after a failure reuses the key, so a request that did reach the server is not applied twice;
 * new variables or a success start a new key (see {@link keyForSubmission}).
 */
export function useIdempotentMutation<TData, TVariables>(
  mutationFn: (variables: TVariables, idempotencyKey: string) => Promise<TData>,
  options: Omit<UseMutationOptions<TData, ApiError, TVariables>, "mutationFn"> = {},
): UseMutationResult<TData, ApiError, TVariables> {
  const generate = useApiContext().idempotencyKey;
  const pending = useRef<PendingKey | null>(null);
  return useMutation<TData, ApiError, TVariables>({
    ...options,
    mutationFn: (variables) => {
      const next = keyForSubmission(pending.current, serialize(variables), generate);
      pending.current = next;
      return mutationFn(variables, next.key);
    },
    onSuccess: (...args) => {
      pending.current = null;
      return options.onSuccess?.(...args);
    },
  });
}
