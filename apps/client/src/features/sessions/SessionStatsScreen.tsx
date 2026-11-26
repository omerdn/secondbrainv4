import { useApi } from "@secondbrain/api-client/react";
import type { SessionDayStats, SessionSummary } from "@secondbrain/api-types";
import { Button, Card, EmptyState, Text } from "@secondbrain/ui";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { Screen } from "@/features/shell";
import { deviceTimeZone, todayIn } from "@/lib/dates";

import { useItemTitles } from "./itemTitles";
import { barShare, hoursAndMinutes, type Range, RANGES, rangeOf } from "./model";

/** "1 sa 20 dk", "45 dk": a total as the statistics show it. */
function useDuration() {
  const { t } = useTranslation();
  return (ms: number) => {
    if (ms > 0 && ms < 60_000) return t("sessions.stats.underMinute");
    const { hours, minutes } = hoursAndMinutes(ms);
    return hours > 0
      ? t("sessions.stats.hoursMinutes", { hours, minutes })
      : t("sessions.stats.minutes", { minutes });
  };
}

/** Work session statistics and history (PRD W5): time is only counted, never completed. */
export function SessionStatsScreen() {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const duration = useDuration();
  const [range, setRange] = useState<Range>(7);
  const { from, to } = rangeOf(todayIn(deviceTimeZone()), range);
  const stats = useQuery({
    queryKey: ["sessions", "stats", from, to],
    queryFn: () => api.sessions.stats({ from, to }),
  });
  const history = useInfiniteQuery({
    queryKey: ["sessions", "history"],
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => api.sessions.history(pageParam),
    // A full page may have more behind it; the next page is what came before its oldest.
    getNextPageParam: (page: SessionSummary[]) =>
      page.length >= HISTORY_PAGE ? page.at(-1)?.created_at : undefined,
  });
  const itemIds = stats.isSuccess
    ? stats.data.items.flatMap((item) => (item.item_id === null ? [] : [item.item_id]))
    : [];
  const items = useItemTitles(itemIds);
  const sessions = history.data?.pages.flat() ?? [];

  return (
    <Screen title={t("sessions.stats.title")}>
      <View className="flex-row gap-2">
        {RANGES.map((days) => (
          <Pressable
            key={days}
            accessibilityRole="radio"
            accessibilityState={{ selected: range === days }}
            onPress={() => {
              setRange(days);
            }}
            className={`rounded-full border px-3 py-1.5 ${
              range === days ? "border-primary bg-primary/10" : "border-border"
            }`}
          >
            <Text>{t("sessions.stats.lastDays", { count: days })}</Text>
          </Pressable>
        ))}
      </View>

      {stats.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : stats.isError ? (
        <EmptyState
          title={t("sessions.stats.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void stats.refetch();
            },
          }}
        />
      ) : (
        <>
          <View className="flex-row flex-wrap gap-3">
            <Card className="min-w-36 flex-1 gap-1">
              <Text variant="caption" tone="muted">
                {t("sessions.stats.work")}
              </Text>
              <Text variant="heading" accessibilityLabel={t("sessions.stats.workTotal")}>
                {duration(stats.data.work_ms)}
              </Text>
            </Card>
            <Card className="min-w-36 flex-1 gap-1">
              <Text variant="caption" tone="muted">
                {t("sessions.stats.breaks")}
              </Text>
              <Text variant="heading" accessibilityLabel={t("sessions.stats.breakTotal")}>
                {duration(stats.data.break_ms)}
              </Text>
            </Card>
          </View>

          <Card className="gap-2">
            <Text variant="heading">{t("sessions.stats.daily")}</Text>
            <Days days={stats.data.days} locale={i18n.language} duration={duration} />
          </Card>

          <Card className="gap-2">
            <Text variant="heading">{t("sessions.stats.byItem")}</Text>
            {stats.data.items.length === 0 ? (
              <Text tone="muted">{t("sessions.stats.noWork")}</Text>
            ) : (
              stats.data.items.map((entry) => {
                const title =
                  entry.item_id === null
                    ? t("sessions.stats.noItem")
                    : (items.get(entry.item_id)?.title ?? t("sessions.hiddenItem"));
                return (
                  <View
                    key={entry.item_id ?? "none"}
                    accessibilityLabel={`${title}: ${duration(entry.work_ms)}`}
                    className="flex-row justify-between gap-3"
                  >
                    <Text className="flex-1" numberOfLines={1}>
                      {title}
                    </Text>
                    <Text tone="muted">{duration(entry.work_ms)}</Text>
                  </View>
                );
              })
            )}
          </Card>
        </>
      )}

      <Card className="gap-2">
        <Text variant="heading">{t("sessions.stats.history")}</Text>
        {history.isPending ? (
          <Text tone="muted">{t("common.loading")}</Text>
        ) : history.isError ? (
          <Button
            title={t("items.detail.retry")}
            variant="ghost"
            size="sm"
            onPress={() => {
              void history.refetch();
            }}
          />
        ) : sessions.length === 0 ? (
          <Text tone="muted">{t("sessions.stats.noSessions")}</Text>
        ) : (
          <>
            {sessions.map((session) => (
              <View key={session.id} className="gap-0.5 border-t border-border pt-2">
                <Text>
                  {formatMoment(session.created_at, i18n.language)}
                  {session.open ? ` · ${t("sessions.stats.open")}` : ""}
                </Text>
                <Text variant="caption" tone="muted">
                  {t("sessions.stats.sessionLine", {
                    work: duration(session.work_ms),
                    breaks: duration(session.break_ms),
                    count: session.segments,
                  })}
                </Text>
              </View>
            ))}
            {history.hasNextPage ? (
              <View className="self-start">
                <Button
                  title={t("sessions.stats.more")}
                  variant="ghost"
                  size="sm"
                  disabled={history.isFetchingNextPage}
                  onPress={() => {
                    void history.fetchNextPage();
                  }}
                />
              </View>
            ) : null}
          </>
        )}
      </Card>
    </Screen>
  );
}

/** The server's history page size. */
const HISTORY_PAGE = 50;

function Days({
  days,
  locale,
  duration,
}: {
  days: readonly SessionDayStats[];
  locale: string;
  duration: (ms: number) => string;
}) {
  const busiest = Math.max(0, ...days.map((day) => day.work_ms));
  // Newest first: today on top.
  return (
    <View className="gap-2">
      {[...days].reverse().map((day) => (
        <View
          key={day.date}
          accessibilityLabel={`${formatDay(day.date, locale)}: ${duration(day.work_ms)}`}
          className="flex-row items-center gap-3"
        >
          <Text className="w-28" variant="caption" tone="muted">
            {formatDay(day.date, locale)}
          </Text>
          <View className="h-2 flex-1 overflow-hidden rounded-full bg-border">
            <View
              className="h-2 rounded-full bg-primary"
              style={{ width: `${Math.round(barShare(day.work_ms, busiest) * 100)}%` }}
            />
          </View>
          <Text className="w-20 text-right" variant="caption">
            {duration(day.work_ms)}
          </Text>
        </View>
      ))}
    </View>
  );
}

function formatDay(day: string, locale: string): string {
  return new Date(`${day}T00:00:00Z`).toLocaleDateString(locale, {
    timeZone: "UTC",
    weekday: "short",
    day: "numeric",
    month: "short",
  });
}

function formatMoment(iso: string, locale: string): string {
  return new Date(iso).toLocaleString(locale, { dateStyle: "medium", timeStyle: "short" });
}
