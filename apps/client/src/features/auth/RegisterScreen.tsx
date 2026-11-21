import { useApi } from "@secondbrain/api-client/react";
import type { RegisterRequest } from "@secondbrain/api-types";
import { Button, Input } from "@secondbrain/ui";
import { useMutation } from "@tanstack/react-query";
import { useRouter } from "expo-router";
import { useState } from "react";
import { View } from "react-native";

import { platformLabel } from "@/lib/app-info";

import { AuthScaffold, FormMessage } from "./AuthScaffold";
import { ProviderButtons } from "./ProviderButtons";
import { useSession } from "./session";
import { t } from "./strings";
import { type FieldErrors, fromApiError, hasErrors, validateRegister } from "./validation";

/** E-mail + password registration (HS1); the account is signed in right away. */
export function RegisterScreen() {
  const api = useApi();
  const session = useSession();
  const router = useRouter();

  const [displayName, setDisplayName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const register = useMutation({
    mutationFn: (body: RegisterRequest) => api.auth.register(body),
  });

  const submit = async () => {
    const found = validateRegister({ email, password, displayName });
    setErrors(found);
    setFormError(null);
    if (hasErrors(found)) return;
    try {
      const response = await register.mutateAsync({
        display_name: displayName.trim(),
        email: email.trim(),
        password,
        client: session.client,
        device: platformLabel(),
      });
      await session.signedIn(response);
    } catch (error) {
      const mapped = fromApiError(error, "register");
      setErrors(mapped.fields);
      setFormError(mapped.form);
    }
  };

  return (
    <AuthScaffold title={t.register.title}>
      <Input
        label={t.displayName}
        value={displayName}
        onChangeText={setDisplayName}
        error={errors.display_name}
        autoComplete="name"
        textContentType="name"
      />
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
        hint={t.passwordHint}
        secureTextEntry
        autoComplete="new-password"
        textContentType="newPassword"
        onSubmitEditing={() => void submit()}
      />
      <FormMessage message={formError} />
      <Button
        title={t.register.submit}
        loading={register.isPending}
        onPress={() => void submit()}
      />
      <ProviderButtons
        onSignedIn={session.signedIn}
        onError={setFormError}
        disabled={register.isPending}
      />
      <View className="items-center">
        <Button
          title={`${t.register.haveAccount} ${t.register.toLogin}`}
          variant="ghost"
          onPress={() => router.replace("/login")}
        />
      </View>
    </AuthScaffold>
  );
}
