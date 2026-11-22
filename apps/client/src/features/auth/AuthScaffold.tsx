import { Card, Text } from "@secondbrain/ui";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { KeyboardAvoidingView, Platform, ScrollView, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import { APP_NAME } from "@/lib/app-info";

/** Centered card layout shared by the auth screens; scrolls above the keyboard on phones. */
export function AuthScaffold({ title, children }: { title: string; children: ReactNode }) {
  const { t } = useTranslation();
  return (
    <SafeAreaView className="flex-1 bg-background">
      <KeyboardAvoidingView
        className="flex-1"
        behavior={Platform.OS === "ios" ? "padding" : undefined}
      >
        <ScrollView
          contentContainerClassName="flex-grow items-center justify-center px-4 py-8"
          keyboardShouldPersistTaps="handled"
        >
          <View className="w-full max-w-sm gap-6">
            <View className="items-center gap-1">
              <Text
                variant="title"
                accessibilityRole="header"
                className="text-center text-3xl text-primary"
              >
                {APP_NAME}
              </Text>
              <Text tone="muted" className="text-center">
                {t("common.tagline")}
              </Text>
            </View>
            <Card className="gap-4">
              <Text variant="title">{title}</Text>
              {children}
            </Card>
          </View>
        </ScrollView>
      </KeyboardAvoidingView>
    </SafeAreaView>
  );
}

/** Form-level message (server error or notice) announced to screen readers. */
export function FormMessage({
  message,
  tone = "danger",
}: {
  message: string | null;
  tone?: "danger" | "success" | "muted";
}) {
  if (message === null) return null;
  return (
    <Text
      tone={tone}
      accessibilityRole={tone === "danger" ? "alert" : undefined}
      accessibilityLiveRegion="polite"
    >
      {message}
    </Text>
  );
}
