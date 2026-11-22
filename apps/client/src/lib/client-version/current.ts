import Constants from "expo-constants";
import { Platform } from "react-native";

export type ClientPlatform = "ios" | "android" | "desktop";

/** The installed app's platform and version (native builds). */
export function currentClient(): Promise<{ platform: ClientPlatform; version: string } | null> {
  // `nativeAppVersion` is untyped in expo-constants.
  const version = Constants.nativeAppVersion as string | null | undefined;
  if ((Platform.OS !== "ios" && Platform.OS !== "android") || !version) {
    return Promise.resolve(null);
  }
  return Promise.resolve({ platform: Platform.OS, version });
}
