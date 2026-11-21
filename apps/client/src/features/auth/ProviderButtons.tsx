import { Button } from "@secondbrain/ui";
import { View } from "react-native";
import { useTranslation } from "react-i18next";

import { API_URL } from "@/lib/api";

import { OrDivider, type ProviderButtonsProps } from "./provider-shared";
import { providerConfig, webProviders, webStartUrl } from "./providers";

const goTo = (url: string) => {
  window.location.assign(url);
};

/**
 * Web: the server runs the whole OAuth flow (`/auth/<provider>/start` → provider → callback),
 * sets the session cookie and redirects back to `/` (or `/login?error=<provider>`).
 */
export function ProviderButtons({
  disabled = false,
  config = providerConfig,
  navigate = goTo,
}: ProviderButtonsProps) {
  const { t } = useTranslation();
  const providers = webProviders(config);
  if (providers.length === 0) return null;
  return (
    <View className="gap-3">
      <OrDivider />
      {providers.map((provider) => (
        <Button
          key={provider}
          title={t(`auth.providers.${provider}`)}
          variant="secondary"
          disabled={disabled}
          onPress={() => {
            navigate(webStartUrl(API_URL, provider));
          }}
        />
      ))}
    </View>
  );
}
