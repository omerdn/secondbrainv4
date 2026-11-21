import { useApi } from "@secondbrain/api-client/react";
import { Button, Text } from "@secondbrain/ui";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { useSession } from "./session";
import { fromApiError } from "./validation";

/** "Send the verification e-mail again" with its result (`POST /auth/email/verification`). */
export function ResendVerification() {
  const { t } = useTranslation();
  const api = useApi();
  const resend = useMutation({ mutationFn: () => api.auth.resendVerification() });
  if (resend.isSuccess) {
    return (
      <Text tone="success" accessibilityLiveRegion="polite">
        {t("auth.verify.resent")}
      </Text>
    );
  }
  return (
    <View className="gap-1">
      <Button
        title={t("auth.verify.resend")}
        variant="secondary"
        size="sm"
        loading={resend.isPending}
        onPress={() => resend.mutate()}
      />
      {resend.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {fromApiError(resend.error, "other").form}
        </Text>
      ) : null}
    </View>
  );
}

/** Reminder above signed-in screens until the address is verified. */
export function VerifyEmailBanner() {
  const { t } = useTranslation();
  const { state } = useSession();
  if (state.status !== "signedIn" || state.user.email_verified) return null;
  return (
    <View
      testID="verify-email-banner"
      className="gap-2 border-b border-border bg-surface-raised px-4 py-3 md:flex-row md:items-center md:justify-between"
    >
      <Text className="md:flex-1">{t("auth.verify.banner", { email: state.user.email })}</Text>
      <ResendVerification />
    </View>
  );
}
