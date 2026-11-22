import { Text } from "@secondbrain/ui";
import { invoke } from "@tauri-apps/api/core";
import TauriWebSocket from "@tauri-apps/plugin-websocket";
import { Redirect } from "expo-router";
import { useEffect, useState } from "react";
import { ScrollView } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import { API_URL, idempotencyKey } from "@/lib/api";

/**
 * Spike T0.6.3 (ADR-0020): connectivity checks inside the Tauri desktop shell. Creates
 * throwaway accounts on the local development server and measures:
 * - cookie mode (as the web app): register, `/me` with the cookie, `/ws` handshake;
 * - bearer mode (as the native apps): register, `/me` with the token, `/ws` through Tauri's
 *   WebSocket plugin, which can send an Authorization header (the webview's WebSocket cannot).
 * Results are shown and reported to the Rust side (stdout). Development only.
 */
export default function DesktopSpike() {
  if (!__DEV__) {
    return <Redirect href="/" />;
  }
  return <Checks />;
}

type Results = Record<string, string>;

const wsUrl = `${API_URL.replace(/^http/u, "ws")}/ws`;
const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function register(client: "web" | "native") {
  const response = await fetch(`${API_URL}/auth/register`, {
    method: "POST",
    credentials: client === "web" ? "include" : "omit",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      email: `desktop-${client}-${Date.now()}@example.test`,
      password: idempotencyKey(),
      display_name: "Desktop spike",
      client,
    }),
  });
  const body = (await response.json().catch(() => ({}))) as { token?: string };
  return { status: response.status, token: body.token ?? null };
}

function browserSocket(): Promise<string> {
  return new Promise((resolve) => {
    const socket = new WebSocket(wsUrl);
    const timer = setTimeout(() => resolve("timeout"), 5_000);
    socket.onmessage = (message) => {
      clearTimeout(timer);
      resolve(String(message.data).includes('"hello"') ? "hello" : "unexpected");
      socket.close();
    };
    socket.onclose = (event) => {
      clearTimeout(timer);
      resolve(`closed ${event.code}`);
    };
  });
}

async function pluginSocket(token: string): Promise<string> {
  const socket = await TauriWebSocket.connect(wsUrl, {
    headers: { Authorization: `Bearer ${token}` },
  });
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve("timeout"), 5_000);
    socket.addListener((message) => {
      clearTimeout(timer);
      const text = message.type === "Text" ? message.data : "";
      resolve(text.includes('"hello"') ? "hello" : `got ${message.type}`);
      void socket.disconnect();
    });
  });
}

async function runChecks(): Promise<Results> {
  const results: Results = { origin: window.location.origin, inTauri: String(inTauri) };
  const step = async (name: string, check: () => Promise<string>) => {
    try {
      results[name] = await check();
    } catch (error) {
      results[name] = `error: ${String(error)}`;
    }
  };

  await step("cookieRegister", async () => String((await register("web")).status));
  await step("cookieMe", async () =>
    String((await fetch(`${API_URL}/me`, { credentials: "include" })).status),
  );
  await step("cookieWs", browserSocket);

  let token: string | null = null;
  await step("bearerRegister", async () => {
    const result = await register("native");
    token = result.token;
    return String(result.status);
  });
  await step("bearerMe", async () =>
    String(
      (await fetch(`${API_URL}/me`, { headers: { Authorization: `Bearer ${token ?? ""}` } }))
        .status,
    ),
  );
  await step("bearerWsWebview", browserSocket);
  if (inTauri) await step("bearerWsPlugin", () => pluginSocket(token ?? ""));
  return results;
}

function Checks() {
  const [results, setResults] = useState<Results | null>(null);
  useEffect(() => {
    void runChecks().then(async (r) => {
      setResults(r);
      if (inTauri) await invoke("spike_report", { report: r }).catch(() => undefined);
    });
  }, []);
  return (
    <SafeAreaView className="flex-1 bg-background">
      <ScrollView contentContainerClassName="gap-2 p-4">
        <Text variant="heading">Masaüstü spike</Text>
        {results === null ? (
          <Text tone="muted">Kontroller çalışıyor…</Text>
        ) : (
          Object.entries(results).map(([key, value]) => (
            <Text key={key} testID={`check-${key}`}>
              {key}: {value}
            </Text>
          ))
        )}
      </ScrollView>
    </SafeAreaView>
  );
}
