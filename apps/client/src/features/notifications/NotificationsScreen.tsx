import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { WeeklySummary, Notification } from "@secondbrain/api-types";
import { Button, Card, EmptyState, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import type { TFunction } from "i18next";
import { useState } from "react";
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
        : payload.type === "reaction"
          ? t(
              payload.activity_kind === "recorded"
                ? "notifications.reactionEntry"
                : "notifications.reaction",
              {
                from: payload.from_name,
                emoji: payload.emoji,
                title: payload.item_title,
              },
            )
          : payload.type === "assignment"
            ? payload.subtask_title
              ? t("notifications.assignmentSubtask", {
                  from: payload.from_name,
                  title: payload.item_title,
                  subtask: payload.subtask_title,
                })
              : t("notifications.assignment", {
                  from: payload.from_name,
                  title: payload.item_title,
                })
            : payload.type === "sharing_change"
              ? payload.change === "removed"
                ? t("notifications.sharingRemoved", {
                    from: payload.from_name,
                    title: payload.item_title,
                  })
                : t("notifications.sharingOwner", { title: payload.item_title })
              : payload.type === "forgotten_reminder"
                ? t("notifications.forgotten", {
                    count: payload.count,
                    titles: payload.titles.join(", "),
                  })
                : payload.type === "team_invitation"
                  ? t("notifications.teamInvitation", {
                      from: payload.from_name,
                      team: payload.team_name,
                      role: t(`notifications.teamRoles.${payload.role}`),
                    })
                  : payload.type === "weekly_summary"
                    ? t("notifications.weeklySummary", {
                        from: shortDate(payload.week_start, i18n.language),
                        to: shortDate(payload.week_end, i18n.language),
                      })
                    : t("notifications.routineReminder", {
                        title: payload.title,
                        time: payload.time,
                      });
  const when = new Date(notification.created_at).toLocaleString(i18n.language, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  const press = () => {
    onRead();
    if (payload.type === "routine_reminder") {
      const href = itemPath("routine", payload.routine_id) as Href;
      router.push(href);
    } else if (payload.type === "reaction" || payload.type === "assignment") {
      const href = itemPath(payload.item_kind, payload.item_id) as Href;
      router.push(href);
    } else if (payload.type === "forgotten_reminder") {
      router.push("/");
    } else if (payload.type === "sharing_change" && payload.item_id !== null) {
      const href = `/items/${payload.item_id}` as Href;
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
      {payload.type === "weekly_summary" ? <WeeklySummaryLines summary={payload} /> : null}
      {payload.type === "team_invitation" ? (
        <TeamInvitationActions teamId={payload.team_id} onRead={onRead} onChanged={onChanged} />
      ) : null}
    </Card>
  );
}

/** Join or decline a team from its invitation (TK2). */
function TeamInvitationActions({
  teamId,
  onRead,
  onChanged,
}: {
  teamId: string;
  onRead: () => void;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const [answered, setAnswered] = useState<"join" | "decline" | null>(null);
  const answer = useIdempotentMutation<unknown, "join" | "decline">(
    (choice, idempotencyKey) =>
      choice === "join"
        ? api.teams.join(teamId, { idempotencyKey })
        : api.teams.decline(teamId, { idempotencyKey }),
    {
      onSuccess: async (_, choice) => {
        setAnswered(choice);
        onRead();
        await onChanged();
      },
    },
  );
  if (answered !== null) {
    return (
      <Text tone="muted">
        {answered === "join" ? t("notifications.teamJoined") : t("notifications.teamDeclined")}
      </Text>
    );
  }
  return (
    <>
      <View className="flex-row gap-2">
        <Button
          title={t("notifications.accept")}
          size="sm"
          disabled={answer.isPending}
          onPress={() => {
            answer.mutate("join");
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
      {answer.isError ? <Text tone="danger">{t("notifications.answerFailed")}</Text> : null}
    </>
  );
}

/** A local `YYYY-MM-DD` date as "16 Kas". */
export function shortDate(date: string, locale: string): string {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1).toLocaleDateString(locale, {
    day: "numeric",
    month: "short",
  });
}

/** Minutes as "1 sa 35 dk" / "1 h 35 min". */
export function durationText(minutes: number, t: TFunction): string {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return t("notifications.summary.minutes", { count: rest });
  if (rest === 0) return t("notifications.summary.hours", { count: hours });
  return `${t("notifications.summary.hours", { count: hours })} ${t("notifications.summary.minutes", { count: rest })}`;
}

/** The weekly summary's parts (OZ1); empty parts are left out. */
function WeeklySummaryLines({ summary }: { summary: WeeklySummary }) {
  const { t } = useTranslation();
  return (
    <View className="gap-2">
      {summary.routines.length > 0 ? (
        <View className="gap-0.5">
          <Text variant="label">{t("notifications.summary.routines")}</Text>
          {summary.routines.map((r) => (
            <Text key={r.routine_id} tone="muted">
              {t("notifications.summary.routine", {
                title: r.title,
                done: r.done_days,
                due: r.scheduled_days,
              })}
              {r.streak > 0 ? ` · ${t("notifications.summary.streak", { count: r.streak })}` : ""}
            </Text>
          ))}
        </View>
      ) : null}
      {summary.goals.length > 0 ? (
        <View className="gap-0.5">
          <Text variant="label">{t("notifications.summary.goals")}</Text>
          {summary.goals.map((g) => (
            <Text key={g.goal_id} tone="muted">
              {t("notifications.summary.goal", {
                title: g.title,
                actual: g.actual_percent,
                expected: g.expected_percent,
              })}
            </Text>
          ))}
        </View>
      ) : null}
      {summary.sessions.length > 0 ? (
        <View className="gap-0.5">
          <Text variant="label">
            {t("notifications.summary.sessions", {
              total: durationText(summary.work_minutes, t),
            })}
          </Text>
          {summary.sessions.map((s) => (
            <Text key={s.item_id ?? s.title} tone="muted">
              {`${s.title}: ${durationText(s.minutes, t)}`}
            </Text>
          ))}
        </View>
      ) : null}
      {summary.forgotten.length > 0 ? (
        <View className="gap-0.5">
          <Text variant="label">{t("notifications.summary.forgotten")}</Text>
          {summary.forgotten.map((title) => (
            <Text key={title} tone="muted">
              {title}
            </Text>
          ))}
        </View>
      ) : null}
    </View>
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
