import { describe, expect, it, vi } from "vitest";

import {
  ApiClient,
  ApiError,
  buildUrl,
  createQueryClient,
  endpoints,
  keyForSubmission,
  MAX_QUERY_RETRIES,
  newIdempotencyKey,
  shouldRetryQuery,
} from "../src";

interface Captured {
  url: string;
  init: RequestInit;
}

/** A fetch stub that records requests and answers with `response`. */
function stubFetch(response: () => Response | Promise<Response>) {
  const calls: Captured[] = [];
  const fetchImpl = vi.fn((url: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: url instanceof Request ? url.url : url.toString(), init: init ?? {} });
    return Promise.resolve(response());
  });
  return { calls, fetchImpl: fetchImpl as unknown as typeof fetch };
}

const json = (status: number, body: unknown, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json", ...headers },
  });

function header(call: Captured | undefined, name: string): string | undefined {
  return (call?.init.headers as Record<string, string> | undefined)?.[name];
}

describe("buildUrl", () => {
  it("joins base, path and query, skipping undefined values", () => {
    expect(buildUrl("http://api.test/", "/me")).toBe("http://api.test/me");
    expect(buildUrl("http://api.test", "items", { q: "a b", limit: 10, cursor: undefined })).toBe(
      "http://api.test/items?q=a+b&limit=10",
    );
  });
});

describe("ApiClient — bearer (native)", () => {
  it("sends the token, JSON body and an idempotency key on mutations", async () => {
    const { calls, fetchImpl } = stubFetch(() => json(200, { ok: true }));
    const api = new ApiClient({
      baseUrl: "http://api.test",
      auth: { kind: "bearer", token: () => "tok" },
      fetch: fetchImpl,
      idempotencyKey: () => "key-1",
    });
    await api.patch("/me", { locale: "en" });
    const call = calls[0];
    expect(call?.url).toBe("http://api.test/me");
    expect(call?.init.method).toBe("PATCH");
    expect(call?.init.body).toBe('{"locale":"en"}');
    expect(call?.init.credentials).toBe("omit");
    expect(header(call, "Authorization")).toBe("Bearer tok");
    expect(header(call, "Content-Type")).toBe("application/json");
    expect(header(call, "Idempotency-Key")).toBe("key-1");
    expect(header(call, "X-CSRF-Token")).toBeUndefined();
  });

  it("uses a caller-supplied key and never adds one to reads", async () => {
    const { calls, fetchImpl } = stubFetch(() => json(200, {}));
    const api = new ApiClient({
      baseUrl: "http://api.test",
      auth: { kind: "bearer", token: () => null },
      fetch: fetchImpl,
    });
    await api.post("/x", {}, { idempotencyKey: "retry-key" });
    await api.get("/x");
    expect(header(calls[0], "Idempotency-Key")).toBe("retry-key");
    expect(header(calls[1], "Idempotency-Key")).toBeUndefined();
    expect(header(calls[1], "Authorization")).toBeUndefined();
    expect(calls[1]?.init.body).toBeNull();
  });
});

describe("ApiClient — cookie (web)", () => {
  it("includes credentials and the CSRF token on mutations only", async () => {
    const { calls, fetchImpl } = stubFetch(() => json(200, {}));
    const api = new ApiClient({
      baseUrl: "http://api.test",
      auth: { kind: "cookie", csrfToken: () => "csrf-1" },
      fetch: fetchImpl,
    });
    await api.get("/me");
    await api.delete("/auth/sessions/1");
    expect(calls[0]?.init.credentials).toBe("include");
    expect(header(calls[0], "X-CSRF-Token")).toBeUndefined();
    expect(header(calls[1], "X-CSRF-Token")).toBe("csrf-1");
    expect(header(calls[1], "Authorization")).toBeUndefined();
  });
});

describe("responses", () => {
  const api = (response: () => Response | Promise<Response>) =>
    new ApiClient({
      baseUrl: "http://api.test",
      auth: { kind: "bearer", token: () => "t" },
      fetch: stubFetch(response).fetchImpl,
    });

  it("returns undefined for 204", async () => {
    await expect(
      api(() => new Response(null, { status: 204 })).post("/auth/logout"),
    ).resolves.toBeUndefined();
  });

  it("maps validation errors with fields grouped by field", async () => {
    const error = await api(() =>
      json(422, {
        error: {
          code: "validation_failed",
          message: "validation failed",
          fields: [
            { field: "email", code: "invalid", message: "must be an e-mail address" },
            { field: "email", code: "too_long", message: "too long" },
            { field: "password", code: "too_short", message: "too short" },
          ],
        },
      }),
    )
      .post("/auth/register", {})
      .catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    const apiError = error as ApiError;
    expect(apiError.code).toBe("validation_failed");
    expect(apiError.status).toBe(422);
    expect(apiError.fields["email"]).toHaveLength(2);
    expect(apiError.fieldMessage("email")).toBe("must be an e-mail address");
    expect(apiError.fieldMessage("password")).toBe("too short");
    expect(apiError.fieldMessage("nope")).toBeUndefined();
    expect(apiError.retryable).toBe(false);
  });

  it.each([
    [401, "unauthenticated", false],
    [403, "forbidden", false],
    [404, "not_found", false],
    [409, "conflict", false],
    [500, "internal", true],
  ] as const)("maps HTTP %i to %s (retryable: %s)", async (status, code, retryable) => {
    const error = (await api(() => json(status, { error: { code, message: "m" } }))
      .get("/x")
      .catch((e: unknown) => e)) as ApiError;
    expect([error.code, error.status, error.retryable]).toEqual([code, status, retryable]);
  });

  it("carries the consent ticket of a held sign-in (HS5)", async () => {
    const error = (await api(() =>
      json(409, {
        error: { code: "consent_required", message: "consent required", consent_ticket: "tk" },
      }),
    )
      .post("/auth/login", {})
      .catch((e: unknown) => e)) as ApiError;
    expect([error.code, error.consentTicket, error.retryable]).toEqual([
      "consent_required",
      "tk",
      false,
    ]);
  });

  it("reads Retry-After of a 429", async () => {
    const error = (await api(() =>
      json(429, { error: { code: "rate_limited", message: "slow down" } }, { "retry-after": "42" }),
    )
      .post("/auth/login", {})
      .catch((e: unknown) => e)) as ApiError;
    expect(error.code).toBe("rate_limited");
    expect(error.retryAfter).toBe(42);
    expect(error.retryable).toBe(true);
  });

  it("treats a non-API error page as unknown", async () => {
    const error = (await api(() => new Response("<html>Bad gateway</html>", { status: 502 }))
      .get("/x")
      .catch((e: unknown) => e)) as ApiError;
    expect([error.code, error.status]).toEqual(["unknown", 502]);
  });

  it("rejects a 200 that is not JSON", async () => {
    const error = (await api(() => new Response("not json", { status: 200 }))
      .get("/x")
      .catch((e: unknown) => e)) as ApiError;
    expect(error.code).toBe("unknown");
  });

  it("turns a failed fetch into a network error", async () => {
    const error = (await api(() => Promise.reject(new TypeError("Failed to fetch")))
      .get("/x")
      .catch((e: unknown) => e)) as ApiError;
    expect([error.code, error.status, error.retryable]).toEqual(["network", 0, true]);
    expect((error as { cause?: unknown }).cause).toBeInstanceOf(TypeError);
  });

  it("lets an abort through unchanged", async () => {
    const abort = new DOMException("aborted", "AbortError");
    await expect(api(() => Promise.reject(abort)).get("/x")).rejects.toBe(abort);
  });
});

describe("endpoints", () => {
  it("call the right method and path", async () => {
    const { calls, fetchImpl } = stubFetch(() => json(200, {}));
    const api = endpoints(
      new ApiClient({
        baseUrl: "http://api.test",
        auth: { kind: "bearer", token: () => "t" },
        fetch: fetchImpl,
      }),
    );
    await api.me.get();
    await api.me.update({ timezone: "Europe/Istanbul" }, { idempotencyKey: "k" });
    await api.auth.revokeSession("a/b");
    await api.auth.googleNative({
      code: "c",
      code_verifier: "v",
      redirect_uri: "app:/cb",
      client_id: "ios",
    });
    await api.auth.appleNative({ code: "c", client_id: "app.bundle" });
    expect(calls.map((c) => `${c.init.method} ${c.url}`)).toEqual([
      "GET http://api.test/me",
      "PATCH http://api.test/me",
      "DELETE http://api.test/auth/sessions/a%2Fb",
      "POST http://api.test/auth/google/native",
      "POST http://api.test/auth/apple/native",
    ]);
    expect(header(calls[1], "Idempotency-Key")).toBe("k");
  });
});

describe("idempotency keys", () => {
  it("are RFC 9562 v4 UUIDs and unique", () => {
    const keys = new Set(Array.from({ length: 100 }, newIdempotencyKey));
    expect(keys.size).toBe(100);
    for (const key of keys) {
      expect(key).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u);
    }
  });

  it("fall back to getRandomValues without randomUUID", () => {
    const original = globalThis.crypto;
    vi.stubGlobal("crypto", { getRandomValues: original.getRandomValues.bind(original) });
    try {
      expect(newIdempotencyKey()).toMatch(/^[0-9a-f-]{36}$/u);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("are reused for the same input and renewed for a different one", () => {
    let n = 0;
    const gen = () => `k${++n}`;
    const first = keyForSubmission(null, '{"a":1}', gen);
    expect(first.key).toBe("k1");
    expect(keyForSubmission(first, '{"a":1}', gen)).toBe(first);
    expect(keyForSubmission(first, '{"a":2}', gen).key).toBe("k2");
  });
});

describe("query defaults", () => {
  it("retry only retryable errors, a bounded number of times", () => {
    const network = new ApiError("network", "x", 0);
    const invalid = new ApiError("validation_failed", "x", 422);
    expect(shouldRetryQuery(0, network)).toBe(true);
    expect(shouldRetryQuery(MAX_QUERY_RETRIES, network)).toBe(false);
    expect(shouldRetryQuery(0, invalid)).toBe(false);
    expect(shouldRetryQuery(0, new Error("other"))).toBe(false);
  });

  it("never retry mutations and never pause for offline", () => {
    const defaults = createQueryClient().getDefaultOptions();
    expect(defaults.mutations?.retry).toBe(false);
    expect(defaults.mutations?.networkMode).toBe("always");
    expect(defaults.queries?.networkMode).toBe("always");
  });
});
