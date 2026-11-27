import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Friend } from "@secondbrain/api-types";
import { Button, Card, Input, Sheet, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { Screen } from "@/features/shell";

import { FriendsActivity } from "./FriendsActivity";
import { FRIENDS_KEY } from "./useIncomingCount";

function Section({
  title,
  empty,
  children,
}: {
  title: string;
  empty?: string;
  children: ReactNode[];
}) {
  if (empty === undefined && children.length === 0) return null;
  return (
    <Card className="gap-2">
      <Text variant="heading">{title}</Text>
      {children.length === 0 ? <Text tone="muted">{empty}</Text> : children}
    </Card>
  );
}

function formatDay(iso: string, locale: string): string {
  return new Date(iso).toLocaleDateString(locale, { dateStyle: "medium" });
}

/** Friends, requests and blocks (PRD AK1, AK2). */
export function FriendsScreen() {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const friends = useQuery({
    queryKey: [...FRIENDS_KEY, "list"],
    queryFn: () => api.friends.list(),
  });
  const incoming = useQuery({
    queryKey: [...FRIENDS_KEY, "incoming"],
    queryFn: () => api.friends.incoming(),
  });
  const outgoing = useQuery({
    queryKey: [...FRIENDS_KEY, "outgoing"],
    queryFn: () => api.friends.outgoing(),
  });
  const blocked = useQuery({
    queryKey: [...FRIENDS_KEY, "blocked"],
    queryFn: () => api.friends.blocked(),
  });
  const refresh = () => client.invalidateQueries({ queryKey: FRIENDS_KEY });
  const [profile, setProfile] = useState<Friend | null>(null);

  const accept = useIdempotentMutation<Friend, string>(
    (id, idempotencyKey) => api.friends.accept(id, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const decline = useIdempotentMutation<unknown, string>(
    (id, idempotencyKey) => api.friends.decline(id, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const cancel = useIdempotentMutation<unknown, string>(
    (id, idempotencyKey) => api.friends.cancel(id, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const block = useIdempotentMutation<unknown, string>(
    (userId, idempotencyKey) => api.friends.block(userId, { idempotencyKey }),
    {
      onSuccess: async () => {
        setProfile(null);
        await refresh();
      },
    },
  );
  const unblock = useIdempotentMutation<unknown, string>(
    (userId, idempotencyKey) => api.friends.unblock(userId, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const unfriend = useIdempotentMutation<unknown, string>(
    (userId, idempotencyKey) => api.friends.unfriend(userId, { idempotencyKey }),
    {
      onSuccess: async () => {
        setProfile(null);
        await refresh();
      },
    },
  );
  const busy =
    accept.isPending ||
    decline.isPending ||
    cancel.isPending ||
    block.isPending ||
    unblock.isPending ||
    unfriend.isPending;
  const failed =
    accept.isError ||
    decline.isError ||
    cancel.isError ||
    block.isError ||
    unblock.isError ||
    unfriend.isError;

  return (
    <Screen title={t("nav.friends")}>
      <AddFriend onSent={refresh} />
      {failed ? <Text tone="danger">{t("friends.failed")}</Text> : null}
      <FriendsActivity friends={friends.data ?? []} />

      <Section title={t("friends.incoming")}>
        {(incoming.data ?? []).map((request) => (
          <View key={request.id} className="gap-2 border-t border-border pt-2">
            <Text>{t("friends.wantsToBeFriends", { name: request.from_name })}</Text>
            <View className="flex-row flex-wrap gap-2">
              <Button
                title={t("friends.accept")}
                accessibilityLabel={t("friends.acceptOf", { name: request.from_name })}
                size="sm"
                disabled={busy}
                onPress={() => {
                  accept.mutate(request.id);
                }}
              />
              <Button
                title={t("friends.decline")}
                accessibilityLabel={t("friends.declineOf", { name: request.from_name })}
                variant="secondary"
                size="sm"
                disabled={busy}
                onPress={() => {
                  decline.mutate(request.id);
                }}
              />
              <Button
                title={t("friends.block")}
                accessibilityLabel={t("friends.blockOf", { name: request.from_name })}
                variant="ghost"
                size="sm"
                disabled={busy}
                onPress={() => {
                  block.mutate(request.from_user);
                }}
              />
            </View>
          </View>
        ))}
      </Section>

      <Section title={t("friends.title")} empty={t("friends.none")}>
        {(friends.data ?? []).map((friend) => (
          <Pressable
            key={friend.user_id}
            accessibilityRole="button"
            accessibilityLabel={t("friends.openProfile", { name: friend.display_name })}
            onPress={() => {
              setProfile(friend);
            }}
            className="gap-0.5 border-t border-border py-2"
          >
            <Text>{friend.display_name}</Text>
            <Text variant="caption" tone="muted">
              {t("friends.since", { date: formatDay(friend.since, i18n.language) })}
            </Text>
          </Pressable>
        ))}
      </Section>

      <Section title={t("friends.outgoing")}>
        {(outgoing.data ?? []).map((request) => (
          <View
            key={request.id}
            className="flex-row items-center justify-between gap-2 border-t border-border pt-2"
          >
            <View className="flex-1">
              <Text numberOfLines={1}>{request.to_email}</Text>
              <Text variant="caption" tone="muted">
                {t("friends.pending")}
              </Text>
            </View>
            <Button
              title={t("friends.cancel")}
              accessibilityLabel={t("friends.cancelTo", { email: request.to_email })}
              variant="ghost"
              size="sm"
              disabled={busy}
              onPress={() => {
                cancel.mutate(request.id);
              }}
            />
          </View>
        ))}
      </Section>

      <Section title={t("friends.blocked")}>
        {(blocked.data ?? []).map((user) => (
          <View
            key={user.user_id}
            className="flex-row items-center justify-between gap-2 border-t border-border pt-2"
          >
            <Text className="flex-1">{user.display_name}</Text>
            <Button
              title={t("friends.unblock")}
              accessibilityLabel={t("friends.unblockOf", { name: user.display_name })}
              variant="ghost"
              size="sm"
              disabled={busy}
              onPress={() => {
                unblock.mutate(user.user_id);
              }}
            />
          </View>
        ))}
      </Section>

      <Sheet
        visible={profile !== null}
        onClose={() => {
          setProfile(null);
        }}
        title={profile?.display_name ?? ""}
        closeLabel={t("tags.close")}
      >
        {profile === null ? null : (
          <View className="gap-3">
            <Text tone="muted">
              {t("friends.since", { date: formatDay(profile.since, i18n.language) })}
            </Text>
            <Text variant="caption" tone="muted">
              {t("friends.profileHint")}
            </Text>
            <NudgeButton friend={profile} />
            <View className="flex-row flex-wrap gap-2">
              <Button
                title={t("friends.unfriend")}
                variant="secondary"
                disabled={busy}
                onPress={() => {
                  unfriend.mutate(profile.user_id);
                }}
              />
              <Button
                title={t("friends.block")}
                variant="dangerOutline"
                disabled={busy}
                onPress={() => {
                  block.mutate(profile.user_id);
                }}
              />
            </View>
          </View>
        )}
      </Sheet>
    </Screen>
  );
}

/** Nudges a friend, at most once a day (PRD AK3). */
function NudgeButton({ friend }: { friend: Friend }) {
  const { t } = useTranslation();
  const api = useApi();
  const nudge = useIdempotentMutation<unknown, string>((userId, idempotencyKey) =>
    api.friends.nudge(userId, { idempotencyKey }),
  );
  const message = nudge.isSuccess
    ? t("social.nudged", { name: friend.display_name })
    : nudge.error === null
      ? null
      : nudge.error instanceof ApiError && nudge.error.status === 429
        ? t("social.nudgedToday")
        : t("friends.failed");
  return (
    <View className="gap-1">
      <View className="self-start">
        <Button
          title={t("social.nudge")}
          accessibilityLabel={t("social.nudgeOf", { name: friend.display_name })}
          loading={nudge.isPending}
          disabled={nudge.isSuccess}
          onPress={() => {
            nudge.mutate(friend.user_id);
          }}
        />
      </View>
      {message !== null ? (
        <Text tone={nudge.isSuccess ? "muted" : "danger"} accessibilityRole="alert">
          {message}
        </Text>
      ) : null}
    </View>
  );
}

/** Asks someone by e-mail; the answer never says whether the address has an account (AK2). */
function AddFriend({ onSent }: { onSent: () => Promise<void> }) {
  const { t } = useTranslation();
  const api = useApi();
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const send = useIdempotentMutation<unknown, string>(
    (address, idempotencyKey) => api.friends.send(address, { idempotencyKey }),
    {
      onSuccess: async () => {
        setEmail("");
        setSent(true);
        await onSent();
      },
    },
  );
  const status = send.error instanceof ApiError ? send.error.status : null;
  const error = !send.isError
    ? undefined
    : status === 429
      ? t("friends.errors.rateLimited")
      : status === 409
        ? t("friends.errors.conflict")
        : status === 422
          ? t("friends.errors.email")
          : t("friends.failed");
  return (
    <Card className="gap-3">
      <Text variant="heading">{t("friends.add")}</Text>
      <Input
        label={t("friends.email")}
        value={email}
        onChangeText={(value) => {
          setEmail(value);
          setSent(false);
        }}
        keyboardType="email-address"
        autoCapitalize="none"
        autoCorrect={false}
        error={error}
        onSubmitEditing={() => {
          if (email.trim() !== "") send.mutate(email);
        }}
      />
      {sent ? <Text tone="success">{t("friends.sent")}</Text> : null}
      <View className="self-start">
        <Button
          title={t("friends.send")}
          loading={send.isPending}
          disabled={send.isPending || email.trim() === ""}
          onPress={() => {
            send.mutate(email);
          }}
        />
      </View>
    </Card>
  );
}
