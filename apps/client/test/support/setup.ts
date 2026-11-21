/**
 * Tests run in Turkish, the default language, regardless of the machine's locale. Tests that
 * need another language switch explicitly.
 */
jest.mock("expo-localization", () => ({
  getLocales: () => [{ languageTag: "tr-TR", languageCode: "tr" }],
}));
