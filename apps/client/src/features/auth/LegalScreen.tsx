import { useApi } from "@secondbrain/api-client/react";
import type { LegalDocumentKind } from "@secondbrain/api-types";
import { Button, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { useLocalSearchParams, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { ScrollView, View } from "react-native";

const KINDS: readonly LegalDocumentKind[] = ["privacy", "kvkk", "terms"];

/** One block of a legal text: a heading or a paragraph (the texts are simple Markdown). */
export function blocksOf(markdown: string): { heading: boolean; text: string }[] {
  return markdown
    .split(/\n\s*\n/u)
    .map((block) => block.trim())
    .filter((block) => block.length > 0)
    .map((block) =>
      block.startsWith("#")
        ? { heading: true, text: block.replace(/^#+\s*/u, "") }
        : { heading: false, text: block.replace(/\s*\n\s*/gu, " ") },
    );
}

/** A legal text in force (HS5): privacy policy, KVKK notice or terms. Open signed in or out. */
export function LegalScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const { kind } = useLocalSearchParams<{ kind?: string }>();
  const known = KINDS.find((k) => k === kind);
  const doc = useQuery({
    queryKey: ["legal", known],
    queryFn: () => api.legal.get(known ?? "privacy"),
    enabled: known !== undefined,
  });
  return (
    <ScrollView contentContainerClassName="mx-auto w-full max-w-2xl gap-4 p-6">
      {doc.data ? (
        <>
          {blocksOf(doc.data.body).map((block, index) => (
            <Text key={index} variant={block.heading ? "heading" : "body"}>
              {block.text}
            </Text>
          ))}
          <Text tone="muted">{t("auth.legal.version", { version: doc.data.version })}</Text>
        </>
      ) : (
        <Text tone="muted">
          {known === undefined || doc.isError ? t("auth.legal.missing") : t("common.loading")}
        </Text>
      )}
      <View className="items-start">
        <Button
          title={t("auth.legal.close")}
          variant="ghost"
          onPress={() => {
            if (router.canGoBack()) router.back();
            else router.replace("/");
          }}
        />
      </View>
    </ScrollView>
  );
}
