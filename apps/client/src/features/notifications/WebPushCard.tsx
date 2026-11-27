import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import { Button, Card, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import {
  subscribe,
  unsubscribe,
  webPushState,
  webPushSupported,
  type WebPushState,
} from "@/lib/web-push";

import { NOTIFICATIONS_KEY } from "./useUnreadCount";

const STATE_KEY = [...NOTIFICATIONS_KEY, "web-push"] as const;

/**
 * Turns Web Push on or off for this browser (T4.2.4). Hidden where the browser cannot push or
 * the server has no VAPID key. The "push" channel of each type still decides what is pushed.
 */
export function WebPushCard() {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const supported = webPushSupported();
  const key = useQuery({
    queryKey: [...NOTIFICATIONS_KEY, "push-key"],
    queryFn: () => api.notifications.pushKey(),
    enabled: supported,
    staleTime: Infinity,
  });
  const state = useQuery({ queryKey: STATE_KEY, queryFn: webPushState, enabled: supported });
  const toggle = useIdempotentMutation<WebPushState, boolean>(
    async (on, idempotencyKey) => {
      if (on) {
        const publicKey = key.data?.public_key;
        if (!publicKey) {
          return "off";
        }
        const request = await subscribe(publicKey);
        if (!request) {
          return webPushState();
        }
        await api.notifications.subscribePush(request, { idempotencyKey });
        return "on";
      }
      const endpoint = await unsubscribe();
      if (endpoint) {
        await api.notifications.unsubscribePush({ endpoint }, { idempotencyKey });
      }
      return "off";
    },
    {
      onSuccess: (next) => {
        client.setQueryData(STATE_KEY, next);
      },
    },
  );
  if (!supported || !key.data?.public_key || state.data === undefined) {
    return null;
  }
  const current = state.data;
  return (
    <Card className="gap-3">
      <View className="gap-1">
        <Text>{t("notifications.webPush.title")}</Text>
        <Text tone="muted">
          {current === "on"
            ? t("notifications.webPush.on")
            : current === "denied"
              ? t("notifications.webPush.denied")
              : t("notifications.webPush.off")}
        </Text>
      </View>
      {current === "denied" ? null : (
        <Button
          title={
            current === "on"
              ? t("notifications.webPush.disable")
              : t("notifications.webPush.enable")
          }
          variant={current === "on" ? "secondary" : "primary"}
          loading={toggle.isPending}
          onPress={() => {
            toggle.mutate(current !== "on");
          }}
        />
      )}
      {toggle.isError ? <Text tone="danger">{t("notifications.webPush.failed")}</Text> : null}
    </Card>
  );
}
