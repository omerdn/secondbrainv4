import type { ItemFilter, Tag } from "@secondbrain/api-types";
import { TagChip, Text } from "@secondbrain/ui";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { FILTER_KINDS, FILTER_STATES, toggled } from "./filterModel";

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <View className="gap-1">
      <Text variant="caption" tone="muted">
        {label}
      </Text>
      <View className="flex-row flex-wrap gap-2">{children}</View>
    </View>
  );
}

export interface FilterBarProps {
  filter: ItemFilter;
  /** The user's own tags (PRD E2). */
  tags: readonly Tag[];
  onChange: (filter: ItemFilter) => void;
}

/** Chooses what the list shows. Values in a row are alternatives; rows narrow each other. */
export function FilterBar({ filter, tags, onChange }: FilterBarProps) {
  const { t } = useTranslation();
  const state = filter.state ?? "active";
  return (
    <View className="gap-3">
      <Row label={t("views.filter.state")}>
        {FILTER_STATES.map((value) => (
          <TagChip
            key={value}
            label={t(`views.states.${value}`)}
            selected={state === value}
            onPress={() => {
              onChange({ ...filter, state: value });
            }}
          />
        ))}
      </Row>
      <Row label={t("views.filter.kind")}>
        {FILTER_KINDS.map((kind) => (
          <TagChip
            key={kind}
            label={t(`items.kinds.${kind}`)}
            selected={filter.kinds?.includes(kind) ?? false}
            onPress={() => {
              onChange({ ...filter, kinds: toggled(filter.kinds, kind) });
            }}
          />
        ))}
      </Row>
      {tags.length > 0 ? (
        <Row label={t("views.filter.tag")}>
          {tags.map((tag) => (
            <TagChip
              key={tag.id}
              label={tag.name}
              selected={filter.tag_ids?.includes(tag.id) ?? false}
              onPress={() => {
                onChange({ ...filter, tag_ids: toggled(filter.tag_ids, tag.id) });
              }}
            />
          ))}
        </Row>
      ) : null}
    </View>
  );
}
