import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Team } from "@secondbrain/api-types";
import { Button, Card, EmptyState, Input, TagChip, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";

import { canManage, TEAM_COLORS, TEAMS_KEY } from "./teamModel";

/** A team in the list: its color, the caller's role, settings for those who run it, leaving. */
function TeamRow({ team, onLeft }: { team: Team; onLeft: () => Promise<void> }) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const [confirming, setConfirming] = useState(false);
  const leave = useIdempotentMutation<void, void>(
    (_none, idempotencyKey) => api.teams.leave(team.id, { idempotencyKey }),
    { onSuccess: onLeft },
  );
  const lastAdmin = leave.error instanceof ApiError && leave.error.status === 422;
  return (
    <View className="gap-2 border-t border-border pt-2">
      <View className="flex-row flex-wrap items-center gap-2">
        <TagChip label={team.name} teamColor={team.color} />
        <Text tone="muted">{t(`teams.roles.${team.role}`)}</Text>
      </View>
      {confirming ? (
        <View className="gap-2">
          <Text>{t("teams.confirmLeave")}</Text>
          <View className="flex-row flex-wrap gap-2">
            <Button
              title={t("teams.leaveYes")}
              variant="danger"
              size="sm"
              loading={leave.isPending}
              onPress={() => {
                leave.mutate();
              }}
            />
            <Button
              title={t("teams.cancel")}
              variant="ghost"
              size="sm"
              onPress={() => {
                setConfirming(false);
                leave.reset();
              }}
            />
          </View>
        </View>
      ) : (
        <View className="flex-row flex-wrap gap-2">
          {canManage(team.role) ? (
            <Button
              title={t("teams.settings")}
              variant="secondary"
              size="sm"
              accessibilityLabel={t("teams.settingsOf", { name: team.name })}
              onPress={() => {
                router.push(`/teams/${encodeURIComponent(team.id)}` as Href);
              }}
            />
          ) : null}
          <Button
            title={t("teams.leave")}
            variant="ghost"
            size="sm"
            accessibilityLabel={t("teams.leaveOf", { name: team.name })}
            onPress={() => {
              setConfirming(true);
            }}
          />
        </View>
      )}
      {leave.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {lastAdmin ? t("teams.lastAdmin") : t("teams.failed")}
        </Text>
      ) : null}
    </View>
  );
}

function CreateTeam({ onCreated }: { onCreated: () => Promise<void> }) {
  const { t } = useTranslation();
  const api = useApi();
  const [name, setName] = useState("");
  const [color, setColor] = useState<string>(TEAM_COLORS[0] ?? "#4f46e5");
  const create = useIdempotentMutation<Team, void>(
    (_none, idempotencyKey) => api.teams.create({ name: name.trim(), color }, { idempotencyKey }),
    {
      onSuccess: async () => {
        setName("");
        await onCreated();
      },
    },
  );
  return (
    <Card className="gap-3">
      <Text variant="heading">{t("teams.create")}</Text>
      <Input label={t("teams.name")} value={name} onChangeText={setName} maxLength={60} />
      <View className="gap-1">
        <Text variant="caption" tone="muted">
          {t("teams.color")}
        </Text>
        <View className="flex-row flex-wrap gap-2">
          {TEAM_COLORS.map((value) => (
            <TagChip
              key={value}
              label={value}
              teamColor={value}
              selected={value === color}
              onPress={() => {
                setColor(value);
              }}
            />
          ))}
        </View>
      </View>
      <View className="self-start">
        <Button
          title={t("teams.create")}
          disabled={name.trim() === ""}
          loading={create.isPending}
          onPress={() => {
            create.mutate();
          }}
        />
      </View>
      {create.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("teams.failed")}
        </Text>
      ) : null}
    </Card>
  );
}

/** The user's teams and team invitations; settings open for admins and managers (PRD TK1, TK2, TK5, TK7). */
export function TeamsScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const teams = useQuery({ queryKey: TEAMS_KEY, queryFn: api.teams.list });
  const invitations = useQuery({
    queryKey: [...TEAMS_KEY, "invitations"],
    queryFn: api.teams.invitations,
  });
  const refresh = async () => {
    await client.invalidateQueries({ queryKey: TEAMS_KEY });
  };
  const join = useIdempotentMutation<Team, string>(
    (id, idempotencyKey) => api.teams.join(id, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const decline = useIdempotentMutation<void, string>(
    (id, idempotencyKey) => api.teams.decline(id, { idempotencyKey }),
    { onSuccess: refresh },
  );

  return (
    <Screen title={t("nav.teams")}>
      {invitations.data !== undefined && invitations.data.length > 0 ? (
        <Card className="gap-2">
          <Text variant="heading">{t("teams.invitations")}</Text>
          {invitations.data.map((invitation) => (
            <View key={invitation.team_id} className="gap-2 border-t border-border pt-2">
              <Text>
                {t("teams.invitedTo", {
                  team: invitation.team_name,
                  role: t(`teams.roles.${invitation.role}`),
                })}
              </Text>
              <View className="flex-row flex-wrap gap-2">
                <Button
                  title={t("teams.join")}
                  size="sm"
                  disabled={join.isPending || decline.isPending}
                  onPress={() => {
                    join.mutate(invitation.team_id);
                  }}
                />
                <Button
                  title={t("teams.decline")}
                  variant="ghost"
                  size="sm"
                  disabled={join.isPending || decline.isPending}
                  onPress={() => {
                    decline.mutate(invitation.team_id);
                  }}
                />
              </View>
            </View>
          ))}
        </Card>
      ) : null}
      {teams.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : teams.isError ? (
        <EmptyState
          title={t("teams.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void teams.refetch();
            },
          }}
        />
      ) : teams.data.length === 0 ? (
        <EmptyState title={t("teams.empty")} />
      ) : (
        <Card className="gap-2">
          <Text variant="heading">{t("teams.mine")}</Text>
          {teams.data.map((team) => (
            <TeamRow key={team.id} team={team} onLeft={refresh} />
          ))}
        </Card>
      )}
      <CreateTeam onCreated={refresh} />
    </Screen>
  );
}
