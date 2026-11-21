/**
 * Which sign-in providers the app offers (HS1). The server answers 404 for a provider it is not
 * configured for, so the buttons are opt-in per build through `EXPO_PUBLIC_*` variables
 * (inlined at build time):
 *
 * - `EXPO_PUBLIC_GOOGLE_SIGN_IN=true`: Google on web (server flow `/auth/google/start`).
 * - `EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID`: Google in the iOS app (code + PKCE → `/auth/google/native`).
 * - `EXPO_PUBLIC_APPLE_SIGN_IN=true`: Apple on web, and in the iOS app when the device supports it.
 */
export interface ProviderConfig {
  googleWeb: boolean;
  googleIosClientId: string | null;
  apple: boolean;
}

const nonEmpty = (value: string | undefined): string | null =>
  value !== undefined && value.trim().length > 0 ? value.trim() : null;

export const providerConfig: ProviderConfig = {
  googleWeb: process.env.EXPO_PUBLIC_GOOGLE_SIGN_IN === "true",
  googleIosClientId: nonEmpty(process.env.EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID),
  apple: process.env.EXPO_PUBLIC_APPLE_SIGN_IN === "true",
};

export type Provider = "google" | "apple";

/** Providers offered on web, in display order. */
export function webProviders(config: ProviderConfig): Provider[] {
  return [
    ...(config.googleWeb ? ["google" as const] : []),
    ...(config.apple ? ["apple" as const] : []),
  ];
}

/** Web sign-in starts on the server, which redirects to the provider (identity `*_start`). */
export function webStartUrl(apiUrl: string, provider: Provider): string {
  return `${apiUrl.replace(/\/+$/u, "")}/auth/${provider}/start`;
}

/** `?error=<provider>` set by the server when a web sign-in fails (identity `finish_web_sign_in`). */
export function providerFromError(value: unknown): Provider | null {
  return value === "google" || value === "apple" ? value : null;
}

/**
 * Google iOS client ids (`<n>-<x>.apps.googleusercontent.com`) come with a URL scheme that is the
 * reversed id; Google redirects the app to `<scheme>:/oauthredirect`.
 */
export function googleRedirectScheme(clientId: string): string {
  return clientId.split(".").reverse().join(".");
}

export function googleRedirectUri(clientId: string): string {
  return `${googleRedirectScheme(clientId)}:/oauthredirect`;
}
