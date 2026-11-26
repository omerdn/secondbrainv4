import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Routine, RoutineDay } from "@secondbrain/api-types";
import { Button, Card, cx, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { addDays, todayIn } from "@/lib/dates";

import {
  canMark,
  completionShare,
  type HeatLevel,
  heatLevel,
  heatmapStart,
  heatmapWeeks,
} from "./checkin";
import { formatDay } from "./describe";

const CELL = 14;
const CELL_GAP = 3;
const BAR_HEIGHT = 8;

/** One class per shade; full strings so Tailwind keeps them (R8). */
const LEVEL_CLASS: Record<HeatLevel | "future", string> = {
  future: "bg-transparent",
  off: "border border-border bg-transparent",
  0: "bg-border",
  1: "bg-success/30",
  2: "bg-success/55",
  3: "bg-success/80",
  4: "bg-success",
};

function HeatCell({
  day,
  selected,
  label,
  onPress,
}: {
  day: RoutineDay | null;
  selected: boolean;
  label: string;
  onPress: () => void;
}) {
  const level = day === null ? "future" : heatLevel(day);
  const style = { width: CELL, height: CELL, borderRadius: 3 };
  if (day === null) {
    return <View style={style} />;
  }
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ selected }}
      aria-selected={selected}
      onPress={onPress}
      style={style}
      className={cx(LEVEL_CLASS[level], selected && "border-2 border-primary")}
    />
  );
}

/**
 * Marking a day with +1 (R4), the streak (R9) and the heatmap (R8). Tapping a square picks that
 * day; only the last 3 days can be marked (R7).
 */
export function CheckInCard({ routine }: { routine: Routine }) {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const id = routine.item.id;
  const today = todayIn(routine.timezone);
  const from = heatmapStart(today);
  const [selected, setSelected] = useState(today);
  const days = useQuery({
    queryKey: ["routine", id, "days", from, today],
    queryFn: () => api.routines.days(id, { from, to: today }),
  });
  const streak = useQuery({
    queryKey: ["routine", id, "streak"],
    queryFn: () => api.routines.streak(id),
  });
  const refresh = () => client.invalidateQueries({ queryKey: ["routine", id] });
  const add = useIdempotentMutation<RoutineDay, { date: string }>(
    ({ date }, idempotencyKey) => api.routines.complete(id, { date }, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const undo = useIdempotentMutation<RoutineDay, { date: string }>(
    ({ date }, idempotencyKey) => api.routines.undo(id, date, { idempotencyKey }),
    { onSuccess: refresh },
  );

  const day = days.data?.find((d) => d.date === selected);
  const live = routine.item.deleted_at === null && routine.item.archived_at === null;
  const markable = live && day?.scheduled === true && canMark(selected, today);
  const busy = add.isPending || undo.isPending;
  const dayName = selected === today ? t("routines.today") : formatDay(selected, i18n.language);
  const describeCell = (cell: RoutineDay) =>
    cell.scheduled
      ? t("routines.cell", {
          day: formatDay(cell.date, i18n.language),
          done: cell.done,
          target: cell.target,
        })
      : t("routines.cellOff", { day: formatDay(cell.date, i18n.language) });

  return (
    <Card className="gap-4">
      <View className="gap-2">
        <Text variant="label">{dayName}</Text>
        {days.isPending ? (
          <Text tone="muted">{t("common.loading")}</Text>
        ) : days.isError ? (
          <Button
            title={t("items.detail.retry")}
            variant="ghost"
            size="sm"
            onPress={() => {
              void days.refetch();
            }}
          />
        ) : day === undefined || !day.scheduled ? (
          <Text tone="muted">{t("routines.notScheduled")}</Text>
        ) : (
          <View className="gap-2">
            <Text
              variant="heading"
              accessibilityLabel={t("routines.doneOf", { done: day.done, target: day.target })}
            >
              {t("routines.doneOf", { done: day.done, target: day.target })}
            </Text>
            <View className="overflow-hidden rounded-full bg-border" style={{ height: BAR_HEIGHT }}>
              <View
                className="bg-success"
                style={{ height: BAR_HEIGHT, width: `${String(completionShare(day) * 100)}%` }}
              />
            </View>
            {day.complete ? <Text tone="success">{t("routines.completeDay")}</Text> : null}
          </View>
        )}
        {live && day?.scheduled === true && !canMark(selected, today) ? (
          <Text tone="muted" variant="caption">
            {t("routines.tooOld")}
          </Text>
        ) : null}
        <View className="flex-row gap-2">
          <Button
            title={t("routines.plusOne")}
            accessibilityLabel={t("routines.plusOneFor", { day: dayName })}
            disabled={!markable || busy}
            loading={add.isPending}
            onPress={() => {
              add.mutate({ date: selected });
            }}
          />
          <Button
            title={t("routines.undo")}
            variant="secondary"
            disabled={!markable || busy || (day?.done ?? 0) === 0}
            loading={undo.isPending}
            onPress={() => {
              undo.mutate({ date: selected });
            }}
          />
        </View>
        {add.isError || undo.isError ? (
          <Text tone="danger" accessibilityRole="alert">
            {t("routines.markFailed")}
          </Text>
        ) : null}
      </View>
      <View className="gap-1">
        <Text variant="label">
          {streak.isSuccess
            ? t("routines.streak", { count: streak.data.current })
            : t("routines.streakLabel")}
        </Text>
        {streak.data?.today_pending === true ? (
          <Text tone="muted" variant="caption">
            {t("routines.streakPending")}
          </Text>
        ) : null}
      </View>
      {days.isSuccess ? (
        <View className="gap-2">
          <Text variant="label">{t("routines.heatmap")}</Text>
          <View className="flex-row" style={{ gap: CELL_GAP }}>
            {heatmapWeeks(days.data, today).map((week, index) => (
              <View key={addDays(from, index * 7)} style={{ gap: CELL_GAP }}>
                {week.map((cell, index) => (
                  <HeatCell
                    key={cell?.date ?? `empty-${String(index)}`}
                    day={cell}
                    selected={cell?.date === selected}
                    label={cell === null ? "" : describeCell(cell)}
                    onPress={() => {
                      if (cell !== null) {
                        setSelected(cell.date);
                      }
                    }}
                  />
                ))}
              </View>
            ))}
          </View>
          <View className="flex-row items-center" style={{ gap: CELL_GAP }}>
            <Text variant="caption" tone="muted">
              {t("routines.less")}
            </Text>
            {([0, 1, 2, 3, 4] as const).map((level) => (
              <View
                key={level}
                className={LEVEL_CLASS[level]}
                style={{ width: CELL, height: CELL, borderRadius: 3 }}
              />
            ))}
            <Text variant="caption" tone="muted">
              {t("routines.more")}
            </Text>
          </View>
        </View>
      ) : null}
    </Card>
  );
}
