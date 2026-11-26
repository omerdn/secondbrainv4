import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Goal, GoalLink } from "@secondbrain/api-types";
import { Button, Card, Input, Sheet, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { kindLabel } from "@/lib/items";

import { LINKABLE_KINDS, linkCandidates, normalizeWeight, weightShares } from "./model";

interface LinkVars {
  itemId: string;
  weight?: string;
}

/** Link, reweigh and unlink mutations; each answer is the goal as it now is. */
function useGoalLinks(goal: Goal) {
  const api = useApi();
  const client = useQueryClient();
  const id = goal.item.id;
  const settle = (next: Goal) => {
    client.setQueryData(["goal", id], next);
    return client.invalidateQueries({ queryKey: ["goal", id, "progress"] });
  };
  const link = useIdempotentMutation<Goal, LinkVars>(
    ({ itemId, weight }, idempotencyKey) =>
      api.goals.link(id, itemId, weight === undefined ? {} : { weight }, { idempotencyKey }),
    { onSuccess: settle },
  );
  const unlink = useIdempotentMutation<Goal, { itemId: string }>(
    ({ itemId }, idempotencyKey) => api.goals.unlink(id, itemId, { idempotencyKey }),
    { onSuccess: settle },
  );
  return { link, unlink };
}

function LinkRow({
  link,
  share,
  editable,
  onSave,
  onRemove,
  busy,
}: {
  link: GoalLink;
  share: number | undefined;
  editable: boolean;
  onSave: (weight: string) => void;
  onRemove: () => void;
  busy: boolean;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(link.weight);
  const [invalid, setInvalid] = useState(false);
  const changed = draft.trim() !== link.weight;
  const save = () => {
    const weight = normalizeWeight(draft);
    if (weight === null) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setDraft(weight);
    onSave(weight);
  };
  return (
    <View className="gap-2 border-t border-border pt-3">
      <View className="flex-row items-center justify-between gap-2">
        <View className="flex-1 gap-0.5">
          <Text variant="caption" tone="muted">
            {t(`items.kinds.${kindLabel(link.item.kind)}`)}
          </Text>
          <Text className="font-semibold" numberOfLines={2}>
            {link.item.title}
          </Text>
          {link.contributes ? (
            share !== undefined ? (
              <Text variant="caption" tone="muted">
                {t("goals.share", { percent: Math.round(share * 100) })}
              </Text>
            ) : null
          ) : (
            <Text variant="caption" tone="muted">
              {t("goals.pageNote")}
            </Text>
          )}
        </View>
        {editable ? (
          <Button
            title={t("goals.remove")}
            accessibilityLabel={t("goals.removeOf", { title: link.item.title })}
            variant="ghost"
            size="sm"
            disabled={busy}
            onPress={onRemove}
          />
        ) : null}
      </View>
      {editable && link.contributes ? (
        <View className="flex-row items-end gap-2">
          <View className="flex-1">
            <Input
              label={t("goals.weightOf", { title: link.item.title })}
              value={draft}
              keyboardType="decimal-pad"
              maxLength={6}
              error={invalid ? t("goals.errors.weight") : undefined}
              onChangeText={setDraft}
              onSubmitEditing={save}
            />
          </View>
          {changed ? (
            <Button
              title={t("goals.saveWeight")}
              variant="secondary"
              disabled={busy}
              onPress={save}
            />
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

/** Picks a task, routine, tracker or page to link (PRD D2). No hover: rows are pressed. */
function LinkPicker({
  goal,
  visible,
  onClose,
  onPick,
}: {
  goal: Goal;
  visible: boolean;
  onClose: () => void;
  onPick: (itemId: string) => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const [query, setQuery] = useState("");
  const items = useQuery({
    queryKey: ["goal-candidates"],
    enabled: visible,
    queryFn: () => api.items.filter({ kinds: [...LINKABLE_KINDS] }, { limit: 200 }),
  });
  const candidates = items.isSuccess ? linkCandidates(items.data.items, goal.links, query) : [];
  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t("goals.pickerTitle")}
      closeLabel={t("tags.close")}
    >
      <View className="gap-3">
        <Input
          label={t("goals.search")}
          value={query}
          onChangeText={setQuery}
          autoCapitalize="none"
        />
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
          <Text tone="muted">{t("goals.noCandidates")}</Text>
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

/** The goal's linked items with their weights (PRD D2, D3). */
export function GoalLinks({ goal }: { goal: Goal }) {
  const { t } = useTranslation();
  const { link, unlink } = useGoalLinks(goal);
  const [picking, setPicking] = useState(false);
  const editable = goal.item.deleted_at === null && goal.item.archived_at === null;
  const shares = weightShares(goal.links);
  const busy = link.isPending || unlink.isPending;
  return (
    <Card className="gap-3">
      <View className="flex-row items-center justify-between gap-2">
        <Text variant="label">{t("goals.links")}</Text>
        {editable ? (
          <Button
            title={t("goals.addLink")}
            size="sm"
            onPress={() => {
              setPicking(true);
            }}
          />
        ) : null}
      </View>
      {goal.links.length === 0 ? <Text tone="muted">{t("goals.noLinks")}</Text> : null}
      {goal.links.map((item) => (
        <LinkRow
          key={`${item.item.id}-${item.weight}`}
          link={item}
          share={shares.get(item.item.id)}
          editable={editable}
          busy={busy}
          onSave={(weight) => {
            link.mutate({ itemId: item.item.id, weight });
          }}
          onRemove={() => {
            unlink.mutate({ itemId: item.item.id });
          }}
        />
      ))}
      {link.isError || unlink.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("goals.linkFailed")}
        </Text>
      ) : null}
      <LinkPicker
        goal={goal}
        visible={picking}
        onClose={() => {
          setPicking(false);
        }}
        onPick={(itemId) => {
          setPicking(false);
          link.mutate({ itemId });
        }}
      />
    </Card>
  );
}
