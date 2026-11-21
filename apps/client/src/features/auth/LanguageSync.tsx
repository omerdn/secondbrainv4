import { useEffect } from "react";

import { applyLanguage, deviceLocales, pickLanguage } from "@/lib/i18n";

import { useSession } from "./session";

/**
 * Keeps the UI language in step with the session (T0.4.7): the signed-in user's profile locale,
 * otherwise the device languages, otherwise Turkish. Renders nothing.
 */
export function LanguageSync({ devices = deviceLocales }: { devices?: () => string[] }) {
  const { state } = useSession();
  const userLocale = state.status === "signedIn" ? state.user.locale : null;
  useEffect(() => {
    applyLanguage(pickLanguage({ userLocale, deviceLocales: devices() }));
  }, [userLocale, devices]);
  return null;
}
