/**
 * Tests run in Turkish, the default language, regardless of the machine's locale. Tests that
 * need another language switch explicitly.
 */
jest.mock("expo-localization", () => ({
  getLocales: () => [{ languageTag: "tr-TR", languageCode: "tr" }],
}));

// NetInfo's native module does not exist under Jest.
jest.mock("@react-native-community/netinfo", () => ({
  __esModule: true,
  default: {
    configure: () => undefined,
    addEventListener: () => () => undefined,
    fetch: () => Promise.resolve({ isConnected: true }),
  },
}));
