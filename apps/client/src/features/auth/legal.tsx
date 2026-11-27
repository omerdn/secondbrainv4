import { ApiError } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import type { LegalAcceptance, LegalDocumentSummary } from "@secondbrain/api-types";
import { Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

export const LEGAL_KEY = ["legal"] as const;

/** The ticket of a sign-in held for legal consent (HS5), if `error` is one. */
export function consentTicketOf(error: unknown): string | null {
  return error instanceof ApiError && error.code === "consent_required"
    ? error.consentTicket
    : null;
}

/** The consent screen's address for a held sign-in. */
export function consentHref(ticket: string): Href {
  const href = `/consent?ticket=${encodeURIComponent(ticket)}` as Href;
  return href;
}

/** What accepting every listed text sends. */
export function acceptancesOf(documents: readonly LegalDocumentSummary[]): LegalAcceptance[] {
  return documents.map((doc) => ({ kind: doc.kind, version: doc.version }));
}

/** The texts in force. */
export function useLegalDocuments() {
  const api = useApi();
  return useQuery({ queryKey: LEGAL_KEY, queryFn: () => api.legal.list(), staleTime: 60_000 });
}

/**
 * The required consent checkbox with links to every text (HS5). Each link opens the text's page.
 */
export function ConsentCheckbox({
  documents,
  checked,
  onChange,
  error,
}: {
  documents: readonly LegalDocumentSummary[];
  checked: boolean;
  onChange: (checked: boolean) => void;
  error?: string | null;
}) {
  const { t } = useTranslation();
  const router = useRouter();
  return (
    <View className="gap-2">
      <View className="flex-row flex-wrap gap-x-3 gap-y-1">
        {documents.map((doc) => {
          const href = `/legal/${doc.kind}` as Href;
          return (
            <Pressable
              key={doc.kind}
              accessibilityRole="link"
              onPress={() => {
                router.push(href);
              }}
            >
              <Text tone="primary">{doc.title}</Text>
            </Pressable>
          );
        })}
      </View>
      <Pressable
        accessibilityRole="checkbox"
        accessibilityState={{ checked }}
        accessibilityLabel={t("auth.legal.accept")}
        onPress={() => {
          onChange(!checked);
        }}
        className="flex-row items-start gap-2"
      >
        <View
          className={`mt-0.5 h-5 w-5 items-center justify-center rounded border ${
            checked ? "border-primary bg-primary" : "border-border"
          }`}
        >
          {checked ? <Text className="text-xs text-primary-foreground">✓</Text> : null}
        </View>
        <Text className="shrink">{t("auth.legal.accept")}</Text>
      </Pressable>
      {error ? <Text tone="danger">{error}</Text> : null}
    </View>
  );
}
