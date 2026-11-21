import { Button, Text } from "@secondbrain/ui";
import { Redirect } from "expo-router";
import type { ReactNode } from "react";
import { ActivityIndicator, View } from "react-native";
import { useTranslation } from "react-i18next";

import { useSession } from "./session";

function Pending({ children }: { children?: ReactNode }) {
  return (
    <View className="flex-1 items-center justify-center gap-4 bg-background px-4">{children}</View>
  );
}

/**
 * Renders `children` only in the wanted session state and redirects otherwise: signed-in
 * screens send visitors to `/login`, auth screens send signed-in users to `/`. While the session
 * is being restored nothing else is shown; if the server cannot be reached the app stays blocked
 * with a retry (online-only, ADR-0003).
 */
export function SessionGate({
  require,
  children,
}: {
  require: "signedIn" | "signedOut";
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const { state, retry } = useSession();
  switch (state.status) {
    case "loading":
      return (
        <Pending>
          <ActivityIndicator accessibilityLabel={t("common.loading")} />
        </Pending>
      );
    case "unavailable":
      return (
        <Pending>
          <Text tone="muted">{t("auth.session.unavailable")}</Text>
          <Button title={t("auth.session.retry")} variant="secondary" onPress={retry} />
        </Pending>
      );
    case "signedIn":
      return require === "signedIn" ? children : <Redirect href="/" />;
    case "signedOut":
      return require === "signedOut" ? children : <Redirect href="/login" />;
  }
}
