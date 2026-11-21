import { getLocales } from "expo-localization";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { Platform } from "react-native";

import { en } from "./locales/en";
import { tr } from "./locales/tr";

/**
 * Client i18n (T0.4.7): Turkish is the default, English the second language. Catalogs live in
 * `./locales`; keys are type-checked through `i18next.d.ts`.
 *
 * Language choice: the signed-in user's profile `locale` (`PATCH /me`), otherwise the device
 * languages in order, otherwise Turkish.
 */
export const LANGUAGES = ["tr", "en"] as const;
export type Language = (typeof LANGUAGES)[number];
export const DEFAULT_LANGUAGE: Language = "tr";

/** Supported language of a BCP 47 tag (`en-US` → `en`), or `null`. */
export function resolveLanguage(tag: string | null | undefined): Language | null {
  const base = tag?.trim().toLowerCase().split(/[-_]/u)[0];
  return LANGUAGES.find((language) => language === base) ?? null;
}

export function pickLanguage({
  userLocale,
  deviceLocales,
}: {
  userLocale?: string | null;
  deviceLocales: readonly string[];
}): Language {
  return (
    resolveLanguage(userLocale) ??
    deviceLocales.map(resolveLanguage).find((language) => language !== null) ??
    DEFAULT_LANGUAGE
  );
}

/** The device's preferred languages (BCP 47), most preferred first. */
export function deviceLocales(): string[] {
  return getLocales().map((locale) => locale.languageTag);
}

export const resources = { tr: { translation: tr }, en: { translation: en } } as const;

export const i18n = i18next.createInstance();

void i18n.use(initReactI18next).init({
  resources,
  lng: pickLanguage({ deviceLocales: deviceLocales() }),
  fallbackLng: DEFAULT_LANGUAGE,
  supportedLngs: [...LANGUAGES],
  interpolation: { escapeValue: false }, // React escapes.
  returnNull: false,
  initAsync: false,
});

/** Screen readers and browser translation read the page language from `<html lang>`. */
function syncDocumentLanguage(language: string) {
  if (Platform.OS === "web" && typeof document !== "undefined") {
    document.documentElement.lang = language;
  }
}
syncDocumentLanguage(i18n.language);
i18n.on("languageChanged", syncDocumentLanguage);

/** Switches the UI language if it differs from the current one. */
export function applyLanguage(language: Language): void {
  if (i18n.language !== language) void i18n.changeLanguage(language);
}
