import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type {
  Routine,
  RoutineRotation,
  RoutineRotationSchedule,
  RoutineSwapRequest,
  UpdateRoutineRequest,
} from "@secondbrain/api-types";
import { Button, Card, TagChip, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { useSession } from "@/features/auth";
import { addDays, todayIn } from "@/lib/dates";

import { formatDay } from "./describe";
import { linesOn, movedUp, namesOf, PEOPLE_DAYS, recentDays } from "./shared";

const ROTATIONS: readonly (RoutineRotation | null)[] = [null, "every_occurrence", "weekly"];
/** Upcoming turns the rotation list shows. */
const TURNS_SHOWN = 8;

/**
 * A shared routine (PRD R5, R6, T3.3.3): who did what on the last days, how a day is completed,
 * and the rotation with its swaps. Hidden for a routine nobody else is on.
 */
export function SharedRoutineCard({ routine }: { routine: Routine }) {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const { state } = useSession();
  const me = state.status === "signedIn" ? state.user.id : null;
  const id = routine.item.id;
  const today = todayIn(routine.timezone);
  const from = addDays(today, -(PEOPLE_DAYS - 1));
  const members = useQuery({
    queryKey: ["item", id, "members"],
    queryFn: () => api.items.members(id),
  });
  const shared = (members.data?.members.length ?? 0) > 1;
  const people = useQuery({
    queryKey: ["routine", id, "people", from, today],
    queryFn: () => api.routines.people(id, { from, to: today }),
    enabled: shared,
  });
  const rotation = useQuery({
    queryKey: ["routine", id, "rotation"],
    queryFn: () => api.routines.rotation(id),
    enabled: shared && routine.rotation !== null,
  });
  const [picked, setPicked] = useState<string[]>([]);
  const refresh = () => client.invalidateQueries({ queryKey: ["routine", id] });
  const update = useIdempotentMutation<Routine, UpdateRoutineRequest>(
    (body, idempotencyKey) => api.routines.update(id, body, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const swap = useIdempotentMutation<RoutineRotationSchedule, RoutineSwapRequest>(
    (body, idempotencyKey) => api.routines.swap(id, body, { idempotencyKey }),
    {
      onSuccess: async () => {
        setPicked([]);
        await refresh();
      },
    },
  );

  if (!shared || members.data === undefined) return null;
  const names = namesOf(members.data.members);
  const nameOf = (userId: string) =>
    userId === me ? t("routines.shared.you") : (names.get(userId) ?? t("routines.shared.former"));
  const canManage = members.data.can_manage;
  const live = routine.item.deleted_at === null && routine.item.archived_at === null;
  const order = rotation.data?.members ?? members.data.members.map((m) => m.user_id);
  const individual = routine.completion_mode === "individual";
  const swapError =
    swap.error === null
      ? null
      : swap.error instanceof ApiError && swap.error.status === 403
        ? t("routines.shared.swapOwnOnly")
        : t("routines.saveFailed");

  return (
    <Card className="gap-4">
      <Text variant="label">{t("routines.shared.title")}</Text>
      <View className="gap-2">
        <Text variant="caption" tone="muted">
          {t("routines.shared.whoDidIt")}
        </Text>
        {people.isPending ? (
          <Text tone="muted">{t("common.loading")}</Text>
        ) : people.isError ? (
          <Button
            title={t("items.detail.retry")}
            variant="ghost"
            size="sm"
            onPress={() => {
              void people.refetch();
            }}
          />
        ) : (
          recentDays(today)
            .filter((date) => date >= routine.start_date)
            .map((date) => (
              <View key={date} className="gap-1">
                <Text variant="caption">
                  {date === today ? t("routines.today") : formatDay(date, i18n.language)}
                </Text>
                <View className="flex-row flex-wrap gap-2">
                  {linesOn(people.data, date, order).map((line) => (
                    <Text
                      key={line.userId}
                      tone={line.done > 0 ? undefined : "muted"}
                      accessibilityLabel={t("routines.shared.personDone", {
                        name: nameOf(line.userId),
                        done: line.done,
                      })}
                    >
                      {individual && line.responsible
                        ? t("routines.shared.lineOfTarget", {
                            name: nameOf(line.userId),
                            done: line.done,
                            target: routine.daily_target,
                          })
                        : t("routines.shared.line", { name: nameOf(line.userId), done: line.done })}
                    </Text>
                  ))}
                </View>
              </View>
            ))
        )}
      </View>

      {canManage && live ? (
        <View className="gap-2">
          <Text variant="caption" tone="muted">
            {t("routines.shared.mode")}
          </Text>
          <View className="flex-row flex-wrap gap-2">
            {(["pool", "individual"] as const).map((mode) => (
              <TagChip
                key={mode}
                label={t(`routines.shared.modes.${mode}`)}
                selected={routine.completion_mode === mode}
                onPress={() => {
                  if (routine.completion_mode === mode) return;
                  // R6: turns exist only in a shared pool, so each person ends the rotation.
                  update.mutate(
                    mode === "individual" && routine.rotation !== null
                      ? { completion_mode: mode, rotation: null }
                      : { completion_mode: mode },
                  );
                }}
              />
            ))}
          </View>
          <Text variant="caption" tone="muted">
            {t("routines.shared.rotation")}
          </Text>
          {individual ? (
            <Text tone="muted">{t("routines.shared.rotationNeedsPool")}</Text>
          ) : (
            <>
              <View className="flex-row flex-wrap gap-2">
                {ROTATIONS.map((value) => (
                  <TagChip
                    key={value ?? "fixed"}
                    label={t(`routines.shared.rotations.${value ?? "fixed"}`)}
                    selected={routine.rotation === value}
                    onPress={() => {
                      if (routine.rotation !== value) update.mutate({ rotation: value });
                    }}
                  />
                ))}
              </View>
              {routine.rotation !== null ? (
                <Text variant="caption" tone="muted">
                  {t("routines.shared.individualEndsRotation")}
                </Text>
              ) : null}
            </>
          )}
          {update.isError ? (
            <Text tone="danger" accessibilityRole="alert">
              {t("routines.saveFailed")}
            </Text>
          ) : null}
        </View>
      ) : null}

      {routine.rotation !== null ? (
        <View className="gap-2">
          <Text variant="caption" tone="muted">
            {t("routines.shared.turns")}
          </Text>
          {rotation.isPending ? (
            <Text tone="muted">{t("common.loading")}</Text>
          ) : rotation.isError ? (
            <Button
              title={t("items.detail.retry")}
              variant="ghost"
              size="sm"
              onPress={() => {
                void rotation.refetch();
              }}
            />
          ) : (
            <>
              {canManage && live ? (
                <View className="gap-1">
                  {rotation.data.members.map((userId, index) => (
                    <View key={userId} className="flex-row items-center gap-2">
                      <Text>{`${String(index + 1)}. ${nameOf(userId)}`}</Text>
                      {index > 0 ? (
                        <Button
                          title={t("routines.shared.moveUp")}
                          accessibilityLabel={t("routines.shared.moveUpLabel", {
                            name: nameOf(userId),
                          })}
                          variant="ghost"
                          size="sm"
                          loading={update.isPending}
                          onPress={() => {
                            update.mutate({
                              rotation_order: movedUp(rotation.data.members, index),
                            });
                          }}
                        />
                      ) : null}
                    </View>
                  ))}
                </View>
              ) : null}
              <View className="flex-row flex-wrap gap-2">
                {rotation.data.assignments.slice(0, TURNS_SHOWN).map((turn) => (
                  <TagChip
                    key={turn.date}
                    label={t("routines.shared.turn", {
                      day:
                        turn.date === today
                          ? t("routines.today")
                          : formatDay(turn.date, i18n.language),
                      name: nameOf(turn.user_id),
                    })}
                    selected={picked.includes(turn.date)}
                    onPress={() => {
                      if (!live) return;
                      setPicked((current) =>
                        current.includes(turn.date)
                          ? current.filter((date) => date !== turn.date)
                          : [...current, turn.date].slice(-2),
                      );
                    }}
                  />
                ))}
              </View>
              {live ? (
                <Text variant="caption" tone="muted">
                  {t("routines.shared.swapHint")}
                </Text>
              ) : null}
              {picked.length === 2 ? (
                <View className="self-start">
                  <Button
                    title={t("routines.shared.swap")}
                    size="sm"
                    loading={swap.isPending}
                    onPress={() => {
                      const [first, second] = picked;
                      if (first !== undefined && second !== undefined) {
                        swap.mutate({ first, second });
                      }
                    }}
                  />
                </View>
              ) : null}
              {swapError !== null ? (
                <Text tone="danger" accessibilityRole="alert">
                  {swapError}
                </Text>
              ) : null}
            </>
          )}
        </View>
      ) : null}
    </Card>
  );
}
