import { ApiError } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import { Button } from "@secondbrain/ui";
import { useMutation } from "@tanstack/react-query";
import { useLocalSearchParams, useRouter } from "expo-router";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { ActivityIndicator } from "react-native";

import { AuthScaffold, FormMessage } from "./AuthScaffold";
import { useSession } from "./session";
import { ResendVerification } from "./VerifyEmailBanner";
import { fromApiError } from "./validation";

/**
 * Target of the e-mailed link `PUBLIC_WEB_URL/verify-email?token=…` (T0.3.3). Works signed in
 * or out. The token is single-use, so it is sent exactly once per screen.
 */
export function VerifyEmailScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const { state, updateUser } = useSession();
  const { token } = useLocalSearchParams<{ token?: string }>();
  const verify = useMutation({
    mutationFn: (value: string) => api.auth.verifyEmail({ token: value }),
  });
  const sent = useRef(false);

  useEffect(() => {
    if (sent.current || token === undefined || token.length === 0) return;
    sent.current = true;
    verify.mutate(token);
  }, [token, verify]);

  // The signed-in user's flag flips without another round trip.
  const signedInUser = state.status === "signedIn" ? state.user : null;
  useEffect(() => {
    if (verify.isSuccess && signedInUser !== null && !signedInUser.email_verified) {
      updateUser({ ...signedInUser, email_verified: true });
    }
  }, [verify.isSuccess, signedInUser, updateUser]);

  const next =
    signedInUser !== null ? (
      <Button title={t("auth.verify.continue")} onPress={() => router.replace("/")} />
    ) : (
      <Button title={t("auth.verify.toLogin")} onPress={() => router.replace("/login")} />
    );

  if (token === undefined || token.length === 0) {
    return (
      <AuthScaffold title={t("auth.verify.title")}>
        <FormMessage message={t("auth.verify.missingToken")} />
        {signedInUser !== null && !signedInUser.email_verified ? <ResendVerification /> : null}
        {next}
      </AuthScaffold>
    );
  }

  const tokenRejected =
    verify.error instanceof ApiError && verify.error.fieldMessage("token") !== undefined;
  return (
    <AuthScaffold title={t("auth.verify.title")}>
      {verify.isSuccess ? (
        <FormMessage message={t("auth.verify.done")} tone="success" />
      ) : verify.isError ? (
        <FormMessage
          message={
            tokenRejected
              ? t("auth.verify.invalidToken")
              : (fromApiError(verify.error, "other").form ?? t("auth.errors.generic"))
          }
        />
      ) : (
        <>
          <ActivityIndicator accessibilityLabel={t("auth.verify.verifying")} />
          <FormMessage message={t("auth.verify.verifying")} tone="muted" />
        </>
      )}
      {verify.isError && signedInUser !== null && !signedInUser.email_verified ? (
        <ResendVerification />
      ) : null}
      {verify.isPending ? null : next}
    </AuthScaffold>
  );
}
