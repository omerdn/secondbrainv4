import type { Item, Tag } from "@secondbrain/api-types";
import { Card, TagChip, Text } from "@secondbrain/ui";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { kindLabel } from "./labels";
import { useStatusLine } from "./statusLines";
import { StatusLineText } from "./StatusLineText";

export interface ItemCardProps {
  item: Item;
  /** The viewer's own tags on this item. */
  tags?: readonly Tag[];
  /** Opens the item; without it the card is not pressable. */
  onPress?: () => void;
}

/** The card every item kind is listed with: kind, state, title, latest status line (S3),
 * description and tags. */
export function ItemCard({ item, tags = [], onPress }: ItemCardProps) {
  const { t } = useTranslation();
  const line = useStatusLine(item.id);
  const kind = t(`items.kinds.${kindLabel(item.kind)}`);
  const state =
    item.deleted_at !== null
      ? t("items.state.trash")
      : item.archived_at !== null
        ? t("items.state.archived")
        : null;
  return (
    <Card onPress={onPress} accessibilityLabel={`${kind}: ${item.title}`}>
      <View className="gap-2">
        <View className="flex-row items-center justify-between gap-2">
          <Text variant="caption" tone="muted">
            {kind}
          </Text>
          {state !== null ? (
            <Text variant="caption" tone="muted">
              {state}
            </Text>
          ) : null}
        </View>
        <Text className="font-semibold" numberOfLines={2}>
          {item.title}
        </Text>
        {line !== null ? <StatusLineText line={line} /> : null}
        {item.description !== null ? (
          <Text variant="label" tone="muted" className="font-normal" numberOfLines={2}>
            {item.description}
          </Text>
        ) : null}
        {tags.length > 0 ? (
          <View className="flex-row flex-wrap gap-1">
            {tags.map((tag) => (
              <TagChip key={tag.id} label={tag.name} />
            ))}
          </View>
        ) : null}
      </View>
    </Card>
  );
}
