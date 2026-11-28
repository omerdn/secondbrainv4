import { useApi } from "@secondbrain/api-client/react";
import type { ItemFilter, Tag } from "@secondbrain/api-types";
import { TagChip, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { useTagColors } from "@/lib/tags";

import {
  FILTER_DATE_FIELDS,
  FILTER_KINDS,
  FILTER_LAST_DAYS,
  FILTER_STATES,
  FILTER_STATUSES,
  toggled,
  withLastDays,
} from "./filterModel";

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
  /** Off for a list of a single kind, such as the task list. */
  showKinds?: boolean;
}

/** Chooses what the list shows. Values in a row are alternatives; rows narrow each other. */
export function FilterBar({ filter, tags, onChange, showKinds = true }: FilterBarProps) {
  const { t } = useTranslation();
  const api = useApi();
  // F3: only friends can be chosen. Shares its cache with the friends screen.
  const friends = useQuery({ queryKey: ["friends", "list"], queryFn: () => api.friends.list() });
  // F1, T5.1.7: only the user's own teams; shares its cache with tag colors.
  const teams = useQuery({ queryKey: ["teams"], queryFn: api.teams.list });
  const colorOf = useTagColors(tags);
  const state = filter.state ?? "active";
  const date = filter.date;
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
      {showKinds ? (
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
      ) : null}
      <Row label={t("views.filter.status")}>
        {FILTER_STATUSES.map((status) => (
          <TagChip
            key={status}
            label={t(`views.statuses.${status}`)}
            selected={filter.statuses?.includes(status) ?? false}
            onPress={() => {
              onChange({ ...filter, statuses: toggled(filter.statuses, status) });
            }}
          />
        ))}
      </Row>
      <Row label={t("views.filter.date")}>
        {FILTER_LAST_DAYS.map((days) => (
          <TagChip
            key={days}
            label={t("views.lastDays", { count: days })}
            selected={date?.last_days === days}
            onPress={() => {
              const next = withLastDays(date, days);
              const { date: _old, ...rest } = filter;
              onChange(next === undefined ? rest : { ...rest, date: next });
            }}
          />
        ))}
      </Row>
      {date !== undefined ? (
        <Row label={t("views.filter.dateField")}>
          {FILTER_DATE_FIELDS.map((field) => (
            <TagChip
              key={field}
              label={t(`views.dateFields.${field}`)}
              selected={date.field === field}
              onPress={() => {
                onChange({ ...filter, date: { ...date, field } });
              }}
            />
          ))}
        </Row>
      ) : null}
      {tags.length > 0 ? (
        <Row label={t("views.filter.tag")}>
          {tags.map((tag) => (
            <TagChip
              key={tag.id}
              label={tag.name}
              teamColor={colorOf(tag)}
              selected={filter.tag_ids?.includes(tag.id) ?? false}
              onPress={() => {
                onChange({ ...filter, tag_ids: toggled(filter.tag_ids, tag.id) });
              }}
            />
          ))}
        </Row>
      ) : null}
      {teams.data !== undefined && teams.data.length > 0 ? (
        <Row label={t("views.filter.team")}>
          {teams.data.map((team) => (
            <TagChip
              key={team.id}
              label={team.name}
              teamColor={team.color}
              selected={filter.team_ids?.includes(team.id) ?? false}
              onPress={() => {
                onChange({ ...filter, team_ids: toggled(filter.team_ids, team.id) });
              }}
            />
          ))}
        </Row>
      ) : null}
      {friends.data !== undefined && friends.data.length > 0 ? (
        <Row label={t("views.filter.person")}>
          {friends.data.map((friend) => (
            <TagChip
              key={friend.user_id}
              label={friend.display_name}
              selected={filter.people?.includes(friend.user_id) ?? false}
              onPress={() => {
                onChange({ ...filter, people: toggled(filter.people, friend.user_id) });
              }}
            />
          ))}
        </Row>
      ) : null}
    </View>
  );
}
