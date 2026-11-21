import type { ErrorBody, ErrorCode, FieldError } from "@secondbrain/api-types";

/**
 * Why a request failed.
 * - Server codes ({@link ErrorCode}) come from the `{ "error": … }` body (kernel::AppError).
 * - `network`: no response at all (offline, DNS, server down, CORS). The connectivity layer
 *   (BP1–BP5, ARCHITECTURE §10.2) reacts to these.
 * - `unknown`: a response that is not the API's error format (proxy page, empty 502…).
 */
export type ApiErrorCode = ErrorCode | "network" | "unknown";

/** Error thrown for every failed request; never a raw `Response` or `TypeError`. */
export class ApiError extends Error {
  override readonly name = "ApiError";

  constructor(
    /** See {@link ApiErrorCode}. */
    readonly code: ApiErrorCode,
    message: string,
    /** HTTP status; `0` for network errors. */
    readonly status: number,
    /** Field errors of a `validation_failed` response, grouped by field. */
    readonly fields: Readonly<Record<string, readonly FieldError[]>> = {},
    /** Seconds to wait before retrying (`Retry-After` of a 429). */
    readonly retryAfter: number | null = null,
  ) {
    super(message);
  }

  /** First error message for a form field, e.g. for `<Input error={…} />`. */
  fieldMessage(field: string): string | undefined {
    return this.fields[field]?.[0]?.message;
  }

  /**
   * Whether repeating the same request may succeed: no response, a server error, or a rate
   * limit. 4xx answers (validation, permission, not found, conflict) will not change.
   */
  get retryable(): boolean {
    return (
      this.code === "network" ||
      this.code === "internal" ||
      this.code === "rate_limited" ||
      this.code === "unknown"
    );
  }
}

const SERVER_CODES: ReadonlySet<string> = new Set<ErrorCode>([
  "not_found",
  "unauthenticated",
  "forbidden",
  "validation_failed",
  "conflict",
  "rate_limited",
  "internal",
]);

function isErrorBody(value: unknown): value is ErrorBody {
  if (typeof value !== "object" || value === null || !("error" in value)) {
    return false;
  }
  const error: unknown = value.error;
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string" &&
    SERVER_CODES.has(error.code) &&
    "message" in error &&
    typeof error.message === "string"
  );
}

function parseRetryAfter(header: string | null): number | null {
  if (header === null) {
    return null;
  }
  const seconds = Number.parseInt(header, 10);
  return Number.isFinite(seconds) && seconds >= 0 ? seconds : null;
}

/** Maps a non-2xx response and its (possibly unparsable) body to an {@link ApiError}. */
export function errorFromResponse(status: number, body: unknown, headers: Headers): ApiError {
  const retryAfter = parseRetryAfter(headers.get("retry-after"));
  if (!isErrorBody(body)) {
    return new ApiError("unknown", `unexpected response (HTTP ${status})`, status, {}, retryAfter);
  }
  const { code, message, fields } = body.error;
  const grouped: Record<string, FieldError[]> = {};
  for (const field of fields ?? []) {
    (grouped[field.field] ??= []).push(field);
  }
  return new ApiError(code, message, status, grouped, retryAfter);
}

/** The error for a request that got no response. */
export function networkError(cause: unknown): ApiError {
  const error = new ApiError("network", "network request failed", 0);
  (error as { cause?: unknown }).cause = cause;
  return error;
}
