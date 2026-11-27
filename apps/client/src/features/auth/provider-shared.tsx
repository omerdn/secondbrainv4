import type { AuthResponse } from "@secondbrain/api-types";
import { Text } from "@secondbrain/ui";
import { View } from "react-native";
import { useTranslation } from "react-i18next";

import type { ProviderConfig } from "./providers";

/** Props of `ProviderButtons` (web and native variants). */
export interface ProviderButtonsProps {
  /** Native only: a provider sign-in finished in the app. */
  onSignedIn: (response: AuthResponse) => Promise<void>;
  onError: (message: string) => void;
  /** A sign-in held for legal consent (HS5): open the consent screen with this ticket. */
  onConsent?: (ticket: string) => void;
  disabled?: boolean;
  config?: ProviderConfig;
  /** Web only, injected for tests; defaults to a full-page navigation. */
  navigate?: (url: string) => void;
}

/** "or" line between the e-mail form and the provider buttons. */
export function OrDivider() {
  const { t } = useTranslation();
  return (
    <View className="flex-row items-center gap-3">
      <View className="h-px flex-1 bg-border" />
      <Text variant="caption" tone="muted">
        {t("auth.providers.or")}
      </Text>
      <View className="h-px flex-1 bg-border" />
    </View>
  );
}
