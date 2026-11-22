import type { ConfigContext, ExpoConfig } from "expo/config";

/**
 * Build-time additions to `app.json` for sign-in providers (T0.4.5, see
 * `src/features/auth/providers.ts`). Without the variables the native app is built without
 * them, so local and CI builds need no Apple or Google credentials.
 *
 * - `EXPO_PUBLIC_APPLE_SIGN_IN=true`: adds the Sign in with Apple capability (iOS).
 * - `EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID`: registers the reversed client id as a URL scheme so
 *   Google can redirect back to the app.
 * - `APP_VERSION` / `APP_BUILD_NUMBER` (release CI, T4.2.5): the version shown to users and the
 *   increasing build number Android needs to install a new APK over the old one.
 * - Release signing: `plugins/with-release-signing.js` (ROUTUNE_KEYSTORE_* variables).
 */
export default ({ config }: ConfigContext): ExpoConfig => {
  const apple = process.env.EXPO_PUBLIC_APPLE_SIGN_IN === "true";
  const googleIos = process.env.EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID?.trim();
  const baseScheme = config.scheme ?? "secondbrain";
  const schemes = [
    ...(Array.isArray(baseScheme) ? baseScheme : [baseScheme]),
    ...(googleIos ? [googleIos.split(".").reverse().join(".")] : []),
  ];
  const version = process.env.APP_VERSION?.trim() || config.version;
  const build = Number.parseInt(process.env.APP_BUILD_NUMBER ?? "", 10);
  const buildNumber = Number.isInteger(build) && build > 0 ? build : undefined;
  return {
    ...config,
    version,
    name: config.name ?? "Routune",
    slug: config.slug ?? "secondbrain",
    scheme: schemes,
    ios: {
      ...config.ios,
      ...(apple ? { usesAppleSignIn: true } : {}),
      ...(buildNumber ? { buildNumber: String(buildNumber) } : {}),
    },
    android: { ...config.android, ...(buildNumber ? { versionCode: buildNumber } : {}) },
    plugins: [
      ...(config.plugins ?? []),
      ...(apple ? ["expo-apple-authentication"] : []),
      "./plugins/with-release-signing",
    ],
  };
};
