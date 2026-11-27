import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Friend, InvitationRole, Item } from "@secondbrain/api-types";
import { Button, Sheet, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { ShareConfirmDialog } from "@/lib/share";

import { invitable, rolesFor } from "./members";

interface Choice {
  friend: Friend;
  role: InvitationRole;
}

/**
 * An item's members (PRD N2, V5): roles, the owner's pending invitations, inviting a friend after
 * the access preview (V7, G5), removing a member, and leaving.
 */
export function MembersSection({ item, me }: { item: Item; me: string | null }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const key = ["item", item.id, "members"] as const;
  const members = useQuery({ queryKey: key, queryFn: () => api.items.members(item.id) });
  const roles = rolesFor(item.kind);
  const canInvite =
    members.data?.can_manage === true && item.deleted_at === null && roles.length > 0;
  const friends = useQuery({
    queryKey: ["friends", "list"],
    queryFn: () => api.friends.list(),
    enabled: canInvite,
  });
  const [picking, setPicking] = useState(false);
  const [role, setRole] = useState<InvitationRole | null>(null);
  const [choice, setChoice] = useState<Choice | null>(null);
  const refresh = () =>
    Promise.all([
      client.invalidateQueries({ queryKey: key }),
      client.invalidateQueries({ queryKey: ["item", item.id, "access"] }),
    ]);

  const invite = useIdempotentMutation<unknown, Choice>(
    ({ friend, role: chosen }, idempotencyKey) =>
      api.items.invite(item.id, { to_user: friend.user_id, role: chosen }, { idempotencyKey }),
    {
      onSuccess: async () => {
        setChoice(null);
        setPicking(false);
        await refresh();
      },
    },
  );
  const remove = useIdempotentMutation<unknown, string>(
    (userId, idempotencyKey) => api.items.removeMember(item.id, userId, { idempotencyKey }),
    { onSuccess: refresh },
  );

  if (!members.isSuccess) {
    return null;
  }
  const chosenRole = role ?? roles[0] ?? null;
  const candidates = friends.isSuccess ? invitable(friends.data, members.data) : [];
  return (
    <View className="gap-2">
      <Text variant="label" tone="muted">
        {t("members.title")}
      </Text>
      {members.data.members.map((member) => {
        const isOwner = member.roles.includes("owner");
        const isMe = member.user_id === me;
        const label = member.roles.map((r) => t(`items.roles.${r}`)).join(", ");
        return (
          <View key={member.user_id} className="flex-row items-center justify-between gap-2">
            <View className="flex-1">
              <Text numberOfLines={1}>
                {isMe ? t("members.you", { name: member.display_name }) : member.display_name}
              </Text>
              <Text variant="caption" tone="muted">
                {label}
              </Text>
            </View>
            {!isOwner && (members.data.can_manage || isMe) ? (
              <Button
                title={isMe ? t("members.leave") : t("members.remove")}
                accessibilityLabel={
                  isMe ? t("members.leave") : t("members.removeOf", { name: member.display_name })
                }
                variant="ghost"
                size="sm"
                disabled={remove.isPending}
                onPress={() => {
                  remove.mutate(member.user_id);
                }}
              />
            ) : null}
          </View>
        );
      })}
      {members.data.pending.map((pending) => (
        <View key={pending.invitation_id} className="gap-0.5">
          <Text numberOfLines={1}>{pending.display_name}</Text>
          <Text variant="caption" tone="muted">
            {t("members.pending", { role: t(`items.roles.${pending.role}`) })}
          </Text>
        </View>
      ))}
      {remove.isError ? <Text tone="danger">{t("members.failed")}</Text> : null}
      {canInvite ? (
        <View className="self-start">
          <Button
            title={t("members.add")}
            variant="secondary"
            size="sm"
            onPress={() => {
              setPicking(true);
            }}
          />
        </View>
      ) : null}

      <Sheet
        visible={picking && choice === null}
        onClose={() => {
          setPicking(false);
        }}
        title={t("members.add")}
        closeLabel={t("tags.close")}
      >
        <View className="gap-3">
          <Text variant="label">{t("members.role")}</Text>
          <View className="flex-row flex-wrap gap-2">
            {roles.map((r) => (
              <Pressable
                key={r}
                accessibilityRole="radio"
                accessibilityState={{ selected: chosenRole === r }}
                accessibilityLabel={t(`items.roles.${r}`)}
                onPress={() => {
                  setRole(r);
                }}
                className={`rounded-full border px-3 py-1.5 ${
                  chosenRole === r ? "border-primary bg-primary/10" : "border-border"
                }`}
              >
                <Text>{t(`items.roles.${r}`)}</Text>
              </Pressable>
            ))}
          </View>
          <Text variant="label">{t("members.friend")}</Text>
          {friends.isPending ? (
            <Text tone="muted">{t("common.loading")}</Text>
          ) : candidates.length === 0 ? (
            <Text tone="muted">{t("members.noFriends")}</Text>
          ) : (
            candidates.map((friend) => (
              <Pressable
                key={friend.user_id}
                accessibilityRole="button"
                accessibilityLabel={t("members.inviteOf", { name: friend.display_name })}
                onPress={() => {
                  if (chosenRole !== null) setChoice({ friend, role: chosenRole });
                }}
                className="rounded-md border border-border p-3"
              >
                <Text>{friend.display_name}</Text>
              </Pressable>
            ))
          )}
        </View>
      </Sheet>

      <ShareConfirmDialog
        request={
          choice === null
            ? null
            : {
                action: "add_member",
                item_id: item.id,
                user_id: choice.friend.user_id,
                role: choice.role,
              }
        }
        knownNames={choice === null ? {} : { [choice.friend.user_id]: choice.friend.display_name }}
        confirmLabel={t("members.invite")}
        busy={invite.isPending}
        onConfirm={() => {
          if (choice !== null) invite.mutate(choice);
        }}
        onCancel={() => {
          setChoice(null);
        }}
      />
      {invite.isError ? <Text tone="danger">{t("members.failed")}</Text> : null}
    </View>
  );
}
