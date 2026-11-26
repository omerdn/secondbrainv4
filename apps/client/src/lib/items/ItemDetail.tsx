import type { AccessExplanation, Item, ItemLink, Tag } from "@secondbrain/api-types";
import { Button, TagChip, Text } from "@secondbrain/ui";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { ItemCard } from "./ItemCard";
import { useStatusLine } from "./statusLines";
import { StatusLineText } from "./StatusLineText";
import { kindLabel, type RoleLabel } from "./labels";
import { WhyPanel } from "./WhyPanel";

/** A section's data: still loading, failed (with a retry), or ready. */
export type Loadable<T> =
  { status: "loading" } | { status: "error"; retry: () => void } | { status: "ready"; value: T };

export interface ItemDetailProps {
  item: Item;
  /** Whether the signed-in user owns the item personally. */
  ownedByMe: boolean;
  roles: Loadable<readonly RoleLabel[]>;
  /** Why the viewer has access (V11); omit to hide the panel. */
  explanation?: Loadable<AccessExplanation>;
  tags: Loadable<readonly Tag[]>;
  /** Only links whose other end the viewer can see; the server leaves out the rest (V9). */
  links: Loadable<readonly ItemLink[]>;
  onOpenItem: (id: string) => void;
  /** Opens the tag picker; omit when tags cannot be changed. */
  onEditTags?: () => void;
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <View className="gap-2">
      <Text variant="label" tone="muted">
        {title}
      </Text>
      {children}
    </View>
  );
}

function SectionBody<T>({
  data,
  empty,
  children,
}: {
  data: Loadable<readonly T[]>;
  empty: string;
  children: (value: readonly T[]) => ReactNode;
}) {
  const { t } = useTranslation();
  if (data.status === "loading") {
    return <Text tone="muted">{t("common.loading")}</Text>;
  }
  if (data.status === "error") {
    return (
      <View className="flex-row items-center gap-2">
        <Text tone="danger">{t("items.detail.loadFailed")}</Text>
        <Button title={t("items.detail.retry")} variant="ghost" size="sm" onPress={data.retry} />
      </View>
    );
  }
  return data.value.length === 0 ? <Text tone="muted">{empty}</Text> : children(data.value);
}

/**
 * The skeleton every item kind's detail screen builds on: kind, state, description, owner, the
 * viewer's roles, tags and links. It only displays what it is given.
 */
export function ItemDetail({
  item,
  ownedByMe,
  roles,
  explanation,
  tags,
  links,
  onOpenItem,
  onEditTags,
}: ItemDetailProps) {
  const { t } = useTranslation();
  const line = useStatusLine(item.id);
  const owner =
    item.owner.type === "team"
      ? t("items.owner.team")
      : ownedByMe
        ? t("items.owner.you")
        : t("items.owner.someone");
  return (
    <View className="gap-6">
      <View className="flex-row flex-wrap items-center gap-2">
        <Text variant="label" tone="muted">
          {t(`items.kinds.${kindLabel(item.kind)}`)}
        </Text>
        {item.archived_at !== null ? (
          <Text variant="label" tone="muted">
            · {t("items.state.archived")}
          </Text>
        ) : null}
        {item.deleted_at !== null ? (
          <Text variant="label" tone="danger">
            · {t("items.state.trash")}
          </Text>
        ) : null}
      </View>
      {line !== null ? <StatusLineText line={line} /> : null}
      {item.description !== null ? <Text>{item.description}</Text> : null}
      <Section title={t("items.detail.owner")}>
        <Text>{owner}</Text>
      </Section>
      <Section title={t("items.detail.roles")}>
        <SectionBody data={roles} empty={t("items.detail.noRoles")}>
          {(value) => <Text>{value.map((role) => t(`items.roles.${role}`)).join(", ")}</Text>}
        </SectionBody>
        {explanation !== undefined ? (
          <WhyPanel
            explanation={explanation}
            titles={
              new Map(
                links.status === "ready"
                  ? links.value.map((link) => [link.item.id, link.item.title])
                  : [],
              )
            }
            onOpenItem={onOpenItem}
          />
        ) : null}
      </Section>
      <Section title={t("items.detail.tags")}>
        <SectionBody data={tags} empty={t("items.detail.noTags")}>
          {(value) => (
            <View className="flex-row flex-wrap gap-1">
              {value.map((tag) => (
                <TagChip key={tag.id} label={tag.name} />
              ))}
            </View>
          )}
        </SectionBody>
        {onEditTags !== undefined ? (
          <View className="self-start">
            <Button title={t("tags.edit")} variant="secondary" size="sm" onPress={onEditTags} />
          </View>
        ) : null}
      </Section>
      <Section title={t("items.detail.links")}>
        <SectionBody data={links} empty={t("items.detail.noLinks")}>
          {(value) => (
            <View className="gap-2">
              {value.map((link) => (
                <ItemCard
                  key={link.id}
                  item={link.item}
                  onPress={() => {
                    onOpenItem(link.item.id);
                  }}
                />
              ))}
            </View>
          )}
        </SectionBody>
      </Section>
    </View>
  );
}
