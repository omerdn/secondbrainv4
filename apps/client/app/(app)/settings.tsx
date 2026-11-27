import { useIdempotentMutation, useApi } from "@secondbrain/api-client/react";
import { Button, Card, Text } from "@secondbrain/ui";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { AccountDeletionCard, DataExportCard } from "@/features/account";
import { useSession } from "@/features/auth";
import { Screen } from "@/features/shell";
import { LANGUAGES, type Language, resolveLanguage } from "@/lib/i18n";

/** Settings (more arrive with profile editing): account, language (T0.4.7) and sign-out. */
export default function Settings() {
  const { t } = useTranslation();
  const api = useApi();
  const { state, signOut, updateUser } = useSession();
  const current = state.status === "signedIn" ? resolveLanguage(state.user.locale) : null;
  const setLocale = useIdempotentMutation(
    (locale: Language, idempotencyKey: string) => api.me.update({ locale }, { idempotencyKey }),
    { onSuccess: updateUser },
  );

  return (
    <Screen title={t("nav.settings")} width="form">
      {state.status === "signedIn" ? (
        <Card className="gap-3">
          <View className="gap-1">
            <Text variant="label">{t("settings.account")}</Text>
            <Text variant="heading">{state.user.display_name}</Text>
            <Text tone="muted">{state.user.email}</Text>
          </View>
          <Button
            title={t("auth.session.signOut")}
            variant="dangerOutline"
            className="self-start"
            onPress={() => void signOut()}
          />
        </Card>
      ) : null}
      <Card className="gap-3">
        <Text variant="label">{t("settings.language")}</Text>
        <View className="flex-row gap-2" accessibilityRole="radiogroup">
          {LANGUAGES.map((language) => (
            <Button
              key={language}
              title={t(`settings.languages.${language}`)}
              variant={current === language ? "primary" : "secondary"}
              accessibilityRole="radio"
              accessibilityState={{ checked: current === language }}
              loading={setLocale.isPending && setLocale.variables === language}
              onPress={() => {
                if (current !== language) setLocale.mutate(language);
              }}
            />
          ))}
        </View>
        {setLocale.isError ? <Text tone="danger">{t("settings.languageFailed")}</Text> : null}
      </Card>
      {state.status === "signedIn" ? <DataExportCard /> : null}
      {state.status === "signedIn" ? <AccountDeletionCard /> : null}
    </Screen>
  );
}
