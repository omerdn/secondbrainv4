import { ApiError, errorFromResponse, networkError } from "./errors";
import { newIdempotencyKey } from "./idempotency";

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

/** How the session is presented (ARCHITECTURE §15). */
export type AuthMode =
  /** Browser: `HttpOnly` cookie sent automatically; mutations add `X-CSRF-Token`. */
  | { kind: "cookie"; csrfToken: () => string | null }
  /** Mobile/desktop app: `Authorization: Bearer <token>` from secure storage. */
  | { kind: "bearer"; token: () => string | null };

export interface ApiClientOptions {
  /** API origin, e.g. `http://localhost:8080` (no trailing slash needed). */
  baseUrl: string;
  auth: AuthMode;
  /** Injected for tests; defaults to the global `fetch`. */
  fetch?: typeof fetch;
  /** Injected for tests; defaults to {@link newIdempotencyKey}. */
  idempotencyKey?: () => string;
}

export interface RequestOptions {
  /** JSON body. */
  body?: unknown;
  /** Query string parameters; `undefined` values are skipped. */
  query?: Record<string, string | number | boolean | undefined>;
  /**
   * Idempotency key for a mutation. Pass the same key when re-submitting the same operation;
   * omitted → a fresh key per call.
   */
  idempotencyKey?: string;
  signal?: AbortSignal;
}

const MUTATIONS: ReadonlySet<HttpMethod> = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/** Whether `method` changes state (and so carries an idempotency key and CSRF token). */
export function isMutation(method: HttpMethod): boolean {
  return MUTATIONS.has(method);
}

/** Joins the base URL, path and query string. */
export function buildUrl(baseUrl: string, path: string, query?: RequestOptions["query"]): string {
  const base = baseUrl.replace(/\/+$/u, "");
  const rel = path.startsWith("/") ? path : `/${path}`;
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value !== undefined) {
      params.append(key, String(value));
    }
  }
  const qs = params.toString();
  return qs.length > 0 ? `${base}${rel}?${qs}` : `${base}${rel}`;
}

/** Typed JSON client. Every failure is an {@link ApiError}. */
export class ApiClient {
  private readonly fetchImpl: typeof fetch;
  private readonly makeKey: () => string;

  constructor(private readonly options: ApiClientOptions) {
    // Bind so a bare global `fetch` is called with the right receiver in browsers.
    this.fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.makeKey = options.idempotencyKey ?? newIdempotencyKey;
  }

  /** Sends a request and returns the parsed JSON body (`undefined` for 204). */
  async request<T>(method: HttpMethod, path: string, options: RequestOptions = {}): Promise<T> {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (options.body !== undefined) {
      headers["Content-Type"] = "application/json";
    }
    const { auth } = this.options;
    if (auth.kind === "bearer") {
      const token = auth.token();
      if (token !== null) {
        headers["Authorization"] = `Bearer ${token}`;
      }
    }
    if (isMutation(method)) {
      headers["Idempotency-Key"] = options.idempotencyKey ?? this.makeKey();
      if (auth.kind === "cookie") {
        const csrf = auth.csrfToken();
        if (csrf !== null) {
          headers["X-CSRF-Token"] = csrf;
        }
      }
    }

    let response: Response;
    try {
      response = await this.fetchImpl(buildUrl(this.options.baseUrl, path, options.query), {
        method,
        headers,
        body: options.body === undefined ? null : JSON.stringify(options.body),
        // Cookies only matter for the web client; bearer requests must not send any.
        credentials: auth.kind === "cookie" ? "include" : "omit",
        signal: options.signal ?? null,
      });
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === "AbortError") {
        throw cause;
      }
      throw networkError(cause);
    }

    const text = await response.text().catch(() => "");
    let body: unknown = undefined;
    if (text.length > 0) {
      try {
        body = JSON.parse(text);
      } catch {
        body = undefined;
      }
    }
    if (!response.ok) {
      throw errorFromResponse(response.status, body, response.headers);
    }
    if (text.length > 0 && body === undefined) {
      throw new ApiError("unknown", "response is not JSON", response.status);
    }
    return body as T;
  }

  get<T>(path: string, options?: Omit<RequestOptions, "body" | "idempotencyKey">): Promise<T> {
    return this.request<T>("GET", path, options);
  }

  post<T>(path: string, body?: unknown, options?: Omit<RequestOptions, "body">): Promise<T> {
    return this.request<T>("POST", path, { ...options, body });
  }

  patch<T>(path: string, body?: unknown, options?: Omit<RequestOptions, "body">): Promise<T> {
    return this.request<T>("PATCH", path, { ...options, body });
  }

  put<T>(path: string, body?: unknown, options?: Omit<RequestOptions, "body">): Promise<T> {
    return this.request<T>("PUT", path, { ...options, body });
  }

  delete<T = void>(path: string, options?: Omit<RequestOptions, "body">): Promise<T> {
    return this.request<T>("DELETE", path, options);
  }
}
