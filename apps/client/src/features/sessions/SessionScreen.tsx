import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Item, PomodoroConfig, TimerCommand, WorkSession } from "@secondbrain/api-types";
import { Button, Card, EmptyState, Input, Sheet, Text } from "@secondbrain/ui";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { useServerEvent } from "@/connectivity";
import { Screen } from "@/features/shell";
import { kindLabel } from "@/lib/items";

import {
  clockFace,
  clockOffset,
  formatClock,
  loggedItems,
  parseMinutes,
  type Preset,
  PRESETS,
  presetStart,
  segmentLog,
} from "./model";

/** The open session and the server clock's offset from ours when it was read. */
interface Loaded {
  session: WorkSession;
  offset: number;
}

/** The clock's size; the text variants stop at headings. */
const CLOCK_STYLE = { fontSize: 56, lineHeight: 64 } as const;

const SESSION_KEY = ["session", "current"] as const;

const loaded = (session: WorkSession): Loaded | null =>
  session.open ? { session, offset: clockOffset(session.server_time, Date.now()) } : null;

/** Items time can go to (W3). */
const SESSION_KINDS = ["task", "routine", "tracker", "page"];

/** The local clock, ticking every second while `ticking`. */
function useNow(ticking: boolean, changed: unknown): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    setNow(Date.now());
    if (!ticking) return undefined;
    const id = setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => {
      clearInterval(id);
    };
  }, [ticking, changed]);
  return now;
}

/** Titles of the items the session names; an item the caller cannot see stays unnamed. */
function useItemTitles(ids: readonly string[]): Map<string, Item> {
  const api = useApi();
  const results = useQueries({
    queries: ids.map((id) => ({
      queryKey: ["item", id],
      queryFn: () => api.items.get(id),
      retry: false,
    })),
  });
  const items = new Map<string, Item>();
  results.forEach((result, index) => {
    const id = ids[index];
    if (id !== undefined && result.data !== undefined) items.set(id, result.data);
  });
  return items;
}

/** Work session timer (PRD W1-W4, ARCHITECTURE §11): the server owns the timer. */
export function SessionScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const session = useQuery({
    queryKey: SESSION_KEY,
    queryFn: async () => {
      try {
        return loaded(await api.sessions.current());
      } catch (error) {
        if (error instanceof ApiError && error.status === 404) return null;
        throw error;
      }
    },
  });
  // Another device or tab changed the timer: read it again.
  useServerEvent("session.timer", () => {
    void client.invalidateQueries({ queryKey: SESSION_KEY });
  });

  const settle = (next: WorkSession) => {
    client.setQueryData(SESSION_KEY, loaded(next));
  };
  const open = useIdempotentMutation<WorkSession, PomodoroConfig>(
    (pomodoro, idempotencyKey) => api.sessions.open({ pomodoro }, { idempotencyKey }),
    { onSuccess: settle },
  );

  return (
    <Screen title={t("nav.session")}>
      {session.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : session.isError ? (
        <EmptyState
          title={t("sessions.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void session.refetch();
            },
          }}
        />
      ) : session.data === null ? (
        <OpenForm
          busy={open.isPending}
          failed={open.isError}
          onOpen={(pomodoro) => {
            open.mutate(pomodoro);
          }}
        />
      ) : (
        <Timer
          loaded={session.data}
          onChange={settle}
          onClosed={() => {
            client.setQueryData(SESSION_KEY, null);
            void client.invalidateQueries({ queryKey: ["sessions"] });
          }}
          onConflict={() => {
            void session.refetch();
          }}
        />
      )}
    </Screen>
  );
}

/** Pomodoro lengths, then "Oturum başlat" (W1, W2). */
function OpenForm({
  busy,
  failed,
  onOpen,
}: {
  busy: boolean;
  failed: boolean;
  onOpen: (pomodoro: PomodoroConfig) => void;
}) {
  const { t } = useTranslation();
  const [work, setWork] = useState("25");
  const [short, setShort] = useState("5");
  const [long, setLong] = useState("15");
  const [invalid, setInvalid] = useState(false);
  const submit = () => {
    const minutes = [work, short, long].map(parseMinutes);
    if (minutes.some((m) => m === null)) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    const [w = 25, s = 5, l = 15] = minutes.map((m) => m ?? 0);
    onOpen({ work_ms: w * 60_000, short_break_ms: s * 60_000, long_break_ms: l * 60_000 });
  };
  const fields = [
    { label: t("sessions.lengths.work"), value: work, set: setWork },
    { label: t("sessions.lengths.shortBreak"), value: short, set: setShort },
    { label: t("sessions.lengths.longBreak"), value: long, set: setLong },
  ];
  return (
    <Card className="gap-3">
      <Text variant="heading">{t("sessions.startTitle")}</Text>
      <Text tone="muted">{t("sessions.startHint")}</Text>
      <View className="flex-row flex-wrap gap-3">
        {fields.map((field) => (
          <View key={field.label} className="min-w-24 flex-1">
            <Input
              label={field.label}
              value={field.value}
              keyboardType="number-pad"
              maxLength={3}
              onChangeText={field.set}
            />
          </View>
        ))}
      </View>
      {invalid ? <Text tone="danger">{t("sessions.errors.minutes")}</Text> : null}
      {failed ? <Text tone="danger">{t("sessions.saveFailed")}</Text> : null}
      <View className="self-start">
        <Button title={t("sessions.open")} loading={busy} disabled={busy} onPress={submit} />
      </View>
    </Card>
  );
}

function Timer({
  loaded: { session, offset },
  onChange,
  onClosed,
  onConflict,
}: {
  loaded: Loaded;
  onChange: (session: WorkSession) => void;
  onClosed: () => void;
  onConflict: () => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const [preset, setPreset] = useState<Preset>("pomodoro");
  const [picking, setPicking] = useState(false);
  const { timer } = session;
  const running = timer.state === "running";
  const now = useNow(running, session) + offset;
  const face = clockFace(timer, now);
  // Stopped: the chosen preset's length, so the clock shows what a start would count.
  const planned = presetStart(preset, session.pomodoro).planned_ms;
  const shown =
    timer.state === "stopped"
      ? formatClock(planned ?? 0, true)
      : formatClock(face.ms, face.countdown);
  const log = segmentLog(session.segments, timer, now);
  const titleIds = loggedItems(session.segments);
  if (timer.item_id !== null && !titleIds.includes(timer.item_id)) titleIds.push(timer.item_id);
  const items = useItemTitles(titleIds);
  const itemTitle = (id: string | null) =>
    id === null ? null : (items.get(id)?.title ?? t("sessions.hiddenItem"));

  const command = useIdempotentMutation<WorkSession, TimerCommand>(
    (body, idempotencyKey) => api.sessions.command(session.id, body, { idempotencyKey }),
    {
      onSuccess: onChange,
      onError: (error) => {
        if (error instanceof ApiError && error.status === 409) onConflict();
      },
    },
  );
  const close = useIdempotentMutation<WorkSession, undefined>(
    (_, idempotencyKey) => api.sessions.close(session.id, { idempotencyKey }),
    { onSuccess: onClosed },
  );
  const busy = command.isPending || close.isPending;
  const send = (body: TimerCommand) => {
    command.mutate(body);
  };
  const item = itemTitle(timer.item_id);
  const modeLabel =
    timer.state === "stopped" || timer.mode === null
      ? t("sessions.stopped")
      : t(`sessions.modes.${timer.mode}`);

  return (
    <View className="gap-4">
      <Card className="items-center gap-3">
        <Text variant="caption" tone="muted">
          {modeLabel}
          {timer.state === "paused" ? ` · ${t("sessions.paused")}` : ""}
        </Text>
        <Text
          accessibilityRole="timer"
          accessibilityLabel={t("sessions.clock")}
          className="font-semibold tabular-nums"
          style={CLOCK_STYLE}
        >
          {shown}
        </Text>
        {face.overtime && timer.state !== "stopped" ? (
          <Text tone="muted">{t("sessions.timeUp")}</Text>
        ) : null}
        <View className="flex-row flex-wrap items-center justify-center gap-2">
          <Text tone={item === null ? "muted" : "default"}>{item ?? t("sessions.noItem")}</Text>
          <Button
            title={item === null ? t("sessions.pickItem") : t("sessions.changeItem")}
            variant="ghost"
            size="sm"
            disabled={busy || running}
            onPress={() => {
              setPicking(true);
            }}
          />
        </View>
        {running ? (
          <Text variant="caption" tone="muted">
            {t("sessions.pauseToSwitch")}
          </Text>
        ) : null}

        {timer.state === "stopped" ? (
          <View className="w-full gap-3">
            <View className="flex-row flex-wrap justify-center gap-2">
              {PRESETS.map((p) => (
                <Pressable
                  key={p}
                  accessibilityRole="radio"
                  accessibilityState={{ selected: preset === p }}
                  accessibilityLabel={t(`sessions.presets.${p}`)}
                  onPress={() => {
                    setPreset(p);
                  }}
                  className={`rounded-full border px-3 py-1.5 ${
                    preset === p ? "border-primary bg-primary/10" : "border-border"
                  }`}
                >
                  <Text>{presetLabel(t, p, session.pomodoro)}</Text>
                </Pressable>
              ))}
            </View>
            <View className="self-center">
              <Button
                title={t("sessions.start")}
                disabled={busy}
                onPress={() => {
                  const start = presetStart(preset, session.pomodoro);
                  send({
                    command: "start",
                    ...start,
                    ...(timer.item_id === null ? {} : { item_id: timer.item_id }),
                  });
                }}
              />
            </View>
          </View>
        ) : (
          <View className="flex-row gap-2">
            {running ? (
              <Button
                title={t("sessions.pause")}
                disabled={busy}
                onPress={() => {
                  send({ command: "pause" });
                }}
              />
            ) : (
              <Button
                title={t("sessions.resume")}
                disabled={busy}
                onPress={() => {
                  send({ command: "resume" });
                }}
              />
            )}
            <Button
              title={t("sessions.stop")}
              variant="secondary"
              disabled={busy}
              onPress={() => {
                send({ command: "stop" });
              }}
            />
          </View>
        )}
        {command.isError ? <Text tone="danger">{t("sessions.saveFailed")}</Text> : null}
      </Card>

      <Card className="gap-2">
        <Text variant="heading">{t("sessions.log")}</Text>
        {log.length === 0 ? (
          <Text tone="muted">{t("sessions.emptyLog")}</Text>
        ) : (
          log.map((entry) => {
            const title = itemTitle(entry.itemId);
            const what =
              entry.mode === "break"
                ? t("sessions.logBreak", { minutes: entry.minutes })
                : title === null
                  ? t("sessions.logWork", { minutes: entry.minutes })
                  : t("sessions.logItem", { minutes: entry.minutes, title });
            return (
              <Text key={entry.id} tone={entry.current ? "default" : "muted"}>
                {entry.current ? `${what} · ${t("sessions.now")}` : what}
              </Text>
            );
          })
        )}
      </Card>

      <View className="self-start">
        <Button
          title={t("sessions.close")}
          variant="ghost"
          disabled={busy}
          onPress={() => {
            close.mutate(undefined);
          }}
        />
      </View>
      {close.isError ? <Text tone="danger">{t("sessions.saveFailed")}</Text> : null}

      <ItemPicker
        visible={picking}
        onClose={() => {
          setPicking(false);
        }}
        current={timer.item_id}
        onPick={(itemId) => {
          setPicking(false);
          send(
            itemId === null
              ? { command: "switch_item" }
              : { command: "switch_item", item_id: itemId },
          );
        }}
      />
    </View>
  );
}

function presetLabel(
  t: ReturnType<typeof useTranslation>["t"],
  preset: Preset,
  pomodoro: PomodoroConfig,
): string {
  const { planned_ms } = presetStart(preset, pomodoro);
  const name = t(`sessions.presets.${preset}`);
  return planned_ms === undefined
    ? name
    : t("sessions.presetWithMinutes", { name, minutes: Math.round(planned_ms / 60_000) });
}

/** Picks the item the time goes to, or none (W3). Rows are pressed; no hover. */
function ItemPicker({
  visible,
  current,
  onClose,
  onPick,
}: {
  visible: boolean;
  current: string | null;
  onClose: () => void;
  onPick: (itemId: string | null) => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const [query, setQuery] = useState("");
  const items = useQuery({
    queryKey: ["session-candidates"],
    enabled: visible,
    queryFn: () => api.items.filter({ kinds: SESSION_KINDS }, { limit: 200 }),
  });
  const needle = query.trim().toLocaleLowerCase();
  const candidates = items.isSuccess
    ? items.data.items.filter(
        (item) =>
          item.deleted_at === null &&
          item.id !== current &&
          (needle === "" || item.title.toLocaleLowerCase().includes(needle)),
      )
    : [];
  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t("sessions.pickerTitle")}
      closeLabel={t("tags.close")}
    >
      <View className="gap-3">
        <Input label={t("sessions.search")} value={query} onChangeText={setQuery} />
        {current !== null ? (
          <Button
            title={t("sessions.clearItem")}
            variant="ghost"
            size="sm"
            onPress={() => {
              onPick(null);
            }}
          />
        ) : null}
        {items.isPending ? (
          <Text tone="muted">{t("common.loading")}</Text>
        ) : items.isError ? (
          <Button
            title={t("items.detail.retry")}
            variant="ghost"
            size="sm"
            onPress={() => {
              void items.refetch();
            }}
          />
        ) : candidates.length === 0 ? (
          <Text tone="muted">{t("sessions.noCandidates")}</Text>
        ) : (
          candidates.map((item) => {
            const kind = t(`items.kinds.${kindLabel(item.kind)}`);
            return (
              <Pressable
                key={item.id}
                accessibilityRole="button"
                accessibilityLabel={`${kind}: ${item.title}`}
                onPress={() => {
                  onPick(item.id);
                }}
                className="gap-0.5 rounded-md border border-border p-3"
              >
                <Text variant="caption" tone="muted">
                  {kind}
                </Text>
                <Text>{item.title}</Text>
              </Pressable>
            );
          })
        )}
      </View>
    </Sheet>
  );
}
