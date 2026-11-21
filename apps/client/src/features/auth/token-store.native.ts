import * as SecureStore from "expo-secure-store";

const KEY = "session-token";

/**
 * Native session token in the OS secure storage (Keychain / Android Keystore), ARCHITECTURE
 * §15. Only readable while the device is unlocked, never synced to other devices.
 */
export const tokenStore = {
  get: (): Promise<string | null> => SecureStore.getItemAsync(KEY),
  set: (token: string): Promise<void> =>
    SecureStore.setItemAsync(KEY, token, {
      keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
    }),
  clear: (): Promise<void> => SecureStore.deleteItemAsync(KEY),
};
