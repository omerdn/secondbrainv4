import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Activity, ActivityReaction } from "@secondbrain/api-types";
import { Card, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { useSession } from "@/features/auth";

import { friendCompletions, REACTIONS, reactionAction, reactionCount } from "./social";

/** Activity events read for the list: the latest ones are enough. */
const FEED_LIMIT = 50;
const SHOWN = 10;

function Reactions({ event }: { event: Activity }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const change = useIdempotentMutation<ActivityReaction[], string>(
    (emoji, idempotencyKey) =>
      reactionAction(event.reactions, emoji) === "unreact"
        ? api.activity.unreact(event.id, { idempotencyKey })
        : api.activity.react(event.id, emoji, { idempotencyKey }),
    {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: ["activity", "feed"] });
      },
    },
  );
  const mine = (emoji: string) => event.reactions.some((r) => r.emoji === emoji && r.mine);
  return (
    <View className="gap-1">
      <View className="flex-row flex-wrap gap-2">
        {REACTIONS.map((emoji) => {
          const count = reactionCount(event.reactions, emoji);
          return (
            <Pressable
              key={emoji}
              accessibilityRole="button"
              accessibilityState={{ selected: mine(emoji) }}
              accessibilityLabel={t("social.reactWith", { emoji, title: event.item_title })}
              onPress={() => {
                if (!change.isPending) change.mutate(emoji);
              }}
              className={
                mine(emoji)
                  ? "rounded-full border border-primary bg-primary/10 px-2 py-0.5"
                  : "rounded-full border border-border px-2 py-0.5"
              }
            >
              <Text className="text-sm">{count > 0 ? `${emoji} ${String(count)}` : emoji}</Text>
            </Pressable>
          );
        })}
      </View>
      {change.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("social.reactFailed")}
        </Text>
      ) : null}
    </View>
  );
}

/** Friends' recent completions with emoji reactions (PRD AK4). Hidden while there are none. */
export function FriendsActivity({
  friends,
}: {
  friends: readonly { user_id: string; display_name: string }[];
}) {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const { state } = useSession();
  const me = state.status === "signedIn" ? state.user.id : null;
  const feed = useQuery({
    queryKey: ["activity", "feed"],
    queryFn: () => api.activity.feed({ limit: FEED_LIMIT }),
    enabled: friends.length > 0,
  });
  const names = new Map(friends.map((f) => [f.user_id, f.display_name]));
  const events = friendCompletions(feed.data?.items ?? [], new Set(names.keys()), me).slice(
    0,
    SHOWN,
  );
  if (events.length === 0) return null;
  return (
    <Card className="gap-3">
      <Text variant="heading">{t("social.friendsActivity")}</Text>
      {events.map((event) => (
        <View key={event.id} className="gap-2 border-t border-border pt-2">
          <Text>
            {t(event.kind === "recorded" ? "social.recorded" : "social.completed", {
              name: names.get(event.actor_id ?? "") ?? "",
              title: event.item_title,
            })}
          </Text>
          <Text variant="caption" tone="muted">
            {new Date(event.created_at).toLocaleString(i18n.language, {
              dateStyle: "medium",
              timeStyle: "short",
            })}
          </Text>
          <Reactions event={event} />
        </View>
      ))}
    </Card>
  );
}
