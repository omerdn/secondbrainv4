import { ApiError } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import { Button, Text } from "@secondbrain/ui";
import { useMutation } from "@tanstack/react-query";
import { useLocalSearchParams, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { AuthScaffold, FormMessage } from "./AuthScaffold";
import { ConsentCheckbox, acceptancesOf, useLegalDocuments } from "./legal";
import { useSession } from "./session";

/**
 * A sign-in held until the current legal texts are accepted (HS5, T4.3.3): new provider
 * accounts and anyone after a new text version. Accepting finishes the sign-in.
 */
export function ConsentScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const session = useSession();
  const router = useRouter();
  const { ticket } = useLocalSearchParams<{ ticket?: string }>();
  const documents = useLegalDocuments();
  const [checked, setChecked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const consent = useMutation({
    mutationFn: () =>
      api.auth.consent({
        ticket: ticket ?? "",
        accepted: acceptancesOf(documents.data ?? []),
        client: session.client,
      }),
  });

  const submit = async () => {
    setError(null);
    if (!checked) {
      setError(t("auth.legal.required"));
      return;
    }
    try {
      await session.signedIn(await consent.mutateAsync());
    } catch (failure) {
      setError(
        failure instanceof ApiError && failure.code === "unauthenticated"
          ? t("auth.legal.expired")
          : t("auth.errors.generic"),
      );
    }
  };

  return (
    <AuthScaffold title={t("auth.legal.title")}>
      <Text tone="muted">{t("auth.legal.intro")}</Text>
      {documents.data ? (
        <ConsentCheckbox documents={documents.data} checked={checked} onChange={setChecked} />
      ) : null}
      <FormMessage message={error} />
      <Button
        title={t("auth.legal.continue")}
        loading={consent.isPending}
        disabled={!ticket || !documents.data}
        onPress={() => void submit()}
      />
      <Button
        title={t("auth.legal.back")}
        variant="ghost"
        onPress={() => router.replace("/login")}
      />
    </AuthScaffold>
  );
}
