import type { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import { Button, Card, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { Redirect } from "expo-router";
import { useState } from "react";
import { Platform, ScrollView, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import { API_URL, credentials } from "@/lib/api";

/**
 * Development-only check of `packages/api-client` against a running server (T0.4.4): health
 * query, register, and an idempotent profile update. Production builds redirect home.
 */
export default function ApiCheck() {
  if (!__DEV__) {
    return <Redirect href="/" />;
  }
  return <Check />;
}

function describe(error: unknown): string {
  const e = error as ApiError;
  return `${e.code} (HTTP ${e.status}): ${e.message}`;
}

function Check() {
  const api = useApi();
  const [log, setLog] = useState<string[]>([]);
  const add = (line: string) => setLog((l) => [...l, line]);

  const health = useQuery({ queryKey: ["health"], queryFn: api.health });

  const register = useIdempotentMutation(async () => {
    const legal = await api.legal.list();
    const res = await api.auth.register({
      accepted_legal: legal.map((doc) => ({ kind: doc.kind, version: doc.version })),
      email: `dev${Date.now()}@example.test`,
      password: "correct horse",
      display_name: "Dev User",
      client: Platform.OS === "web" ? "web" : "native",
    });
    credentials.token = res.token ?? null;
    credentials.csrfToken = res.csrf_token ?? null;
    return res;
  });

  const update = useIdempotentMutation((vars: { timezone: string }, idempotencyKey) =>
    api.me.update(vars, { idempotencyKey }),
  );

  return (
    <SafeAreaView className="flex-1 bg-background">
      <ScrollView contentContainerClassName="gap-4 p-4">
        <Text variant="title">API check</Text>
        <Text tone="muted">{API_URL}</Text>
        <Card>
          <Text variant="label">Health</Text>
          <Text testID="health">
            {health.isPending
              ? "…"
              : health.isError
                ? describe(health.error)
                : `${health.data.status}`}
          </Text>
        </Card>
        <View className="flex-row flex-wrap gap-2">
          <Button
            title="Register"
            loading={register.isPending}
            onPress={() =>
              register.mutate(undefined, {
                onSuccess: (r) => add(`registered ${r.user.email}`),
                onError: (e) => add(`register failed: ${describe(e)}`),
              })
            }
          />
          <Button
            title="Set timezone"
            variant="secondary"
            loading={update.isPending}
            onPress={() =>
              update.mutate(
                { timezone: "Europe/Istanbul" },
                {
                  onSuccess: (u) => add(`timezone = ${u.timezone}`),
                  onError: (e) => add(`update failed: ${describe(e)}`),
                },
              )
            }
          />
          <Button
            title="Invalid timezone"
            variant="ghost"
            onPress={() =>
              update.mutate(
                { timezone: "Mars/Base" },
                {
                  onError: (e) => add(`expected 422: ${e.code}, ${e.fieldMessage("timezone")}`),
                },
              )
            }
          />
        </View>
        <Card>
          <Text variant="label">Log</Text>
          {log.map((line, i) => (
            <Text key={i} variant="caption">
              {line}
            </Text>
          ))}
        </Card>
      </ScrollView>
    </SafeAreaView>
  );
}
