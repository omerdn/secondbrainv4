import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { DataExport } from "@secondbrain/api-types";
import { Button, Card, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Linking, View } from "react-native";

import { API_URL } from "@/lib/api";

const EXPORT_KEY = ["account", "export"] as const;

/** How often a pending export is checked again. */
export const EXPORT_POLL_MS = 3000;

/** The full download address of an export file path the API gave. */
export function downloadUrl(path: string, apiUrl: string = API_URL): string {
  return `${apiUrl.replace(/\/+$/u, "")}${path}`;
}

/**
 * The data export (PRD HS3, T4.3.2): prepare a JSON and Markdown copy of everything the user can
 * see, then download it through a link that works for 24 hours.
 */
export function DataExportCard() {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const latest = useQuery({
    queryKey: EXPORT_KEY,
    queryFn: () => api.account.latestExport(),
    refetchInterval: (query) => (query.state.data?.status === "pending" ? EXPORT_POLL_MS : false),
  });
  const request = useIdempotentMutation<DataExport, void>(
    (_: void, idempotencyKey) => api.account.requestExport({ idempotencyKey }),
    {
      onSuccess: (next) => {
        client.setQueryData(EXPORT_KEY, next);
      },
    },
  );
  if (latest.isPending || latest.isError) {
    return null;
  }
  const current = latest.data;
  const until = current
    ? new Date(current.expires_at).toLocaleString(i18n.language, {
        day: "numeric",
        month: "long",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "";
  return (
    <Card className="gap-3">
      <Text variant="label">{t("account.export.title")}</Text>
      <Text tone="muted">{t("account.export.hint")}</Text>
      {current?.status === "pending" ? (
        <Text>{t("account.export.pending")}</Text>
      ) : current?.status === "ready" && current.json_path && current.markdown_path ? (
        <View className="gap-2">
          <Text>{t("account.export.ready", { until })}</Text>
          <View className="flex-row flex-wrap gap-2">
            <Button
              title={t("account.export.json")}
              variant="secondary"
              onPress={() => {
                void Linking.openURL(downloadUrl(current.json_path ?? ""));
              }}
            />
            <Button
              title={t("account.export.markdown")}
              variant="secondary"
              onPress={() => {
                void Linking.openURL(downloadUrl(current.markdown_path ?? ""));
              }}
            />
          </View>
        </View>
      ) : current?.status === "failed" ? (
        <Text tone="danger">{t("account.export.failed")}</Text>
      ) : null}
      {current?.status === "pending" ? null : (
        <Button
          title={t("account.export.request")}
          variant={current?.status === "ready" ? "ghost" : "primary"}
          className="self-start"
          loading={request.isPending}
          onPress={() => {
            request.mutate();
          }}
        />
      )}
      {request.isError ? <Text tone="danger">{t("account.export.requestFailed")}</Text> : null}
    </Card>
  );
}
