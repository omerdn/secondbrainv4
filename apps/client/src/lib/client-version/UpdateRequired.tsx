import type { ClientPolicy } from "@secondbrain/api-types";
import { Button, Card, Text } from "@secondbrain/ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Linking, View } from "react-native";

import { API_URL } from "@/lib/api";

import { isOutdated } from "./compare";
import { type ClientPlatform, currentClient } from "./current";

export interface UpdateRequiredDeps {
  current(): Promise<{ platform: ClientPlatform; version: string } | null>;
  policy(): Promise<ClientPolicy>;
  open(url: string): Promise<unknown>;
}

const defaults: UpdateRequiredDeps = {
  current: currentClient,
  policy: async () => {
    const response = await fetch(`${API_URL}/client/policy`);
    if (!response.ok) throw new Error(`client policy: ${response.status}`);
    return (await response.json()) as ClientPolicy;
  },
  open: (url) => Linking.openURL(url),
};

/**
 * Blocks an app older than the server's minimum supported version (T4.2.7, ADR-0023) with a
 * link to the new version. Checked once per launch; any failure lets the app run.
 */
export function UpdateRequired({ deps = defaults }: { deps?: UpdateRequiredDeps }) {
  const { t } = useTranslation();
  const [blocked, setBlocked] = useState<{ url: string | null } | null>(null);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const client = await deps.current();
        if (client === null) return;
        const rule = (await deps.policy())[client.platform];
        if (active && isOutdated(client.version, rule.min_version)) {
          setBlocked({ url: rule.download_url });
        }
      } catch {
        // Offline or old server: the connectivity screens handle reachability.
      }
    })();
    return () => {
      active = false;
    };
  }, [deps]);

  if (blocked === null) return null;
  const { url } = blocked;
  return (
    <View
      className="absolute inset-0 items-center justify-center bg-background px-4"
      accessibilityViewIsModal
    >
      <Card className="w-full max-w-sm gap-3">
        <Text variant="title">{t("updateRequired.title")}</Text>
        <Text tone="muted">{t("updateRequired.description")}</Text>
        {url !== null ? (
          <Button title={t("updateRequired.download")} onPress={() => void deps.open(url)} />
        ) : null}
      </Card>
    </View>
  );
}
