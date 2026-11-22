import Constants from "expo-constants";
import { Platform } from "react-native";

/** Display name from `app.json`. */
export const APP_NAME: string = Constants.expoConfig?.name ?? "Routune";

/** Human-readable platform label, e.g. for the session device name (T0.3.2). */
export function platformLabel(os: typeof Platform.OS = Platform.OS): string {
  switch (os) {
    case "ios":
      return "iOS";
    case "android":
      return "Android";
    case "web":
      return "Web";
    default:
      return os;
  }
}
