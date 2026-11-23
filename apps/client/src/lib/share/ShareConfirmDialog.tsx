import { useApi } from "@secondbrain/api-client/react";
import type { AccessPreviewRequest } from "@secondbrain/api-types";
import { Button, Sheet, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

export interface ShareConfirmDialogProps {
  /** The action about to happen; `null` keeps the dialog closed. */
  request: AccessPreviewRequest | null;
  /**
   * Names the caller already knows, by user id. The server does not name the person being added,
   * so the screen that picked them passes the name here.
   */
  knownNames?: Readonly<Record<string, string>>;
  /** Names the action on the confirm button, e.g. "Bağla". */
  confirmLabel: string;
  /** Runs the action. Only reachable once the preview has been shown. */
  onConfirm: () => void;
  onCancel: () => void;
  /** The action is running. */
  busy?: boolean;
}

/**
 * Shows who would gain access before an action that widens visibility, and asks for approval
 * (PRD V7). The action cannot be confirmed until the server has answered.
 */
export function ShareConfirmDialog({
  request,
  knownNames = {},
  confirmLabel,
  onConfirm,
  onCancel,
  busy = false,
}: ShareConfirmDialogProps) {
  const { t } = useTranslation();
  const api = useApi();
  const preview = useQuery({
    queryKey: ["access", "preview", request],
    queryFn: () => {
      if (request === null) {
        throw new Error("no action to preview");
      }
      return api.access.preview(request);
    },
    enabled: request !== null,
    // Always asks again: who can see an item may have changed since the last time.
    staleTime: 0,
    gcTime: 0,
  });
  // Refetching old data must not count as an answer for the current state.
  const ready = preview.isSuccess && !preview.isFetching;
  return (
    <Sheet
      visible={request !== null}
      onClose={onCancel}
      title={t("share.title")}
      closeLabel={t("share.cancel")}
      footer={
        <View className="flex-row justify-end">
          <Button title={confirmLabel} disabled={!ready} loading={busy} onPress={onConfirm} />
        </View>
      }
    >
      {preview.isError ? (
        <View className="gap-2">
          <Text tone="danger" accessibilityRole="alert">
            {t("share.failed")}
          </Text>
          <View className="self-start">
            <Button
              title={t("share.retry")}
              variant="secondary"
              size="sm"
              onPress={() => {
                void preview.refetch();
              }}
            />
          </View>
        </View>
      ) : !ready ? (
        <Text tone="muted">{t("share.checking")}</Text>
      ) : preview.data.people.length === 0 ? (
        <Text>{t("share.nobody")}</Text>
      ) : (
        <View className="gap-2">
          <Text>{t("share.willSee", { count: preview.data.items.length })}</Text>
          {preview.data.people.map((person) => (
            <Text key={person.id} className="font-semibold">
              {person.display_name ?? knownNames[person.id] ?? t("share.unknownPerson")}
            </Text>
          ))}
        </View>
      )}
    </Sheet>
  );
}
