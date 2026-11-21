import type { AuthResponse } from "@secondbrain/api-types";
import { Text } from "@secondbrain/ui";
import { View } from "react-native";

import type { ProviderConfig } from "./providers";
import { t } from "./strings";

/** Props of `ProviderButtons` (web and native variants). */
export interface ProviderButtonsProps {
  /** Native only: a provider sign-in finished in the app. */
  onSignedIn: (response: AuthResponse) => Promise<void>;
  onError: (message: string) => void;
  disabled?: boolean;
  config?: ProviderConfig;
  /** Web only, injected for tests; defaults to a full-page navigation. */
  navigate?: (url: string) => void;
}

/** "or" line between the e-mail form and the provider buttons. */
export function OrDivider() {
  return (
    <View className="flex-row items-center gap-3">
      <View className="h-px flex-1 bg-border" />
      <Text variant="caption" tone="muted">
        {t.providers.or}
      </Text>
      <View className="h-px flex-1 bg-border" />
    </View>
  );
}
