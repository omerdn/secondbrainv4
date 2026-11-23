import type { AccessExplanation, AccessReason } from "@secondbrain/api-types";
import { Button, Text } from "@secondbrain/ui";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import type { Loadable } from "./ItemDetail";

export interface WhyPanelProps {
  explanation: Loadable<AccessExplanation>;
  /** Titles of linked items the viewer can see, by id. */
  titles: ReadonlyMap<string, string>;
  onOpenItem: (id: string) => void;
}

/** The item a reason came through, if it has one. The server only names items the viewer sees. */
function sourceOf(reason: AccessReason): string | null {
  if (reason.kind === "linked") return reason.item_id;
  if (reason.kind === "notebook") return reason.notebook_id;
  return null;
}

function Reason({
  reason,
  titles,
  onOpenItem,
}: {
  reason: AccessReason;
  titles: ReadonlyMap<string, string>;
  onOpenItem: (id: string) => void;
}) {
  const { t } = useTranslation();
  const source = sourceOf(reason);
  const title = source !== null ? titles.get(source) : undefined;
  const text =
    reason.kind === "team_role"
      ? t("items.why.reasons.team_role", { role: t(`items.why.teamRoles.${reason.role}`) })
      : reason.kind === "linked" && title !== undefined
        ? t("items.why.reasons.linkedTo", { title })
        : t(`items.why.reasons.${reason.kind}`);
  return (
    <View className="flex-row flex-wrap items-center justify-between gap-2">
      <Text className="shrink">{text}</Text>
      {source !== null ? (
        <Button
          title={t(reason.kind === "notebook" ? "items.why.openNotebook" : "items.why.openLinked")}
          variant="ghost"
          size="sm"
          onPress={() => {
            onOpenItem(source);
          }}
        />
      ) : null}
    </View>
  );
}

/** "Why am I seeing this?": every path that gives the viewer access, and what it allows (V11). */
export function WhyPanel({ explanation, titles, onOpenItem }: WhyPanelProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <View className="gap-2">
      <View className="self-start">
        <Button
          title={t("items.why.title")}
          variant="ghost"
          size="sm"
          accessibilityState={{ expanded: open }}
          onPress={() => {
            setOpen(!open);
          }}
        />
      </View>
      {open ? (
        <View className="gap-3 rounded-lg border border-border bg-surface-raised p-4">
          {explanation.status === "loading" ? (
            <Text tone="muted">{t("common.loading")}</Text>
          ) : explanation.status === "error" ? (
            <View className="flex-row items-center gap-2">
              <Text tone="danger">{t("items.detail.loadFailed")}</Text>
              <Button
                title={t("items.detail.retry")}
                variant="ghost"
                size="sm"
                onPress={explanation.retry}
              />
            </View>
          ) : (
            <>
              <View className="gap-2">
                {explanation.value.reasons.map((reason, index) => (
                  <Reason
                    key={`${reason.kind}-${sourceOf(reason) ?? index}`}
                    reason={reason}
                    titles={titles}
                    onOpenItem={onOpenItem}
                  />
                ))}
              </View>
              <View className="gap-1">
                <Text variant="caption" tone="muted">
                  {t("items.why.canDo")}
                </Text>
                <Text variant="label">
                  {explanation.value.permissions
                    .map((permission) => t(`items.why.permissions.${permission}`))
                    .join(", ")}
                </Text>
              </View>
            </>
          )}
        </View>
      ) : null}
    </View>
  );
}
