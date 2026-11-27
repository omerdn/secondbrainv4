import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Notification } from "@secondbrain/api-types";
import { Button, Card, EmptyState, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { Screen } from "@/features/shell";
import { itemPath, kindLabel } from "@/lib/items";

import { NOTIFICATIONS_KEY } from "./useUnreadCount";

/** The in-app notification center (PRD BL1): newest first, live, read on press. */
export function NotificationsScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const router = useRouter();
  const list = useQuery({
    queryKey: [...NOTIFICATIONS_KEY, "list"],
    queryFn: () => api.notifications.list(),
  });
  const refresh = () => client.invalidateQueries({ queryKey: NOTIFICATIONS_KEY });
  const read = useIdempotentMutation<Notification, string>(
    (id, idempotencyKey) => api.notifications.read(id, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const readAll = useIdempotentMutation<unknown, undefined>(
    (_, idempotencyKey) => api.notifications.readAll({ idempotencyKey }),
    { onSuccess: refresh },
  );
  const openPreferences = () => {
    const href = "/notifications/preferences" as Href;
    router.push(href);
  };

  return (
    <Screen title={t("nav.notifications")}>
      <View className="flex-row flex-wrap gap-2">
        <Button
          title={t("notifications.readAll")}
          variant="secondary"
          size="sm"
          disabled={readAll.isPending || (list.data?.unread ?? 0) === 0}
          onPress={() => {
            readAll.mutate(undefined);
          }}
        />
        <Button
          title={t("notifications.preferences")}
          variant="ghost"
          size="sm"
          onPress={openPreferences}
        />
      </View>
      {list.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : list.isError ? (
        <EmptyState
          title={t("notifications.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void list.refetch();
            },
          }}
        />
      ) : list.data.notifications.length === 0 ? (
        <EmptyState title={t("notifications.empty")} description={t("notifications.emptyHint")} />
      ) : (
        <View className="gap-2">
          {list.data.notifications.map((notification) => (
            <NotificationRow
              key={notification.id}
              notification={notification}
              onRead={() => {
                if (notification.read_at === null) read.mutate(notification.id);
              }}
              onChanged={refresh}
            />
          ))}
        </View>
      )}
    </Screen>
  );
}

type Payload = Notification["payload"];
type InvitationPayload = Extract<Payload, { type: "invitation" }>;

function NotificationRow({
  notification,
  onRead,
  onChanged,
}: {
  notification: Notification;
  onRead: () => void;
  onChanged: () => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const router = useRouter();
  const { payload } = notification;
  const unread = notification.read_at === null;
  const text =
    payload.type === "invitation"
      ? t("notifications.invitation", {
          from: payload.from_name,
          title: payload.item_title,
          kind: t(`items.kinds.${kindLabel(payload.item_kind)}`).toLocaleLowerCase(i18n.language),
        })
      : payload.type === "nudge"
        ? t("notifications.nudge", { from: payload.from_name })
        : t("notifications.routineReminder", { title: payload.title, time: payload.time });
  const when = new Date(notification.created_at).toLocaleString(i18n.language, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  const press = () => {
    onRead();
    if (payload.type === "routine_reminder") {
      const href = itemPath("routine", payload.routine_id) as Href;
      router.push(href);
    }
  };
  return (
    <Card className={unread ? "gap-2 border-primary" : "gap-2"}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={unread ? t("notifications.unreadLabel", { text }) : text}
        onPress={press}
        className="gap-1"
      >
        <View className="flex-row items-center gap-2">
          {unread ? <View testID="unread-dot" className="h-2 w-2 rounded-full bg-primary" /> : null}
          <Text className={unread ? "flex-1 font-semibold" : "flex-1"}>{text}</Text>
        </View>
        <Text variant="caption" tone="muted">
          {when}
        </Text>
      </Pressable>
      {payload.type === "invitation" ? (
        <InvitationActions payload={payload} onRead={onRead} onChanged={onChanged} />
      ) : null}
    </Card>
  );
}

/** Accept or decline an invitation from its notification (G5). */
function InvitationActions({
  payload,
  onRead,
  onChanged,
}: {
  payload: InvitationPayload;
  onRead: () => void;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const answer = useIdempotentMutation<unknown, "accept" | "decline">(
    (choice, idempotencyKey) =>
      choice === "accept"
        ? api.invitations.accept(payload.invitation_id, { idempotencyKey })
        : api.invitations.decline(payload.invitation_id, { idempotencyKey }),
    {
      onSuccess: async (_, choice) => {
        onRead();
        await onChanged();
        if (choice === "accept") {
          const href = itemPath(payload.item_kind, payload.item_id) as Href;
          router.push(href);
        }
      },
    },
  );
  return (
    <>
      <View className="flex-row gap-2">
        <Button
          title={t("notifications.accept")}
          size="sm"
          disabled={answer.isPending}
          onPress={() => {
            answer.mutate("accept");
          }}
        />
        <Button
          title={t("notifications.decline")}
          variant="ghost"
          size="sm"
          disabled={answer.isPending}
          onPress={() => {
            answer.mutate("decline");
          }}
        />
      </View>
      {answer.isError ? (
        <Text variant="caption" tone="danger">
          {t("notifications.answerFailed")}
        </Text>
      ) : null}
    </>
  );
}
