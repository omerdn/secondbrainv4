import { ApiError } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import type { ResetPasswordRequest } from "@secondbrain/api-types";
import { Button, Input } from "@secondbrain/ui";
import { useMutation } from "@tanstack/react-query";
import { useLocalSearchParams, useRouter } from "expo-router";
import { useState } from "react";

import { AuthScaffold, FormMessage } from "./AuthScaffold";
import { useSession } from "./session";
import { t } from "./strings";
import { type FieldErrors, fromApiError, hasErrors, validateReset } from "./validation";

/**
 * Target of the e-mailed link `PUBLIC_WEB_URL/reset-password?token=…`. A successful reset signs
 * out every session on the server, so the local session is forgotten too.
 */
export function ResetPasswordScreen() {
  const api = useApi();
  const session = useSession();
  const router = useRouter();
  const { token } = useLocalSearchParams<{ token?: string }>();
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [tokenRejected, setTokenRejected] = useState(false);
  const reset = useMutation({
    mutationFn: (body: ResetPasswordRequest) => api.auth.resetPassword(body),
  });

  const requestNew = (
    <Button
      title={t.reset.requestNew}
      variant="ghost"
      onPress={() => router.replace("/forgot-password")}
    />
  );

  if (token === undefined || token.length === 0 || tokenRejected) {
    return (
      <AuthScaffold title={t.reset.title}>
        <FormMessage message={tokenRejected ? t.reset.invalidToken : t.reset.missingToken} />
        {requestNew}
      </AuthScaffold>
    );
  }

  const submit = async () => {
    const found = validateReset({ password, confirm });
    setErrors(found);
    setFormError(null);
    if (hasErrors(found)) return;
    try {
      await reset.mutateAsync({ token, password });
      await session.forget();
      router.replace({ pathname: "/login", params: { reset: "done" } });
    } catch (error) {
      if (error instanceof ApiError && error.fieldMessage("token") !== undefined) {
        setTokenRejected(true);
        return;
      }
      const mapped = fromApiError(error, "other");
      setErrors(mapped.fields);
      setFormError(mapped.form);
    }
  };

  return (
    <AuthScaffold title={t.reset.title}>
      <Input
        label={t.newPassword}
        value={password}
        onChangeText={setPassword}
        error={errors.password}
        hint={t.passwordHint}
        secureTextEntry
        autoComplete="new-password"
        textContentType="newPassword"
      />
      <Input
        label={t.confirmPassword}
        value={confirm}
        onChangeText={setConfirm}
        error={errors.confirm}
        secureTextEntry
        autoComplete="new-password"
        textContentType="newPassword"
        onSubmitEditing={() => void submit()}
      />
      <FormMessage message={formError} />
      <Button title={t.reset.submit} loading={reset.isPending} onPress={() => void submit()} />
      {requestNew}
    </AuthScaffold>
  );
}
