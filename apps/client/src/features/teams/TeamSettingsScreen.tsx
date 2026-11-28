import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type {
  Notebook,
  Tag,
  TeamDetail,
  TeamMatrix,
  TeamRole,
  TeamVisibilityMode,
} from "@secondbrain/api-types";
import { Button, Card, EmptyState, Input, TagChip, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";

import {
  canManage,
  ITEM_PERMISSIONS,
  mayRemove,
  ROLES,
  TEAM_PERMISSIONS,
  TEAMS_KEY,
  toggledIn,
} from "./teamModel";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card className="gap-3">
      <Text variant="heading">{title}</Text>
      {children}
    </Card>
  );
}

function Failed({ when }: { when: boolean }) {
  const { t } = useTranslation();
  return when ? (
    <Text tone="danger" accessibilityRole="alert">
      {t("teams.failed")}
    </Text>
  ) : null;
}

/** Members, their roles and invitations (TK2, TK3, TK5, TK7). */
function Members({ detail, refresh }: { detail: TeamDetail; refresh: () => Promise<void> }) {
  const { t } = useTranslation();
  const api = useApi();
  const id = detail.team.id;
  const mine = detail.team.role;
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<TeamRole>("member");
  const [sent, setSent] = useState(false);
  const invite = useIdempotentMutation<void, void>(
    (_none, idempotencyKey) =>
      api.teams.inviteByEmail(id, { email: email.trim(), role }, { idempotencyKey }),
    {
      onMutate: () => {
        setSent(false);
      },
      onSuccess: async () => {
        setEmail("");
        setSent(true);
        await refresh();
      },
    },
  );
  const setMemberRole = useIdempotentMutation<void, { userId: string; role: TeamRole }>(
    (change, idempotencyKey) =>
      api.teams.setRole(id, change.userId, { role: change.role }, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const remove = useIdempotentMutation<void, string>(
    (userId, idempotencyKey) => api.teams.removeMember(id, userId, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const lastAdmin = setMemberRole.error instanceof ApiError && setMemberRole.error.status === 422;
  const invitable = mine === "admin" ? ROLES : ROLES.filter((r) => r !== "admin");
  return (
    <Section title={t("teams.members")}>
      {detail.members.map((member) => (
        <View key={member.user_id} className="gap-2 border-t border-border pt-2">
          <View className="flex-row flex-wrap items-center gap-2">
            <Text className="font-semibold">{member.display_name}</Text>
            <Text tone="muted">
              {member.active ? t(`teams.roles.${member.role}`) : t("teams.invited")}
            </Text>
          </View>
          {member.active ? (
            <View className="flex-row flex-wrap items-center gap-2">
              {mine === "admin"
                ? ROLES.map((value) => (
                    <TagChip
                      key={value}
                      label={t(`teams.roles.${value}`)}
                      selected={member.role === value}
                      onPress={
                        member.role === value || setMemberRole.isPending
                          ? undefined
                          : () => {
                              setMemberRole.mutate({ userId: member.user_id, role: value });
                            }
                      }
                    />
                  ))
                : null}
              {mayRemove(mine, member.role) ? (
                <Button
                  title={t("teams.remove")}
                  variant="ghost"
                  size="sm"
                  accessibilityLabel={t("teams.removeLabel", { name: member.display_name })}
                  disabled={remove.isPending}
                  onPress={() => {
                    remove.mutate(member.user_id);
                  }}
                />
              ) : null}
            </View>
          ) : null}
        </View>
      ))}
      {lastAdmin ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("teams.lastAdmin")}
        </Text>
      ) : (
        <Failed when={setMemberRole.isError || remove.isError} />
      )}
      <View className="gap-2 border-t border-border pt-3">
        <Input
          label={t("teams.inviteEmail")}
          value={email}
          onChangeText={setEmail}
          autoCapitalize="none"
          keyboardType="email-address"
        />
        <View className="flex-row flex-wrap gap-2">
          {invitable.map((value) => (
            <TagChip
              key={value}
              label={t(`teams.roles.${value}`)}
              selected={role === value}
              onPress={() => {
                setRole(value);
              }}
            />
          ))}
        </View>
        <View className="self-start">
          <Button
            title={t("teams.invite")}
            variant="secondary"
            disabled={email.trim() === ""}
            loading={invite.isPending}
            onPress={() => {
              invite.mutate();
            }}
          />
        </View>
        {sent ? <Text tone="muted">{t("teams.inviteSent")}</Text> : null}
        <Failed when={invite.isError} />
      </View>
    </Section>
  );
}

/** The role × permission matrix: admins edit it, managers read it (TK3). */
function Permissions({ teamId, isAdmin }: { teamId: string; isAdmin: boolean }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const key = [...TEAMS_KEY, teamId, "matrix"];
  const matrix = useQuery({ queryKey: key, queryFn: () => api.teams.matrix(teamId) });
  const [draft, setDraft] = useState<TeamMatrix | null>(null);
  const save = useIdempotentMutation<TeamMatrix, TeamMatrix>(
    (body, idempotencyKey) => api.teams.setMatrix(teamId, body, { idempotencyKey }),
    {
      onSuccess: async (saved) => {
        client.setQueryData(key, saved);
        setDraft(null);
        await client.invalidateQueries({ queryKey: ["items"] });
      },
    },
  );
  if (matrix.data === undefined) {
    return (
      <Section title={t("teams.permissions")}>
        <Text tone={matrix.isError ? "danger" : "muted"}>
          {matrix.isError ? t("teams.failed") : t("common.loading")}
        </Text>
      </Section>
    );
  }
  const current = draft ?? matrix.data;
  const edit = (role: TeamRole, change: (row: TeamMatrix["roles"][number]) => void) => {
    const next: TeamMatrix = {
      roles: current.roles.map((row) => {
        if (row.role !== role) return row;
        const copy = { ...row, item: [...row.item], team: [...row.team] };
        change(copy);
        return copy;
      }),
    };
    setDraft(next);
  };
  return (
    <Section title={t("teams.permissions")}>
      {isAdmin ? null : <Text tone="muted">{t("teams.readOnly")}</Text>}
      {current.roles
        .filter((row) => row.role !== "admin")
        .map((row) => (
          <View key={row.role} className="gap-2 border-t border-border pt-2">
            <Text className="font-semibold">{t(`teams.roles.${row.role}`)}</Text>
            <Text variant="caption" tone="muted">
              {t("teams.onItems")}
            </Text>
            <View className="flex-row flex-wrap gap-2">
              {ITEM_PERMISSIONS.map((permission) => (
                <TagChip
                  key={permission}
                  label={t(`teams.item.${permission}`)}
                  selected={row.item.includes(permission)}
                  onPress={
                    isAdmin
                      ? () => {
                          edit(row.role, (copy) => {
                            copy.item = toggledIn(ITEM_PERMISSIONS, copy.item, permission);
                          });
                        }
                      : undefined
                  }
                />
              ))}
            </View>
            <Text variant="caption" tone="muted">
              {t("teams.inTeam")}
            </Text>
            <View className="flex-row flex-wrap gap-2">
              {TEAM_PERMISSIONS.map((permission) => (
                <TagChip
                  key={permission}
                  label={t(`teams.team.${permission}`)}
                  selected={row.team.includes(permission)}
                  onPress={
                    isAdmin
                      ? () => {
                          edit(row.role, (copy) => {
                            copy.team = toggledIn(TEAM_PERMISSIONS, copy.team, permission);
                          });
                        }
                      : undefined
                  }
                />
              ))}
            </View>
          </View>
        ))}
      {isAdmin && draft !== null ? (
        <View className="flex-row flex-wrap gap-2">
          <Button
            title={t("teams.save")}
            loading={save.isPending}
            onPress={() => {
              save.mutate(draft);
            }}
          />
          <Button
            title={t("teams.cancel")}
            variant="ghost"
            onPress={() => {
              setDraft(null);
            }}
          />
        </View>
      ) : null}
      <Failed when={save.isError} />
    </Section>
  );
}

/** The team's tags, in the team's color (E3, T5.1.4). */
function Tags({ teamId, color }: { teamId: string; color: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const tags = useQuery({ queryKey: ["tags"], queryFn: api.tags.list });
  const [name, setName] = useState("");
  const refresh = () => client.invalidateQueries({ queryKey: ["tags"] });
  const create = useIdempotentMutation<Tag, void>(
    (_none, idempotencyKey) =>
      api.tags.createForTeam(teamId, { name: name.trim() }, { idempotencyKey }),
    {
      onSuccess: async () => {
        setName("");
        await refresh();
      },
    },
  );
  const remove = useIdempotentMutation<void, string>(
    (id, idempotencyKey) => api.tags.remove(id, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const own = (tags.data ?? []).filter((tag) => tag.team_id === teamId);
  return (
    <Section title={t("teams.tags")}>
      {own.length === 0 ? (
        <Text tone="muted">{t("teams.noTags")}</Text>
      ) : (
        <View className="flex-row flex-wrap gap-2">
          {own.map((tag) => (
            <TagChip
              key={tag.id}
              label={tag.name}
              teamColor={color}
              removeLabel={t("teams.deleteTag", { name: tag.name })}
              onRemove={
                remove.isPending
                  ? undefined
                  : () => {
                      remove.mutate(tag.id);
                    }
              }
            />
          ))}
        </View>
      )}
      <Input label={t("teams.newTag")} value={name} onChangeText={setName} maxLength={50} />
      <View className="self-start">
        <Button
          title={t("teams.addTag")}
          variant="secondary"
          disabled={name.trim() === ""}
          loading={create.isPending}
          onPress={() => {
            create.mutate();
          }}
        />
      </View>
      <Failed when={create.isError || remove.isError} />
    </Section>
  );
}

/** The team's notebooks; each is for the whole team or the chosen people (Y2, T5.1.5). */
function Notebooks({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const notebooks = useQuery({ queryKey: ["notebooks"], queryFn: () => api.notebooks.list() });
  const [title, setTitle] = useState("");
  const [visibility, setVisibility] = useState<TeamVisibilityMode>("all");
  const create = useIdempotentMutation<Notebook, void>(
    (_none, idempotencyKey) =>
      api.notebooks.createForTeam(teamId, { title: title.trim(), visibility }, { idempotencyKey }),
    {
      onSuccess: async () => {
        setTitle("");
        await client.invalidateQueries({ queryKey: ["notebooks"] });
      },
    },
  );
  const own = (notebooks.data ?? []).filter(
    (notebook) => notebook.item.owner.type === "team" && notebook.item.owner.team_id === teamId,
  );
  const modes: readonly TeamVisibilityMode[] = ["all", "assignees"];
  return (
    <Section title={t("teams.notebooks")}>
      {own.length === 0 ? (
        <Text tone="muted">{t("teams.noNotebooks")}</Text>
      ) : (
        own.map((notebook) => (
          <View key={notebook.item.id} className="flex-row flex-wrap items-center gap-2">
            <Text>{notebook.item.title}</Text>
            <Text tone="muted">
              {t(`teams.visibility.${notebook.item.team_visibility ?? "all"}`)}
            </Text>
          </View>
        ))
      )}
      <Input label={t("teams.newNotebook")} value={title} onChangeText={setTitle} maxLength={500} />
      <View className="flex-row flex-wrap gap-2">
        {modes.map((mode) => (
          <TagChip
            key={mode}
            label={t(`teams.visibility.${mode}`)}
            selected={visibility === mode}
            onPress={() => {
              setVisibility(mode);
            }}
          />
        ))}
      </View>
      <View className="self-start">
        <Button
          title={t("teams.createNotebook")}
          variant="secondary"
          disabled={title.trim() === ""}
          loading={create.isPending}
          onPress={() => {
            create.mutate();
          }}
        />
      </View>
      <Failed when={create.isError} />
    </Section>
  );
}

/**
 * The team settings page (PRD TK5): members, roles, the permission matrix, tags and notebooks.
 * A settings screen, not a workspace; only admins and managers see it.
 */
export function TeamSettingsScreen({ id }: { id: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const detail = useQuery({
    queryKey: [...TEAMS_KEY, id, "detail"],
    queryFn: () => api.teams.get(id),
  });
  const refresh = async () => {
    await client.invalidateQueries({ queryKey: TEAMS_KEY });
  };
  if (detail.isPending) {
    return (
      <Screen title={t("teams.settings")}>
        <Text tone="muted">{t("common.loading")}</Text>
      </Screen>
    );
  }
  if (detail.isError || !canManage(detail.data.team.role)) {
    // Someone outside the team, or a member or guest: nothing to show (TK5, V9).
    return (
      <Screen title={t("teams.settings")}>
        <EmptyState title={t("teams.noAccess")} />
      </Screen>
    );
  }
  const team = detail.data.team;
  return (
    <Screen title={team.name}>
      <View className="flex-row flex-wrap items-center gap-2">
        <TagChip label={team.name} teamColor={team.color} />
        <Text tone="muted">{t(`teams.roles.${team.role}`)}</Text>
      </View>
      <Members detail={detail.data} refresh={refresh} />
      <Permissions teamId={team.id} isAdmin={team.role === "admin"} />
      <Tags teamId={team.id} color={team.color} />
      <Notebooks teamId={team.id} />
    </Screen>
  );
}
