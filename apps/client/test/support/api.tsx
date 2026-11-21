import { ApiClient, endpoints } from "@secondbrain/api-client";
import { ApiProvider } from "@secondbrain/api-client/react";
import { ThemeProvider } from "@secondbrain/ui";
import { QueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { SessionProvider } from "@/features/auth";
import type { TokenStore, Transport } from "@/features/auth/session";
import { credentials } from "@/lib/api";

export interface Call {
  method: string;
  path: string;
  body: unknown;
  headers: Record<string, string>;
}

type Handler = (call: Call) => Response | Promise<Response>;

export const json = (status: number, body?: unknown, headers: Record<string, string> = {}) =>
  new Response(body === undefined ? null : JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", ...headers },
  });

export const apiError = (status: number, code: string, fields: unknown[] = []) =>
  json(status, {
    error: { code, message: code, ...(fields.length > 0 ? { fields } : {}) },
  });

export const user = {
  id: "u1",
  email: "ali@example.com",
  display_name: "Ali",
  avatar_url: null,
  timezone: "Europe/Istanbul",
  locale: "tr",
  created_at: "2026-01-01T00:00:00Z",
  email_verified: false,
};

/**
 * A fake server: `routes` maps `"METHOD /path"` to a handler; anything else is 404. Every call
 * is recorded in `calls`.
 */
export function fakeServer(routes: Record<string, Handler>) {
  const calls: Call[] = [];
  const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(input instanceof Request ? input.url : input.toString());
    const call: Call = {
      method: init?.method ?? "GET",
      path: url.pathname + url.search,
      body: typeof init?.body === "string" ? JSON.parse(init.body) : undefined,
      headers: (init?.headers ?? {}) as Record<string, string>,
    };
    calls.push(call);
    const handler = routes[`${call.method} ${url.pathname}`];
    return handler === undefined ? apiError(404, "not_found") : handler(call);
  }) as typeof fetch;
  const api = endpoints(
    new ApiClient({
      baseUrl: "http://api.test",
      fetch: fetchImpl,
      idempotencyKey: () => "key",
      auth: { kind: "cookie", csrfToken: () => credentials.csrfToken },
    }),
  );
  return { api, calls };
}

export function memoryTokenStore(initial: string | null = null): TokenStore & {
  value: string | null;
} {
  const store = {
    value: initial,
    get: () => Promise.resolve(store.value),
    set: (token: string) => {
      store.value = token;
      return Promise.resolve();
    },
    clear: () => {
      store.value = null;
      return Promise.resolve();
    },
  };
  return store;
}

export function Providers({
  api,
  transport,
  tokenStore = memoryTokenStore(),
  children,
}: {
  api: ReturnType<typeof fakeServer>["api"];
  transport: Transport;
  tokenStore?: TokenStore;
  children: ReactNode;
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return (
    <ApiProvider api={api} queryClient={queryClient} idempotencyKey={() => "key"}>
      <SessionProvider transport={transport} tokenStore={tokenStore}>
        <ThemeProvider>{children}</ThemeProvider>
      </SessionProvider>
    </ApiProvider>
  );
}

export function resetCredentials() {
  credentials.token = null;
  credentials.csrfToken = null;
}
