import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Friend, WorkSession } from "@secondbrain/api-types";
import { Button, Card, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { useServerEvent } from "@/connectivity";

import { invitable, participantName } from "./model";

export const SHARED_KEY = ["sessions", "shared"] as const;
const INVITATIONS_KEY = ["sessions", "invitations"] as const;
const FRIENDS_KEY = ["friends", "list"] as const;

export const sharedHref = (id: string) => `/session?shared=${encodeURIComponent(id)}` as Href;

/** The caller's friends, shared with the friends screen's cache. */
export function useFriends() {
  const api = useApi();
  return useQuery({ queryKey: FRIENDS_KEY, queryFn: () => api.friends.list() });
}

/**
 * Shared sessions (PRD W6): invitations to join, the ones the caller is in, and starting one
 * with friends.
 */
export function SharedSessions() {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const router = useRouter();
  const friends = useFriends();
  const shared = useQuery({ queryKey: SHARED_KEY, queryFn: () => api.sessions.shared() });
  const invitations = useQuery({
    queryKey: INVITATIONS_KEY,
    queryFn: () => api.sessions.invitations(),
  });
  useServerEvent("session.invitation", () => {
    void client.invalidateQueries({ queryKey: INVITATIONS_KEY });
  });
  const go = (session: WorkSession) => {
    void client.invalidateQueries({ queryKey: ["sessions"] });
    router.push(sharedHref(session.id));
  };
  const start = useIdempotentMutation<WorkSession, undefined>(
    (_, idempotencyKey) => api.sessions.openShared({}, { idempotencyKey }),
    { onSuccess: go },
  );
  const join = useIdempotentMutation<WorkSession, string>(
    (id, idempotencyKey) => api.sessions.join(id, {}, { idempotencyKey }),
    { onSuccess: go },
  );
  const names = friends.data ?? [];
  const labels = { you: t("sessions.shared.you"), someone: t("sessions.shared.someone") };
  return (
    <Card className="gap-3">
      <Text variant="heading">{t("sessions.shared.title")}</Text>
      <Text tone="muted">{t("sessions.shared.hint")}</Text>
      {(invitations.data ?? []).map((invitation) => {
        const from = participantName(invitation.invited_by, null, names, labels);
        return (
          <View key={invitation.session_id} className="gap-2 border-t border-border pt-2">
            <Text>{t("sessions.shared.invitedBy", { name: from })}</Text>
            <View className="self-start">
              <Button
                title={t("sessions.shared.join")}
                accessibilityLabel={t("sessions.shared.joinOf", { name: from })}
                size="sm"
                loading={join.isPending}
                onPress={() => {
                  join.mutate(invitation.session_id);
                }}
              />
            </View>
          </View>
        );
      })}
      {(shared.data ?? []).map((session) => (
        <View
          key={session.id}
          className="flex-row items-center justify-between gap-2 border-t border-border pt-2"
        >
          <Text className="flex-1">
            {t("sessions.shared.with", {
              names: session.participants
                .map((id) => participantName(id, null, names, labels))
                .join(", "),
            })}
          </Text>
          <Button
            title={t("sessions.shared.open")}
            size="sm"
            variant="secondary"
            onPress={() => {
              router.push(sharedHref(session.id));
            }}
          />
        </View>
      ))}
      <View className="self-start">
        <Button
          title={t("sessions.shared.start")}
          variant="secondary"
          size="sm"
          loading={start.isPending}
          disabled={names.length === 0}
          onPress={() => {
            start.mutate(undefined);
          }}
        />
      </View>
      {names.length === 0 ? (
        <Text variant="caption" tone="muted">
          {t("sessions.shared.needFriends")}
        </Text>
      ) : null}
      {start.isError || join.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("sessions.saveFailed")}
        </Text>
      ) : null}
    </Card>
  );
}

/** Who is in a shared session, and inviting friends who are not (W6, V1). */
export function Participants({ session, me }: { session: WorkSession; me: string | null }) {
  const { t } = useTranslation();
  const api = useApi();
  const friends = useFriends();
  const invite = useIdempotentMutation<unknown, string>((userId, idempotencyKey) =>
    api.sessions.invite(session.id, userId, { idempotencyKey }),
  );
  const names: readonly Friend[] = friends.data ?? [];
  const candidates = invitable(names, session.participants);
  const labels = { you: t("sessions.shared.you"), someone: t("sessions.shared.someone") };
  return (
    <Card className="gap-2">
      <Text variant="heading">{t("sessions.shared.participants")}</Text>
      {session.participants.map((id) => (
        <Text key={id}>{participantName(id, me, names, labels)}</Text>
      ))}
      {candidates.length > 0 ? (
        <View className="gap-2 border-t border-border pt-2">
          <Text variant="caption" tone="muted">
            {t("sessions.shared.inviteHint")}
          </Text>
          <View className="flex-row flex-wrap gap-2">
            {candidates.map((friend) => (
              <Button
                key={friend.user_id}
                title={friend.display_name}
                accessibilityLabel={t("sessions.shared.inviteOf", { name: friend.display_name })}
                size="sm"
                variant="secondary"
                disabled={invite.isPending}
                onPress={() => {
                  invite.mutate(friend.user_id);
                }}
              />
            ))}
          </View>
        </View>
      ) : null}
      {invite.isSuccess ? (
        <Text tone="muted" accessibilityRole="alert">
          {t("sessions.shared.invited")}
        </Text>
      ) : invite.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("sessions.saveFailed")}
        </Text>
      ) : null}
    </Card>
  );
}
