import { useApi } from "@secondbrain/api-client/react";
import type { Calendar, Item } from "@secondbrain/api-types";
import { Button, Card, EmptyState, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { Screen } from "@/features/shell";
import { deviceTimeZone, todayIn } from "@/lib/dates";
import { itemPath } from "@/lib/items";

import {
  type CalendarView,
  type Entry,
  entriesByDay,
  goalsOn,
  rangeFor,
  shift,
  VIEWS,
  weeksOf,
} from "./model";

/** Entries a month cell lists before "+N". */
const CELL_ENTRIES = 3;

const asDate = (day: string) => new Date(`${day}T00:00:00Z`);

function formatDay(day: string, locale: string, options: Intl.DateTimeFormatOptions): string {
  return asDate(day).toLocaleDateString(locale, { timeZone: "UTC", ...options });
}

/** The calendar (PRD C1): day, week and month views of tasks, routines and goals. */
export function CalendarScreen() {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const today = todayIn(deviceTimeZone());
  const [view, setView] = useState<CalendarView>("month");
  const [anchor, setAnchor] = useState(today);
  const { from, to } = rangeFor(view, anchor);
  const calendar = useQuery({
    queryKey: ["calendar", from, to],
    queryFn: () => api.calendar.get(from, to),
  });
  const locale = i18n.language;
  const title =
    view === "month"
      ? formatDay(anchor, locale, { month: "long", year: "numeric" })
      : view === "week"
        ? `${formatDay(from, locale, { day: "numeric", month: "short" })} – ${formatDay(
            to,
            locale,
            {
              day: "numeric",
              month: "short",
              year: "numeric",
            },
          )}`
        : formatDay(anchor, locale, {
            weekday: "long",
            day: "numeric",
            month: "long",
            year: "numeric",
          });
  const openItem = (item: Item) => {
    const href = itemPath(item.kind, item.id) as Href;
    router.push(href);
  };
  const openDay = (day: string) => {
    setAnchor(day);
    setView("day");
  };

  return (
    <Screen title={t("nav.calendar")}>
      <View className="flex-row flex-wrap items-center justify-between gap-2">
        <View className="flex-row gap-2">
          {VIEWS.map((v) => (
            <Pressable
              key={v}
              accessibilityRole="radio"
              accessibilityState={{ selected: view === v }}
              accessibilityLabel={t(`calendar.views.${v}`)}
              onPress={() => {
                setView(v);
              }}
              className={`rounded-full border px-3 py-1.5 ${
                view === v ? "border-primary bg-primary/10" : "border-border"
              }`}
            >
              <Text>{t(`calendar.views.${v}`)}</Text>
            </Pressable>
          ))}
        </View>
        <View className="flex-row items-center gap-1">
          <Button
            title="‹"
            accessibilityLabel={t("calendar.previous")}
            variant="ghost"
            size="sm"
            onPress={() => {
              setAnchor(shift(view, anchor, -1));
            }}
          />
          <Button
            title={t("calendar.today")}
            variant="secondary"
            size="sm"
            onPress={() => {
              setAnchor(today);
            }}
          />
          <Button
            title="›"
            accessibilityLabel={t("calendar.next")}
            variant="ghost"
            size="sm"
            onPress={() => {
              setAnchor(shift(view, anchor, 1));
            }}
          />
        </View>
      </View>
      <Text variant="heading" accessibilityRole="header" testID="calendar-title">
        {title}
      </Text>

      {calendar.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : calendar.isError ? (
        <EmptyState
          title={t("calendar.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void calendar.refetch();
            },
          }}
        />
      ) : view === "month" ? (
        <MonthGrid
          calendar={calendar.data}
          anchor={anchor}
          today={today}
          locale={locale}
          onDay={openDay}
        />
      ) : view === "week" ? (
        <WeekList
          calendar={calendar.data}
          today={today}
          locale={locale}
          onDay={openDay}
          onItem={openItem}
        />
      ) : (
        <DayList calendar={calendar.data} day={anchor} onItem={openItem} />
      )}
    </Screen>
  );
}

function EntryRow({ entry, onItem }: { entry: Entry; onItem: (item: Item) => void }) {
  const { t } = useTranslation();
  const label = t(`calendar.kinds.${entry.kind}`);
  return (
    <Pressable
      accessibilityRole="link"
      accessibilityLabel={`${label}: ${entry.item.title}`}
      onPress={() => {
        onItem(entry.item);
      }}
      className="flex-row items-center gap-2 border-t border-border py-2"
    >
      <Text variant="caption" tone="muted" className="w-24">
        {label}
      </Text>
      <Text className="flex-1" numberOfLines={2}>
        {entry.item.title}
      </Text>
    </Pressable>
  );
}

function DayList({
  calendar,
  day,
  onItem,
}: {
  calendar: Calendar;
  day: string;
  onItem: (item: Item) => void;
}) {
  const { t } = useTranslation();
  const entries = entriesByDay(calendar, day, day).get(day) ?? [];
  const goals = goalsOn(calendar, day);
  return (
    <View className="gap-3">
      <Card className="gap-1">
        {entries.length === 0 ? (
          <Text tone="muted">{t("calendar.emptyDay")}</Text>
        ) : (
          entries.map((entry) => <EntryRow key={entry.key} entry={entry} onItem={onItem} />)
        )}
      </Card>
      {goals.length === 0 ? null : (
        <Card className="gap-1">
          <Text variant="heading">{t("calendar.runningGoals")}</Text>
          {goals.map((goal) => (
            <Pressable
              key={goal.id}
              accessibilityRole="link"
              accessibilityLabel={goal.title}
              onPress={() => {
                onItem(goal);
              }}
              className="border-t border-border py-2"
            >
              <Text>{goal.title}</Text>
            </Pressable>
          ))}
        </Card>
      )}
    </View>
  );
}

function WeekList({
  calendar,
  today,
  locale,
  onDay,
  onItem,
}: {
  calendar: Calendar;
  today: string;
  locale: string;
  onDay: (day: string) => void;
  onItem: (item: Item) => void;
}) {
  const { t } = useTranslation();
  const byDay = entriesByDay(calendar, calendar.from, calendar.to);
  return (
    <View className="gap-3">
      {[...byDay.entries()].map(([day, entries]) => (
        <Card key={day} className="gap-1">
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={t("calendar.openDay", {
              day: formatDay(day, locale, { weekday: "long", day: "numeric", month: "long" }),
            })}
            onPress={() => {
              onDay(day);
            }}
          >
            <Text variant="label" tone={day === today ? "primary" : "default"}>
              {formatDay(day, locale, { weekday: "long", day: "numeric", month: "short" })}
            </Text>
          </Pressable>
          {entries.length === 0 ? (
            <Text variant="caption" tone="muted">
              {t("calendar.nothing")}
            </Text>
          ) : (
            entries.map((entry) => <EntryRow key={entry.key} entry={entry} onItem={onItem} />)
          )}
        </Card>
      ))}
    </View>
  );
}

function MonthGrid({
  calendar,
  anchor,
  today,
  locale,
  onDay,
}: {
  calendar: Calendar;
  anchor: string;
  today: string;
  locale: string;
  onDay: (day: string) => void;
}) {
  const { t } = useTranslation();
  const byDay = entriesByDay(calendar, calendar.from, calendar.to);
  const weeks = weeksOf(calendar.from, calendar.to);
  const month = anchor.slice(0, 7);
  const names = (weeks[0] ?? []).map((day) => formatDay(day, locale, { weekday: "short" }));
  return (
    <View className="overflow-hidden rounded-lg border border-border">
      <View className="flex-row border-b border-border">
        {names.map((name) => (
          <Text key={name} variant="caption" tone="muted" className="flex-1 py-1 text-center">
            {name}
          </Text>
        ))}
      </View>
      {weeks.map((week) => (
        <View key={week[0]} className="flex-row border-b border-border">
          {week.map((day) => {
            const entries = byDay.get(day) ?? [];
            const inMonth = day.startsWith(month);
            const shown = entries.slice(0, CELL_ENTRIES);
            const more = entries.length - shown.length;
            return (
              <Pressable
                key={day}
                accessibilityRole="button"
                accessibilityLabel={t("calendar.cell", {
                  day: formatDay(day, locale, { day: "numeric", month: "long" }),
                  count: entries.length,
                })}
                onPress={() => {
                  onDay(day);
                }}
                className={`min-h-20 flex-1 gap-0.5 border-r border-border p-1 ${
                  day === today ? "bg-primary/10" : ""
                }`}
              >
                <Text
                  variant="caption"
                  tone={day === today ? "primary" : inMonth ? "default" : "muted"}
                >
                  {Number(day.slice(8))}
                </Text>
                {shown.map((entry) => (
                  <Text key={entry.key} variant="caption" numberOfLines={1} tone="muted">
                    {entry.item.title}
                  </Text>
                ))}
                {more > 0 ? (
                  <Text variant="caption" tone="primary">
                    {t("calendar.more", { count: more })}
                  </Text>
                ) : null}
              </Pressable>
            );
          })}
        </View>
      ))}
    </View>
  );
}
