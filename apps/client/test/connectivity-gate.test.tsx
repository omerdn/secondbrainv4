import { ApiError } from "@secondbrain/api-client";
import { Button, Input, ThemeProvider } from "@secondbrain/ui";
import { QueryClient, QueryClientProvider, useMutation } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react-native";
import { useState } from "react";
import { Text } from "react-native";

import { ConnectivityGate, ConnectivityProvider, useOnConnected } from "@/connectivity";
import "@/lib/i18n";

import { fakeLinks, fakeNetwork } from "./support/connectivity";

/** A form whose save is pending until the test settles it (BP3). */
function Form({ save }: { save: () => Promise<void> }) {
  const [title, setTitle] = useState("");
  const mutation = useMutation({ mutationFn: save });
  return (
    <>
      <Input label="Başlık" value={title} onChangeText={setTitle} />
      <Button title="Kaydet" onPress={() => mutation.mutate()} />
      {mutation.isError ? <Text>kaydedilemedi</Text> : null}
      {mutation.isSuccess ? <Text>kaydedildi</Text> : null}
    </>
  );
}

function ConnectedCounter() {
  const [count, setCount] = useState(0);
  useOnConnected(() => setCount((c) => c + 1));
  return <Text testID="connected-count">{count}</Text>;
}

async function setup(save: () => Promise<void> = () => Promise.resolve()) {
  const links = fakeLinks();
  const network = fakeNetwork();
  // No garbage-collection timers: they would keep Jest alive after the real-timer test.
  const queryClient = new QueryClient({
    defaultOptions: { queries: { gcTime: Infinity }, mutations: { gcTime: Infinity } },
  });
  await render(
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <ConnectivityProvider
          userId="u1"
          openLink={links.open}
          network={network.source}
          random={() => 0}
        >
          <ConnectedCounter />
          <ConnectivityGate>
            <Form save={save} />
          </ConnectivityGate>
        </ConnectivityProvider>
      </ThemeProvider>
    </QueryClientProvider>,
  );
  const signal = (fn: () => void) =>
    act(() => {
      fn();
    });
  return { ...links, network, signal };
}

beforeEach(() => jest.useFakeTimers());
afterEach(() => jest.useRealTimers());

describe("ConnectivityGate", () => {
  it("BP1: shows nothing of the app while connecting", async () => {
    await setup();
    expect(screen.getByTestId("connectivity-booting")).toBeOnTheScreen();
    expect(screen.queryByLabelText("Başlık")).toBeNull();
  });

  it("BP1: a failed start shows 'no connection' until a retry succeeds", async () => {
    const { last, signal } = await setup();
    await signal(() => last().handlers.failed());
    expect(screen.getByText("Bağlantı yok")).toBeOnTheScreen();
    expect(screen.queryByLabelText("Başlık")).toBeNull();

    await fireEvent.press(screen.getByRole("button", { name: "Tekrar dene" }));
    expect(screen.getByTestId("connectivity-booting")).toBeOnTheScreen();
    await signal(() => last().handlers.up());
    expect(screen.getByLabelText("Başlık")).toBeOnTheScreen();
  });

  it("BP2: a lost connection locks the screen until it is back", async () => {
    const { last, signal } = await setup();
    await signal(() => last().handlers.up());
    expect(screen.queryByTestId("connectivity-lock")).toBeNull();

    await signal(() => last().handlers.lost("heartbeat_timeout"));
    expect(screen.getByTestId("connectivity-lock")).toBeOnTheScreen();
    expect(screen.getByText("Bağlantı koptu, yeniden bağlanılıyor…")).toBeOnTheScreen();

    await signal(() => last().handlers.up());
    expect(screen.queryByTestId("connectivity-lock")).toBeNull();
  });

  it("BP3: the form keeps its content and a pending save still reports its outcome", async () => {
    let reject: (error: unknown) => void = () => undefined;
    const save = () =>
      new Promise<void>((_, r) => {
        reject = r;
      });
    // TanStack Query schedules its notifications on timers.
    jest.useRealTimers();
    const { last, signal } = await setup(save);
    await signal(() => last().handlers.up());

    await fireEvent.changeText(screen.getByLabelText("Başlık"), "Süt al");
    await fireEvent.press(screen.getByRole("button", { name: "Kaydet" }));
    await signal(() => last().handlers.lost("socket_closed"));
    expect(screen.getByTestId("connectivity-lock")).toBeOnTheScreen();

    // The request in flight fails: the error is shown, not swallowed.
    await act(async () => {
      reject(new ApiError("network", "offline", 0));
      await Promise.resolve();
    });
    await signal(() => last().handlers.up());
    expect(await screen.findByText("kaydedilemedi")).toBeOnTheScreen();
    expect(screen.getByLabelText("Başlık").props.value).toBe("Süt al");
  });

  it("runs on-connected callbacks each time the connection comes up", async () => {
    const { last, signal } = await setup();
    expect(screen.getByTestId("connected-count").props.children).toBe(0);
    await signal(() => last().handlers.up());
    expect(screen.getByTestId("connected-count").props.children).toBe(1);
    await signal(() => last().handlers.lost("socket_closed"));
    await signal(() => last().handlers.up());
    expect(screen.getByTestId("connected-count").props.children).toBe(2);
  });
});
