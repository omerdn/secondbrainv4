import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { AccountDeletion, OwnedSharedItem } from "@secondbrain/api-types";
import { Button, Card, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

const DELETION_KEY = ["account", "deletion"] as const;
const SHARED_KEY = ["account", "shared-items"] as const;

/** A date as the user reads it, e.g. "27 Aralık 2026". */
export function longDate(iso: string, locale: string): string {
  return new Date(iso).toLocaleDateString(locale, {
    day: "numeric",
    month: "long",
    year: "numeric",
  });
}

/**
 * Account deletion (PRD HS2, T4.3.1): ask, see when it happens, cancel; and the guide to hand
 * over shared items first. Whatever is not handed over goes to its oldest member after 30 days.
 */
export function AccountDeletionCard() {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const deletion = useQuery({ queryKey: DELETION_KEY, queryFn: () => api.account.deletion() });
  const pending = Boolean(deletion.data?.deletes_at);
  const shared = useQuery({
    queryKey: SHARED_KEY,
    queryFn: () => api.account.sharedItems(),
    enabled: pending || confirming,
  });
  const onDeletion = {
    onSuccess: (next: AccountDeletion) => {
      client.setQueryData(DELETION_KEY, next);
      setConfirming(false);
    },
  };
  const request = useIdempotentMutation<AccountDeletion, void>(
    (_: void, idempotencyKey) => api.account.requestDeletion({ idempotencyKey }),
    onDeletion,
  );
  const cancel = useIdempotentMutation<AccountDeletion, void>(
    (_: void, idempotencyKey) => api.account.cancelDeletion({ idempotencyKey }),
    onDeletion,
  );
  const transfer = useIdempotentMutation<void, OwnedSharedItem>(
    (item, idempotencyKey) =>
      api.items.transferOwnership(item.item_id, { user_id: item.heir_id }, { idempotencyKey }),
    {
      onSuccess: () => {
        void client.invalidateQueries({ queryKey: SHARED_KEY });
      },
    },
  );

  if (deletion.isPending || deletion.isError) {
    return null;
  }
  const items = shared.data ?? [];
  return (
    <Card className="gap-3">
      <Text variant="label">{t("account.deletion.title")}</Text>
      {pending && deletion.data.deletes_at ? (
        <Text tone="danger">
          {t("account.deletion.pending", {
            date: longDate(deletion.data.deletes_at, i18n.language),
          })}
        </Text>
      ) : (
        <Text tone="muted">{t("account.deletion.hint")}</Text>
      )}
      {(pending || confirming) && items.length > 0 ? (
        <View className="gap-2">
          <Text>{t("account.deletion.sharedTitle")}</Text>
          {items.map((item) => (
            <View
              key={item.item_id}
              className="flex-row flex-wrap items-center justify-between gap-2 border-t border-border pt-2"
            >
              <View className="shrink gap-0.5">
                <Text>{item.title}</Text>
                <Text tone="muted">
                  {t("account.deletion.heir", { name: item.heir_name || "—" })}
                </Text>
              </View>
              <Button
                title={t("account.deletion.transferNow")}
                variant="secondary"
                size="sm"
                loading={transfer.isPending && transfer.variables?.item_id === item.item_id}
                onPress={() => {
                  transfer.mutate(item);
                }}
              />
            </View>
          ))}
          {transfer.isError ? (
            <Text tone="danger">{t("account.deletion.transferFailed")}</Text>
          ) : null}
        </View>
      ) : null}
      {pending ? (
        <Button
          title={t("account.deletion.cancel")}
          variant="primary"
          className="self-start"
          loading={cancel.isPending}
          onPress={() => {
            cancel.mutate();
          }}
        />
      ) : confirming ? (
        <View className="gap-2">
          <Text tone="danger">{t("account.deletion.confirm")}</Text>
          <View className="flex-row flex-wrap gap-2">
            <Button
              title={t("account.deletion.confirmYes")}
              variant="danger"
              loading={request.isPending}
              onPress={() => {
                request.mutate();
              }}
            />
            <Button
              title={t("account.deletion.confirmNo")}
              variant="secondary"
              onPress={() => {
                setConfirming(false);
              }}
            />
          </View>
        </View>
      ) : (
        <Button
          title={t("account.deletion.request")}
          variant="dangerOutline"
          className="self-start"
          onPress={() => {
            setConfirming(true);
          }}
        />
      )}
      {request.isError || cancel.isError ? (
        <Text tone="danger">{t("account.deletion.failed")}</Text>
      ) : null}
    </Card>
  );
}
