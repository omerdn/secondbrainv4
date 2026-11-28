import type { Tag } from "@secondbrain/api-types";
import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import { Button, Input, Sheet, TagChip, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { useTagColors } from "@/lib/tags";

export interface TagPickerProps {
  itemId: string;
  visible: boolean;
  onClose: () => void;
}

/**
 * Attaches the user's own tags to an item and detaches them. Personal tags stay private even on
 * a shared item (PRD E2): the server only ever returns the caller's.
 */
export function TagPicker({ itemId, visible, onClose }: TagPickerProps) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);

  const all = useQuery({ queryKey: ["tags"], queryFn: api.tags.list, enabled: visible });
  const attached = useQuery({
    queryKey: ["item", itemId, "tags"],
    queryFn: () => api.items.tags(itemId),
    enabled: visible,
  });
  const colorOf = useTagColors(all.data);
  const attachedIds = new Set(attached.data?.map((tag) => tag.id));

  const refresh = async (tagId: string) => {
    await Promise.all([
      client.invalidateQueries({ queryKey: ["item", itemId, "tags"] }),
      client.invalidateQueries({ queryKey: ["tags", tagId, "items"] }),
    ]);
  };
  const failed = () => {
    setError(t("tags.failed"));
  };

  const toggle = useIdempotentMutation<void, { tagId: string; attach: boolean }>(
    ({ tagId, attach }, idempotencyKey) =>
      attach
        ? api.items.attachTag(itemId, tagId, { idempotencyKey })
        : api.items.detachTag(itemId, tagId, { idempotencyKey }),
    {
      onMutate: () => {
        setError(null);
      },
      onSuccess: (_data, { tagId }) => refresh(tagId),
      onError: failed,
    },
  );

  const create = useIdempotentMutation<Tag, string>(
    (tagName, idempotencyKey) => api.tags.create({ name: tagName }, { idempotencyKey }),
    {
      onMutate: () => {
        setError(null);
      },
      onSuccess: async (tag) => {
        setName("");
        await client.invalidateQueries({ queryKey: ["tags"], exact: true });
        toggle.mutate({ tagId: tag.id, attach: true });
      },
      onError: (err) => {
        setError(
          err instanceof ApiError && err.status === 409 ? t("tags.nameTaken") : t("tags.failed"),
        );
      },
    },
  );

  const trimmed = name.trim();
  const busy = toggle.isPending || create.isPending;
  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t("tags.pickerTitle")}
      closeLabel={t("tags.close")}
    >
      <View className="gap-4">
        {all.isPending || attached.isPending ? (
          <Text tone="muted">{t("common.loading")}</Text>
        ) : all.isError || attached.isError ? (
          <Text tone="danger">{t("tags.loadFailed")}</Text>
        ) : all.data.length === 0 ? (
          <Text tone="muted">{t("tags.emptyHint")}</Text>
        ) : (
          <View className="flex-row flex-wrap gap-2">
            {all.data.map((tag) => {
              const selected = attachedIds.has(tag.id);
              return (
                <TagChip
                  key={tag.id}
                  label={tag.name}
                  teamColor={colorOf(tag)}
                  selected={selected}
                  onPress={
                    busy
                      ? undefined
                      : () => {
                          toggle.mutate({ tagId: tag.id, attach: !selected });
                        }
                  }
                />
              );
            })}
          </View>
        )}
        <Input
          label={t("tags.newTag")}
          value={name}
          onChangeText={setName}
          maxLength={50}
          autoCapitalize="none"
        />
        <Button
          title={t("tags.create")}
          variant="secondary"
          disabled={trimmed === "" || busy}
          loading={create.isPending}
          onPress={() => {
            create.mutate(trimmed);
          }}
        />
        {error !== null ? (
          <Text tone="danger" accessibilityRole="alert">
            {error}
          </Text>
        ) : null}
      </View>
    </Sheet>
  );
}
