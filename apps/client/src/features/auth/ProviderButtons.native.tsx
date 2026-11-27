import { useApi } from "@secondbrain/api-client/react";
import { Button, useScheme } from "@secondbrain/ui";
import * as AppleAuthentication from "expo-apple-authentication";
import * as AuthSession from "expo-auth-session";
import Constants from "expo-constants";
import * as WebBrowser from "expo-web-browser";
import { useEffect, useState } from "react";
import { Platform, View } from "react-native";
import { useTranslation } from "react-i18next";

import { platformLabel } from "@/lib/app-info";

import { consentTicketOf } from "./legal";
import { OrDivider, type ProviderButtonsProps } from "./provider-shared";
import { googleRedirectUri, providerConfig } from "./providers";
import { fromApiError } from "./validation";

WebBrowser.maybeCompleteAuthSession();

const GOOGLE_DISCOVERY: AuthSession.DiscoveryDocument = {
  authorizationEndpoint: "https://accounts.google.com/o/oauth2/v2/auth",
  tokenEndpoint: "https://oauth2.googleapis.com/token",
};

/**
 * Native apps run the provider UI themselves and hand the result to the server, which returns a
 * bearer session (`/auth/google/native`, `/auth/apple/native`). Only iOS is wired up for now.
 */
export function ProviderButtons({
  onSignedIn,
  onError,
  onConsent,
  disabled = false,
  config = providerConfig,
}: ProviderButtonsProps) {
  const [appleAvailable, setAppleAvailable] = useState(false);
  useEffect(() => {
    if (Platform.OS !== "ios" || !config.apple) return;
    let active = true;
    AppleAuthentication.isAvailableAsync()
      .then((available) => {
        if (active) setAppleAvailable(available);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [config.apple]);

  const googleClientId = Platform.OS === "ios" ? config.googleIosClientId : null;
  if (googleClientId === null && !appleAvailable) return null;
  const props = { onSignedIn, onError, onConsent, disabled };
  return (
    <View className="gap-3">
      <OrDivider />
      {googleClientId !== null ? <GoogleButton clientId={googleClientId} {...props} /> : null}
      {appleAvailable ? <AppleButton {...props} /> : null}
    </View>
  );
}

type ButtonProps = Pick<ProviderButtonsProps, "onSignedIn" | "onError" | "onConsent" | "disabled">;

function GoogleButton({
  clientId,
  onSignedIn,
  onError,
  onConsent,
  disabled,
}: ButtonProps & { clientId: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const [busy, setBusy] = useState(false);
  const redirectUri = googleRedirectUri(clientId);
  const [request, , promptAsync] = AuthSession.useAuthRequest(
    {
      clientId,
      redirectUri,
      scopes: ["openid", "email", "profile"],
      responseType: AuthSession.ResponseType.Code,
      usePKCE: true,
    },
    GOOGLE_DISCOVERY,
  );

  const signIn = async () => {
    if (request?.codeVerifier === undefined) return;
    setBusy(true);
    try {
      const result = await promptAsync();
      if (result.type !== "success") {
        if (result.type === "error") onError(t("auth.providers.failed.google"));
        return;
      }
      const code = result.params.code;
      if (code === undefined) return onError(t("auth.providers.failed.google"));
      await onSignedIn(
        await api.auth.googleNative({
          code,
          code_verifier: request.codeVerifier,
          redirect_uri: redirectUri,
          client_id: clientId,
          device: platformLabel(),
        }),
      );
    } catch (error) {
      const ticket = consentTicketOf(error);
      if (ticket !== null) return onConsent?.(ticket);
      onError(fromApiError(error, "other").form ?? t("auth.providers.failed.google"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Button
      title={t("auth.providers.google")}
      variant="secondary"
      loading={busy}
      disabled={disabled === true || request === null}
      onPress={() => void signIn()}
    />
  );
}

/** Apple's own button, as its Human Interface Guidelines require on iOS. */
function AppleButton({ onSignedIn, onError, onConsent, disabled }: ButtonProps) {
  const { t } = useTranslation();
  const api = useApi();
  const scheme = useScheme();
  const [busy, setBusy] = useState(false);

  const signIn = async () => {
    const bundleId = Constants.expoConfig?.ios?.bundleIdentifier;
    if (busy || disabled === true || bundleId === undefined) return;
    setBusy(true);
    try {
      const credential = await AppleAuthentication.signInAsync({
        requestedScopes: [
          AppleAuthentication.AppleAuthenticationScope.FULL_NAME,
          AppleAuthentication.AppleAuthenticationScope.EMAIL,
        ],
      });
      if (credential.authorizationCode === null) return onError(t("auth.providers.failed.apple"));
      await onSignedIn(
        await api.auth.appleNative({
          code: credential.authorizationCode,
          client_id: bundleId,
          given_name: credential.fullName?.givenName ?? undefined,
          family_name: credential.fullName?.familyName ?? undefined,
          device: platformLabel(),
        }),
      );
    } catch (error) {
      if ((error as { code?: string }).code === "ERR_REQUEST_CANCELED") return;
      const ticket = consentTicketOf(error);
      if (ticket !== null) return onConsent?.(ticket);
      onError(fromApiError(error, "other").form ?? t("auth.providers.failed.apple"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <AppleAuthentication.AppleAuthenticationButton
      buttonType={AppleAuthentication.AppleAuthenticationButtonType.CONTINUE}
      buttonStyle={
        scheme === "dark"
          ? AppleAuthentication.AppleAuthenticationButtonStyle.WHITE
          : AppleAuthentication.AppleAuthenticationButtonStyle.BLACK
      }
      cornerRadius={6}
      style={{ height: 40, opacity: disabled === true || busy ? 0.5 : 1 }}
      onPress={() => void signIn()}
    />
  );
}
