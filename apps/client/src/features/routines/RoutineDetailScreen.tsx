import { ApiError } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import { Button, Card, EmptyState, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { todayIn } from "@/lib/dates";

import { CheckInCard } from "./CheckInCard";
import { describePlan, formatDay } from "./describe";
import { nextOccurrences, PREVIEW_COUNT } from "./recurrence";
import { SharedRoutineCard } from "./SharedRoutineCard";

/** A routine's plan, daily target and reminders (PRD R1-R3, R13). */
export function RoutineDetailScreen({ id }: { id: string }) {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const routine = useQuery({
    queryKey: ["routine", id],
    queryFn: () => api.routines.get(id),
  });

  if (routine.isPending) {
    return <Screen title={t("common.loading")}>{null}</Screen>;
  }
  if (routine.isError) {
    const notFound = routine.error instanceof ApiError && routine.error.status === 404;
    return (
      <Screen title={t("nav.routines")}>
        {notFound ? (
          <EmptyState
            title={t("items.detail.notFound")}
            description={t("items.detail.notFoundHint")}
          />
        ) : (
          <EmptyState
            title={t("routines.loadFailed")}
            action={{
              title: t("items.detail.retry"),
              onPress: () => {
                void routine.refetch();
              },
            }}
          />
        )}
      </Screen>
    );
  }

  const { item, timezone, daily_target, start_date, end_date, reminder_times, versions } =
    routine.data;
  const today = todayIn(timezone);
  const current =
    versions.find((v) => v.valid_from <= today && (v.valid_to === null || today <= v.valid_to)) ??
    versions.at(-1);
  const rrule = current?.rrule ?? "";
  const upcoming =
    current === undefined
      ? []
      : nextOccurrences(
          rrule,
          current.valid_from,
          today,
          PREVIEW_COUNT,
          current.valid_to ?? undefined,
        );
  return (
    <Screen title={item.title}>
      <View className="gap-4">
        <CheckInCard routine={routine.data} />
        <SharedRoutineCard routine={routine.data} />
        <Card className="gap-2">
          <Text variant="label">{t("routines.plan")}</Text>
          <Text>{describePlan(rrule, t, i18n.language)}</Text>
          <Text tone="muted">{t("routines.targetPerDay", { count: daily_target })}</Text>
          <Text tone="muted">
            {end_date === null
              ? t("routines.dates", { start: formatDay(start_date, i18n.language) })
              : t("routines.datesWithEnd", {
                  start: formatDay(start_date, i18n.language),
                  end: formatDay(end_date, i18n.language),
                })}
          </Text>
          <Text tone="muted">{t("routines.timezone", { zone: timezone })}</Text>
        </Card>
        <Card className="gap-2">
          <Text variant="label">{t("routines.preview")}</Text>
          {upcoming.length === 0 ? (
            <Text tone="muted">{t("routines.noPreview")}</Text>
          ) : (
            upcoming.map((day) => (
              <Text key={day} tone="muted">
                {formatDay(day, i18n.language)}
              </Text>
            ))
          )}
        </Card>
        <Card className="gap-2">
          <Text variant="label">{t("routines.reminders")}</Text>
          <Text tone="muted">
            {reminder_times.length === 0 ? t("routines.noReminders") : reminder_times.join(", ")}
          </Text>
        </Card>
        <View className="self-start">
          <Button
            title={t("trackers.itemDetails")}
            variant="ghost"
            size="sm"
            onPress={() => {
              const href = `/items/${encodeURIComponent(id)}` as Href;
              router.push(href);
            }}
          />
        </View>
      </View>
    </Screen>
  );
}
