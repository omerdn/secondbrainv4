import { ApiClient, createQueryClient, endpoints } from "@secondbrain/api-client";
import { randomUUID } from "expo-crypto";
import { Platform } from "react-native";

/** Local development API, used when `EXPO_PUBLIC_API_URL` is unset. */
export const DEFAULT_API_URL = "http://localhost:8080";

/**
 * The configured API origin, or {@link DEFAULT_API_URL} when unset, empty or blank. An empty
 * value (as in `.env.example`) must not become `""`: requests would then go to the dev server
 * that served the app, which even answers `/health` (T0.5.8).
 */
export function apiUrlFrom(value: string | undefined): string {
  const trimmed = value?.trim() ?? "";
  return trimmed === "" ? DEFAULT_API_URL : trimmed;
}

/** API origin; set `EXPO_PUBLIC_API_URL` per environment (staging, E0.7). */
export const API_URL: string = apiUrlFrom(process.env.EXPO_PUBLIC_API_URL);

/**
 * Session credentials read by the API client. The session feature (`features/auth`, T0.4.5)
 * fills them: the native token is also kept in secure storage, the web CSRF token is fetched
 * again after a reload.
 */
export const credentials = {
  /** Native bearer token. */
  token: null as string | null,
  /** Web CSRF token (from `AuthResponse.csrf_token` or `GET /auth/csrf`). */
  csrfToken: null as string | null,
};

/**
 * Idempotency keys from `expo-crypto`: Hermes (iOS/Android) has no Web Crypto, and on web
 * expo-crypto uses `crypto.randomUUID`.
 */
export const idempotencyKey = (): string => randomUUID();

/** Web uses the `HttpOnly` cookie + CSRF header; native apps send a bearer token (§15). */
export const api = endpoints(
  new ApiClient({
    baseUrl: API_URL,
    idempotencyKey,
    auth:
      Platform.OS === "web"
        ? { kind: "cookie", csrfToken: () => credentials.csrfToken }
        : { kind: "bearer", token: () => credentials.token },
  }),
);

export const queryClient = createQueryClient();
