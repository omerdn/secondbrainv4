import type { ConfigContext, ExpoConfig } from "expo/config";

/**
 * Build-time additions to `app.json` for sign-in providers (T0.4.5, see
 * `src/features/auth/providers.ts`). Without the variables the native app is built without
 * them, so local and CI builds need no Apple or Google credentials.
 *
 * - `EXPO_PUBLIC_APPLE_SIGN_IN=true`: adds the Sign in with Apple capability (iOS).
 * - `EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID`: registers the reversed client id as a URL scheme so
 *   Google can redirect back to the app.
 */
export default ({ config }: ConfigContext): ExpoConfig => {
  const apple = process.env.EXPO_PUBLIC_APPLE_SIGN_IN === "true";
  const googleIos = process.env.EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID?.trim();
  const baseScheme = config.scheme ?? "secondbrain";
  const schemes = [
    ...(Array.isArray(baseScheme) ? baseScheme : [baseScheme]),
    ...(googleIos ? [googleIos.split(".").reverse().join(".")] : []),
  ];
  return {
    ...config,
    name: config.name ?? "Routune",
    slug: config.slug ?? "secondbrain",
    scheme: schemes,
    ios: { ...config.ios, ...(apple ? { usesAppleSignIn: true } : {}) },
    plugins: [...(config.plugins ?? []), ...(apple ? ["expo-apple-authentication"] : [])],
  };
};
