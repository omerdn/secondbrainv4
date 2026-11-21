import { useApi } from "@secondbrain/api-client/react";
import type { LoginRequest } from "@secondbrain/api-types";
import { Button, Input } from "@secondbrain/ui";
import { useMutation } from "@tanstack/react-query";
import { useLocalSearchParams, useRouter } from "expo-router";
import { useState } from "react";
import { View } from "react-native";

import { platformLabel } from "@/lib/app-info";

import { AuthScaffold, FormMessage } from "./AuthScaffold";
import { ProviderButtons } from "./ProviderButtons";
import { providerFromError } from "./providers";
import { useSession } from "./session";
import { t } from "./strings";
import { type FieldErrors, fromApiError, hasErrors, validateLogin } from "./validation";

/** E-mail + password sign-in, provider buttons, links to register and password reset (HS1). */
export function LoginScreen() {
  const api = useApi();
  const session = useSession();
  const router = useRouter();
  const params = useLocalSearchParams<{ error?: string; reset?: string }>();
  const provider = providerFromError(params.error);

  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [formError, setFormError] = useState<string | null>(
    provider === null ? null : t.providers.failed[provider],
  );
  const login = useMutation({ mutationFn: (body: LoginRequest) => api.auth.login(body) });

  const submit = async () => {
    const found = validateLogin({ email, password });
    setErrors(found);
    setFormError(null);
    if (hasErrors(found)) return;
    try {
      const response = await login.mutateAsync({
        email: email.trim(),
        password,
        client: session.client,
        device: platformLabel(),
      });
      await session.signedIn(response);
    } catch (error) {
      const mapped = fromApiError(error, "login");
      setErrors(mapped.fields);
      setFormError(mapped.form);
    }
  };

  return (
    <AuthScaffold title={t.login.title}>
      <FormMessage message={params.reset === "done" ? t.reset.done : null} tone="success" />
      <Input
        label={t.email}
        value={email}
        onChangeText={setEmail}
        error={errors.email}
        autoCapitalize="none"
        autoComplete="email"
        keyboardType="email-address"
        textContentType="username"
        inputMode="email"
      />
      <Input
        label={t.password}
        value={password}
        onChangeText={setPassword}
        error={errors.password}
        secureTextEntry
        autoComplete="current-password"
        textContentType="password"
        onSubmitEditing={() => void submit()}
      />
      <FormMessage message={formError} />
      <Button title={t.login.submit} loading={login.isPending} onPress={() => void submit()} />
      <Button
        title={t.login.forgot}
        variant="ghost"
        onPress={() => router.push("/forgot-password")}
      />
      <ProviderButtons
        onSignedIn={session.signedIn}
        onError={setFormError}
        disabled={login.isPending}
      />
      <View className="items-center">
        <Button
          title={`${t.login.noAccount} ${t.login.toRegister}`}
          variant="ghost"
          onPress={() => router.push("/register")}
        />
      </View>
    </AuthScaffold>
  );
}
