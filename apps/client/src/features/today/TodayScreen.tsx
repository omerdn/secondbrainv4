import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type {
  ActiveGoal,
  DueTask,
  HomePreferences,
  Item,
  ResumeItem,
  RoutineDay,
  RoutineToday,
  Today,
} from "@secondbrain/api-types";
import { Button, Card, EmptyState, Input, Sheet, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { SessionGlanceCard } from "@/features/sessions";
import { Screen } from "@/features/shell";
import { deviceTimeZone, todayIn } from "@/lib/dates";
import { itemPath, kindLabel, StatusLineText } from "@/lib/items";

import { daysFrom, parseThreshold, percent } from "./model";

/** The Today screen (PRD H1-H6): everything for the day in one place. */
export function TodayScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const router = useRouter();
  const today = todayIn(deviceTimeZone());
  const key = ["today", today] as const;
  const screen = useQuery({ queryKey: key, queryFn: () => api.today.get(today) });
  const [settings, setSettings] = useState(false);

  const open = (path: string) => {
    const href = path as Href;
    router.push(href);
  };
  const openItem = (item: Item) => {
    open(itemPath(item.kind, item.id));
  };
  // A marked routine: its day changes in place; the rest (resume, forgotten) refetches.
  const markedDay = (routineId: string, day: RoutineDay) => {
    client.setQueryData<Today>(key, (current) =>
      current === undefined
        ? current
        : {
            ...current,
            routines: current.routines.map((r) => (r.item.id === routineId ? { ...r, day } : r)),
          },
    );
    void client.invalidateQueries({ queryKey: ["routine", routineId] });
  };

  return (
    <Screen title={t("nav.today")}>
      {screen.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : screen.isError ? (
        <EmptyState
          title={t("today.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void screen.refetch();
            },
          }}
        />
      ) : (
        <>
          {screen.data.session === null ? null : (
            <SessionGlanceCard
              glance={screen.data.session}
              onPress={() => {
                open("/session");
              }}
            />
          )}
          <Section title={t("today.routines")} empty={t("today.noRoutines")}>
            {screen.data.routines.map((routine) => (
              <RoutineRow key={routine.item.id} routine={routine} onMarked={markedDay} />
            ))}
          </Section>
          <Section title={t("today.overdue")} empty={t("today.noOverdue")} hideWhenEmpty>
            {screen.data.overdue.map((task) => (
              <TaskRow
                key={task.item.id}
                task={task}
                today={screen.data.today}
                onPress={openItem}
              />
            ))}
          </Section>
          <Section title={t("today.upcoming")} empty={t("today.noUpcoming")}>
            {screen.data.upcoming.map((task) => (
              <TaskRow
                key={task.item.id}
                task={task}
                today={screen.data.today}
                onPress={openItem}
              />
            ))}
          </Section>
          <Section title={t("today.goals")} empty={t("today.noGoals")}>
            {screen.data.goals.map((goal) => (
              <GoalRow key={goal.item.id} goal={goal} onPress={openItem} />
            ))}
          </Section>
          <Section title={t("today.resume")} empty={t("today.noResume")}>
            {screen.data.resume.map((entry) => (
              <ResumeRow
                key={entry.item.id}
                entry={entry}
                today={screen.data.today}
                onPress={openItem}
              />
            ))}
          </Section>
          <Section
            title={t("today.forgotten", { count: screen.data.forgotten_days })}
            empty={t("today.noForgotten")}
            action={
              <Button
                title={t("today.threshold")}
                variant="ghost"
                size="sm"
                onPress={() => {
                  setSettings(true);
                }}
              />
            }
          >
            {screen.data.forgotten.map((item) => (
              <ItemRow key={item.id} item={item} onPress={openItem} />
            ))}
          </Section>
          <ThresholdSheet
            visible={settings}
            current={screen.data.forgotten_days}
            onClose={() => {
              setSettings(false);
            }}
            onSaved={() => {
              setSettings(false);
              void client.invalidateQueries({ queryKey: ["today"] });
            }}
          />
        </>
      )}
    </Screen>
  );
}

function Section({
  title,
  empty,
  action,
  hideWhenEmpty = false,
  children,
}: {
  title: string;
  empty: string;
  action?: ReactNode;
  hideWhenEmpty?: boolean;
  children: ReactNode[];
}) {
  if (hideWhenEmpty && children.length === 0) return null;
  return (
    <Card className="gap-2">
      <View className="flex-row items-center justify-between gap-2">
        <Text variant="heading">{title}</Text>
        {action}
      </View>
      {children.length === 0 ? <Text tone="muted">{empty}</Text> : children}
    </Card>
  );
}

/** Today's occurrence of a routine, marked right here (H1). */
function RoutineRow({
  routine,
  onMarked,
}: {
  routine: RoutineToday;
  onMarked: (routineId: string, day: RoutineDay) => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const id = routine.item.id;
  const { day } = routine;
  const mark = useIdempotentMutation<RoutineDay, undefined>(
    (_, idempotencyKey) => api.routines.complete(id, { date: day.date }, { idempotencyKey }),
    {
      onSuccess: (next) => {
        onMarked(id, next);
      },
    },
  );
  const undo = useIdempotentMutation<RoutineDay, undefined>(
    (_, idempotencyKey) => api.routines.undo(id, day.date, { idempotencyKey }),
    {
      onSuccess: (next) => {
        onMarked(id, next);
      },
    },
  );
  const busy = mark.isPending || undo.isPending;
  return (
    <View className="gap-1 border-t border-border pt-2">
      <View className="flex-row items-center justify-between gap-2">
        <View className="flex-1 gap-0.5">
          <Text numberOfLines={2}>{routine.item.title}</Text>
          <Text variant="caption" tone={day.complete ? "success" : "muted"}>
            {day.complete
              ? t("today.routineDone", { done: day.done, target: day.target })
              : t("today.routineProgress", { done: day.done, target: day.target })}
          </Text>
        </View>
        {day.done > 0 ? (
          <Button
            title={t("today.undo")}
            accessibilityLabel={t("today.undoOf", { title: routine.item.title })}
            variant="ghost"
            size="sm"
            disabled={busy}
            onPress={() => {
              undo.mutate(undefined);
            }}
          />
        ) : null}
        <Button
          title={t("today.mark")}
          accessibilityLabel={t("today.markOf", { title: routine.item.title })}
          variant={day.complete ? "secondary" : "primary"}
          size="sm"
          disabled={busy}
          onPress={() => {
            mark.mutate(undefined);
          }}
        />
      </View>
      {mark.isError || undo.isError ? (
        <Text variant="caption" tone="danger">
          {t("today.markFailed")}
        </Text>
      ) : null}
    </View>
  );
}

function Row({
  label,
  onPress,
  children,
}: {
  label: string;
  onPress: () => void;
  children: ReactNode;
}) {
  return (
    <Pressable
      accessibilityRole="link"
      accessibilityLabel={label}
      onPress={onPress}
      className="gap-0.5 border-t border-border pt-2"
    >
      {children}
    </Pressable>
  );
}

function TaskRow({
  task,
  today,
  onPress,
}: {
  task: DueTask;
  today: string;
  onPress: (item: Item) => void;
}) {
  const { t } = useTranslation();
  const days = daysFrom(today, task.due_date);
  const due =
    days < 0
      ? t("today.dueAgo", { count: -days })
      : days === 0
        ? t("today.dueToday")
        : days === 1
          ? t("today.dueTomorrow")
          : t("today.dueIn", { count: days });
  return (
    <Row
      label={task.item.title}
      onPress={() => {
        onPress(task.item);
      }}
    >
      <Text numberOfLines={2}>{task.item.title}</Text>
      <Text variant="caption" tone={days < 0 ? "danger" : "muted"}>
        {`${due} · ${t("today.subtasks", { done: task.done, total: task.total })}`}
      </Text>
    </Row>
  );
}

function GoalRow({ goal, onPress }: { goal: ActiveGoal; onPress: (item: Item) => void }) {
  const { t } = useTranslation();
  return (
    <Row
      label={goal.item.title}
      onPress={() => {
        onPress(goal.item);
      }}
    >
      <Text numberOfLines={2}>{goal.item.title}</Text>
      <Text variant="caption" tone="muted">
        {t("today.goalLine", { actual: percent(goal.actual), pace: percent(goal.pace) })}
      </Text>
    </Row>
  );
}

function ResumeRow({
  entry,
  today,
  onPress,
}: {
  entry: ResumeItem;
  today: string;
  onPress: (item: Item) => void;
}) {
  const { t } = useTranslation();
  return (
    <Row
      label={entry.item.title}
      onPress={() => {
        onPress(entry.item);
      }}
    >
      <Text variant="caption" tone="muted">
        {t(`items.kinds.${kindLabel(entry.item.kind)}`)}
      </Text>
      <Text numberOfLines={2}>{entry.item.title}</Text>
      {entry.line === null ? null : <StatusLineText line={entry.line} today={today} />}
    </Row>
  );
}

function ItemRow({ item, onPress }: { item: Item; onPress: (item: Item) => void }) {
  const { t } = useTranslation();
  return (
    <Row
      label={item.title}
      onPress={() => {
        onPress(item);
      }}
    >
      <Text variant="caption" tone="muted">
        {t(`items.kinds.${kindLabel(item.kind)}`)}
      </Text>
      <Text numberOfLines={2}>{item.title}</Text>
    </Row>
  );
}

/** The forgotten threshold (H4): whole days, 1 to 365. */
function ThresholdSheet({
  visible,
  current,
  onClose,
  onSaved,
}: {
  visible: boolean;
  current: number;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const [draft, setDraft] = useState(String(current));
  const [invalid, setInvalid] = useState(false);
  const save = useIdempotentMutation<HomePreferences, number>(
    (days, idempotencyKey) =>
      api.today.setPreferences({ forgotten_days: days }, { idempotencyKey }),
    { onSuccess: onSaved },
  );
  const submit = () => {
    const days = parseThreshold(draft);
    if (days === null) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    save.mutate(days);
  };
  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t("today.thresholdTitle")}
      closeLabel={t("tags.close")}
    >
      <View className="gap-3">
        <Text tone="muted">{t("today.thresholdHint")}</Text>
        <Input
          label={t("today.thresholdLabel")}
          value={draft}
          keyboardType="number-pad"
          maxLength={3}
          error={invalid ? t("today.thresholdInvalid") : undefined}
          onChangeText={setDraft}
          onSubmitEditing={submit}
        />
        {save.isError ? <Text tone="danger">{t("today.saveFailed")}</Text> : null}
        <View className="self-start">
          <Button
            title={t("today.save")}
            loading={save.isPending}
            disabled={save.isPending}
            onPress={submit}
          />
        </View>
      </View>
    </Sheet>
  );
}
