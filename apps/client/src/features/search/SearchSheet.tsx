import { useApi } from "@secondbrain/api-client/react";
import type { SearchHit } from "@secondbrain/api-types";
import { Input, Sheet, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { itemPath, kindLabel } from "@/lib/items";

/** How long typing pauses before a search goes out. */
export const SEARCH_DEBOUNCE_MS = 250;

/** Global search (PRD AR1): titles, descriptions, the caller's tags and page contents. */
export function SearchSheet({ visible, onClose }: { visible: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  useEffect(() => {
    const id = setTimeout(() => {
      setQuery(text.trim());
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      clearTimeout(id);
    };
  }, [text]);
  const results = useQuery({
    queryKey: ["search", query],
    enabled: visible && query !== "",
    queryFn: () => api.search.query(query),
  });
  const close = () => {
    setText("");
    setQuery("");
    onClose();
  };
  const open = (hit: SearchHit) => {
    const href = itemPath(hit.item.kind, hit.item.id) as Href;
    close();
    router.push(href);
  };
  return (
    <Sheet visible={visible} onClose={close} title={t("search.title")} closeLabel={t("tags.close")}>
      <View className="gap-3">
        <Input
          label={t("search.label")}
          value={text}
          onChangeText={setText}
          autoFocus
          autoCapitalize="none"
          autoCorrect={false}
          maxLength={100}
          returnKeyType="search"
          onSubmitEditing={() => {
            const first = results.data?.[0];
            if (first !== undefined) open(first);
          }}
        />
        {query === "" ? (
          <Text tone="muted">{t("search.hint")}</Text>
        ) : results.isPending ? (
          <Text tone="muted">{t("common.loading")}</Text>
        ) : results.isError ? (
          <Text tone="danger">{t("search.failed")}</Text>
        ) : results.data.length === 0 ? (
          <Text tone="muted">{t("search.none", { query })}</Text>
        ) : (
          results.data.map((hit) => <ResultRow key={hit.item.id} hit={hit} onPress={open} />)
        )}
      </View>
    </Sheet>
  );
}

function ResultRow({ hit, onPress }: { hit: SearchHit; onPress: (hit: SearchHit) => void }) {
  const { t } = useTranslation();
  const kind = t(`items.kinds.${kindLabel(hit.item.kind)}`);
  const where =
    hit.field === "tag" && hit.snippet !== null
      ? t("search.byTag", { tag: hit.snippet })
      : t(`search.fields.${hit.field}`);
  return (
    <Pressable
      accessibilityRole="link"
      accessibilityLabel={`${kind}: ${hit.item.title}`}
      onPress={() => {
        onPress(hit);
      }}
      className="gap-0.5 rounded-md border border-border p-3"
    >
      <Text variant="caption" tone="muted">
        {`${kind} · ${where}`}
        {hit.item.archived_at !== null ? ` · ${t("items.state.archived")}` : ""}
      </Text>
      <Text numberOfLines={2}>{hit.item.title}</Text>
      {hit.field === "content" && hit.snippet !== null ? (
        <Text variant="caption" tone="muted" numberOfLines={2}>
          {hit.snippet}
        </Text>
      ) : null}
    </Pressable>
  );
}
