import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type {
  NotificationChannel,
  NotificationKind,
  NotificationPreference,
} from "@secondbrain/api-types";
import { Card, EmptyState, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { Screen } from "@/features/shell";

import { NOTIFICATIONS_KEY } from "./useUnreadCount";
import { WebPushCard } from "./WebPushCard";

const CHANNELS: readonly NotificationChannel[] = ["in_app", "push", "email"];

/** The channels of a type after turning `channel` on or off. */
export function toggled(
  channels: readonly NotificationChannel[],
  channel: NotificationChannel,
): NotificationChannel[] {
  return channels.includes(channel)
    ? channels.filter((c) => c !== channel)
    : CHANNELS.filter((c) => c === channel || channels.includes(c));
}

/** Channel preferences per notification type (PRD BL3). */
export function PreferencesScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const key = [...NOTIFICATIONS_KEY, "preferences"] as const;
  const prefs = useQuery({ queryKey: key, queryFn: () => api.notifications.preferences() });
  const save = useIdempotentMutation<
    NotificationPreference,
    { kind: NotificationKind; channels: NotificationChannel[] }
  >(
    ({ kind, channels }, idempotencyKey) =>
      api.notifications.setPreference(kind, { channels }, { idempotencyKey }),
    {
      onSuccess: (saved) => {
        client.setQueryData<NotificationPreference[]>(key, (current) =>
          current?.map((p) => (p.kind === saved.kind ? saved : p)),
        );
      },
    },
  );
  return (
    <Screen title={t("notifications.preferencesTitle")} width="form">
      <Text tone="muted">{t("notifications.preferencesHint")}</Text>
      <WebPushCard />
      {prefs.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : prefs.isError ? (
        <EmptyState
          title={t("notifications.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void prefs.refetch();
            },
          }}
        />
      ) : (
        <Card className="gap-1">
          {prefs.data.map((pref) => (
            <View key={pref.kind} className="gap-2 border-t border-border py-3">
              <Text>{t(`notifications.kinds.${pref.kind}`)}</Text>
              <View className="flex-row flex-wrap gap-2">
                {CHANNELS.map((channel) => {
                  const on = pref.channels.includes(channel);
                  const label = t(`notifications.channels.${channel}`);
                  return (
                    <Pressable
                      key={channel}
                      accessibilityRole="checkbox"
                      accessibilityState={{ checked: on, disabled: save.isPending }}
                      accessibilityLabel={t("notifications.channelOf", {
                        channel: label,
                        kind: t(`notifications.kinds.${pref.kind}`),
                      })}
                      disabled={save.isPending}
                      onPress={() => {
                        save.mutate({ kind: pref.kind, channels: toggled(pref.channels, channel) });
                      }}
                      className={`rounded-full border px-3 py-1.5 ${
                        on ? "border-primary bg-primary/10" : "border-border"
                      }`}
                    >
                      <Text tone={on ? "primary" : "muted"}>{label}</Text>
                    </Pressable>
                  );
                })}
              </View>
            </View>
          ))}
        </Card>
      )}
      {save.isError ? <Text tone="danger">{t("notifications.saveFailed")}</Text> : null}
    </Screen>
  );
}
