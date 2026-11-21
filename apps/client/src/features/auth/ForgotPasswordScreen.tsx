import { useApi } from "@secondbrain/api-client/react";
import { Button, Input, Text } from "@secondbrain/ui";
import { useMutation } from "@tanstack/react-query";
import { useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { AuthScaffold, FormMessage } from "./AuthScaffold";
import { type FieldErrors, fromApiError, hasErrors, validateForgot } from "./validation";

/**
 * Requests a reset link. The server answers 202 whether or not the address has an account, so
 * the screen always shows the same confirmation (no account discovery).
 */
export function ForgotPasswordScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const forgot = useMutation({
    mutationFn: (address: string) => api.auth.forgotPassword({ email: address }),
  });

  const submit = async () => {
    const found = validateForgot({ email });
    setErrors(found);
    setFormError(null);
    if (hasErrors(found)) return;
    try {
      await forgot.mutateAsync(email.trim());
    } catch (error) {
      const mapped = fromApiError(error, "other");
      setErrors(mapped.fields);
      setFormError(mapped.form);
    }
  };

  return (
    <AuthScaffold title={t("auth.forgot.title")}>
      {forgot.isSuccess ? (
        <FormMessage message={t("auth.forgot.sent")} tone="success" />
      ) : (
        <>
          <Text tone="muted">{t("auth.forgot.intro")}</Text>
          <Input
            label={t("auth.email")}
            value={email}
            onChangeText={setEmail}
            error={errors.email}
            autoCapitalize="none"
            autoComplete="email"
            keyboardType="email-address"
            inputMode="email"
            onSubmitEditing={() => void submit()}
          />
          <FormMessage message={formError} />
          <Button
            title={t("auth.forgot.submit")}
            loading={forgot.isPending}
            onPress={() => void submit()}
          />
        </>
      )}
      <Button
        title={t("auth.forgot.back")}
        variant="ghost"
        onPress={() => router.replace("/login")}
      />
    </AuthScaffold>
  );
}
