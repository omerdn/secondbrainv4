import { ApiClient, createQueryClient, endpoints } from "@secondbrain/api-client";
import { randomUUID } from "expo-crypto";
import { Platform } from "react-native";

/** API origin; set `EXPO_PUBLIC_API_URL` per environment (staging, E0.7). */
export const API_URL: string = process.env.EXPO_PUBLIC_API_URL ?? "http://localhost:8080";

/**
 * Session credentials held in memory for now. Persisting them (secure storage on native, the
 * CSRF token after a reload on web) is part of the auth screens (T0.4.5).
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
