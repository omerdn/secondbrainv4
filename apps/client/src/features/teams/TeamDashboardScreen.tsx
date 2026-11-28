import { useApi } from "@secondbrain/api-client/react";
import type { Activity, Item } from "@secondbrain/api-types";
import { Card, EmptyState, TagChip, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { Screen } from "@/features/shell";
import { deviceTimeZone, todayIn } from "@/lib/dates";
import { itemPath } from "@/lib/items";

import { TEAMS_KEY } from "./teamModel";

/** Events shown on the dashboard; the full log has its own screen (T5.2.3). */
const ACTIVITY_LIMIT = 20;

function percent(ratio: number): number {
  return Number.isFinite(ratio) ? Math.round(ratio * 100) : 0;
}

function Section({
  title,
  empty,
  children,
}: {
  title: string;
  empty: string;
  children: ReactNode[];
}) {
  return (
    <Card className="gap-2">
      <Text variant="heading">{title}</Text>
      {children.length === 0 ? <Text tone="muted">{empty}</Text> : children}
    </Card>
  );
}

function Row({ item, children }: { item: Item; children: ReactNode }) {
  const router = useRouter();
  return (
    <Pressable
      accessibilityRole="link"
      accessibilityLabel={item.title}
      onPress={() => {
        router.push(itemPath(item.kind, item.id) as Href);
      }}
      className="gap-0.5 border-t border-border pt-2"
    >
      <Text numberOfLines={2}>{item.title}</Text>
      {children}
    </Pressable>
  );
}

function ActivityLine({ event, names }: { event: Activity; names: Map<string, string> }) {
  const { t, i18n } = useTranslation();
  const who =
    event.actor_id === null
      ? t("teams.dashboard.system")
      : (names.get(event.actor_id) ?? t("teams.dashboard.someone"));
  const when = new Date(event.created_at).toLocaleString(i18n.language, {
    dateStyle: "short",
    timeStyle: "short",
  });
  return (
    <View className="gap-0.5 border-t border-border pt-2">
      <Text>{t(`teams.dashboard.activity.${event.kind}`, { who, title: event.item_title })}</Text>
      <Text variant="caption" tone="muted">
        {when}
      </Text>
    </View>
  );
}

/**
 * The team dashboard (PRD TK6): the team's active goals, how its routines were kept lately, its
 * overdue tasks and its latest activity. Everything comes filtered by what the user can see
 * (V9); outside the team it is empty.
 */
export function TeamDashboardScreen({ id }: { id: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const today = todayIn(deviceTimeZone());
  const detail = useQuery({
    queryKey: [...TEAMS_KEY, id, "detail"],
    queryFn: () => api.teams.get(id),
  });
  const board = useQuery({
    queryKey: [...TEAMS_KEY, id, "dashboard", today],
    queryFn: () => api.today.teamDashboard(id, today),
  });
  const activity = useQuery({
    queryKey: [...TEAMS_KEY, id, "activity"],
    queryFn: () => api.activity.filtered({ team_ids: [id] }, { limit: ACTIVITY_LIMIT }),
  });
  if (detail.isError) {
    // Not in the team: nothing about it is shown (V9).
    return (
      <Screen title={t("teams.dashboard.title")}>
        <EmptyState title={t("teams.dashboard.notMember")} />
      </Screen>
    );
  }
  const team = detail.data?.team;
  const names = new Map(detail.data?.members.map((m) => [m.user_id, m.display_name]));
  return (
    <Screen title={team?.name ?? t("teams.dashboard.title")}>
      {team !== undefined ? (
        <View className="flex-row flex-wrap items-center gap-2">
          <TagChip label={team.name} teamColor={team.color} />
          <Text tone="muted">{t("teams.dashboard.title")}</Text>
        </View>
      ) : null}
      {board.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : board.isError ? (
        <EmptyState
          title={t("teams.dashboard.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void board.refetch();
            },
          }}
        />
      ) : (
        <>
          <Section title={t("teams.dashboard.goals")} empty={t("teams.dashboard.noGoals")}>
            {board.data.goals.map((goal) => (
              <Row key={goal.item.id} item={goal.item}>
                <Text variant="caption" tone="muted">
                  {t("teams.dashboard.goalLine", {
                    actual: percent(goal.actual),
                    pace: percent(goal.pace),
                  })}
                </Text>
              </Row>
            ))}
          </Section>
          <Section
            title={t("teams.dashboard.routines", { count: board.data.adherence_days })}
            empty={t("teams.dashboard.noRoutines")}
          >
            {board.data.routines.map((routine) => (
              <Row key={routine.item.id} item={routine.item}>
                <Text variant="caption" tone="muted">
                  {t("teams.dashboard.adherence", {
                    complete: routine.complete,
                    scheduled: routine.scheduled,
                  })}
                </Text>
              </Row>
            ))}
          </Section>
          <Section title={t("teams.dashboard.overdue")} empty={t("teams.dashboard.noOverdue")}>
            {board.data.overdue.map((task) => (
              <Row key={task.item.id} item={task.item}>
                <Text variant="caption" tone="danger">
                  {t("teams.dashboard.dueLine", {
                    date: task.due_date,
                    done: task.done,
                    total: task.total,
                  })}
                </Text>
              </Row>
            ))}
          </Section>
        </>
      )}
      <Section title={t("teams.dashboard.activityTitle")} empty={t("teams.dashboard.noActivity")}>
        {(activity.data?.items ?? []).map((event) => (
          <ActivityLine key={event.id} event={event} names={names} />
        ))}
      </Section>
    </Screen>
  );
}
