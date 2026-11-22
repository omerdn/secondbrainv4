import { ApiError } from "@secondbrain/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react-native";
import { Text } from "react-native";

import { ConnectivityProvider, useConnectivity } from "@/connectivity";
import { netInfoSource } from "@/connectivity/ConnectivityProvider";

import { fakeLinks, fakeNetwork } from "./support/connectivity";

function Probe() {
  const { state, retry } = useConnectivity();
  return (
    <Text testID="state" onPress={retry}>
      {state.kind === "reconnecting" ? `reconnecting:${state.attempt}` : state.kind}
    </Text>
  );
}

async function setup(userId: string | null = "u1", onSessionEnded?: () => void) {
  const links = fakeLinks();
  const network = fakeNetwork();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const tree = (auth: string | null) => (
    <QueryClientProvider client={queryClient}>
      <ConnectivityProvider
        userId={auth}
        openLink={links.open}
        network={network.source}
        random={() => 0}
        onSessionEnded={onSessionEnded}
      >
        <Probe />
      </ConnectivityProvider>
    </QueryClientProvider>
  );
  const view = await render(tree(userId));
  return {
    ...links,
    network,
    queryClient,
    rerender: (auth: string | null) => view.rerender(tree(auth)),
    unmount: view.unmount,
  };
}

const shown = () => screen.getByTestId("state").props.children as string;
const signal = (fn: () => void) =>
  act(() => {
    fn();
  });

beforeEach(() => jest.useFakeTimers());
afterEach(() => jest.useRealTimers());

describe("ConnectivityProvider", () => {
  it("boots with the socket when signed in and goes online", async () => {
    const { last } = await setup("u1");
    expect(shown()).toBe("booting");
    expect(last().userId).toBe("u1");
    await signal(() => last().handlers.up());
    expect(shown()).toBe("online");
  });

  it("BP1: a failed boot blocks until retry succeeds", async () => {
    const { last, links } = await setup(null);
    expect(last().userId).toBeNull();
    await signal(() => last().handlers.failed());
    expect(shown()).toBe("offline_blocked");
    await fireEvent.press(screen.getByTestId("state"));
    expect(shown()).toBe("booting");
    expect(links).toHaveLength(2);
    await signal(() => last().handlers.up());
    expect(shown()).toBe("online");
  });

  it("BP2: reconnects with a delay after the first attempt and refetches when back", async () => {
    const { last, links, queryClient } = await setup();
    const invalidate = jest.spyOn(queryClient, "invalidateQueries");
    await signal(() => last().handlers.up());
    await signal(() => last().handlers.lost("heartbeat_timeout"));
    expect(shown()).toBe("reconnecting:1");
    expect(links).toHaveLength(2);

    await signal(() => last().handlers.failed());
    expect(links).toHaveLength(2);
    // random() = 0 → half of the 1 s base delay.
    await act(() => {
      jest.advanceTimersByTime(499);
    });
    expect(links).toHaveLength(2);
    await act(() => {
      jest.advanceTimersByTime(1);
    });
    expect(links).toHaveLength(3);
    expect(shown()).toBe("reconnecting:2");

    await signal(() => last().handlers.up());
    expect(shown()).toBe("online");
    expect(invalidate).toHaveBeenCalledTimes(1);
  });

  it("follows OS network events", async () => {
    const { last, links, network } = await setup();
    await signal(() => last().handlers.up());
    await network.set(false);
    expect(shown()).toBe("reconnecting:1");
    await signal(() => last().handlers.failed());
    await network.set(true);
    expect(shown()).toBe("reconnecting:2");
    expect(links).toHaveLength(3);
  });

  it("BP5: a network failure of a request makes the link check itself", async () => {
    const { last, queryClient } = await setup();
    await signal(() => last().handlers.up());
    await act(() =>
      queryClient
        .fetchQuery({
          queryKey: ["x"],
          queryFn: () => Promise.reject(new ApiError("network", "offline", 0)),
        })
        .catch(() => undefined),
    );
    expect(last().checks).toBe(1);
    await act(() =>
      queryClient
        .fetchQuery({
          queryKey: ["y"],
          queryFn: () => Promise.reject(new ApiError("not_found", "nope", 404)),
        })
        .catch(() => undefined),
    );
    expect(last().checks).toBe(1);
  });

  it("swaps the link when the session changes, without leaving online", async () => {
    const { last, links, rerender } = await setup(null);
    await signal(() => last().handlers.up());
    await rerender("u1");
    expect(links).toHaveLength(2);
    expect(links[0]?.closed).toBe(true);
    expect(last().userId).toBe("u1");
    expect(shown()).toBe("online");
  });

  it("T0.5.7: a session ended by the server signs out without reconnecting", async () => {
    const onSessionEnded = jest.fn();
    const { last, links, rerender } = await setup("u1", onSessionEnded);
    await signal(() => last().handlers.up());
    await signal(() => last().handlers.sessionEnded?.());
    expect(onSessionEnded).toHaveBeenCalledTimes(1);
    expect(shown()).toBe("online");
    expect(last().closed).toBe(true);
    expect(links).toHaveLength(1);
    // Signed out: the signed-out link replaces it.
    await rerender(null);
    expect(links).toHaveLength(2);
    expect(last().userId).toBeNull();
  });

  it("closes the link and stops retrying on unmount", async () => {
    const { last, links, unmount } = await setup();
    await signal(() => last().handlers.up());
    await signal(() => last().handlers.lost("socket_closed"));
    await signal(() => last().handlers.failed());
    await unmount();
    expect(last().closed).toBe(true);
    jest.advanceTimersByTime(60_000);
    expect(links).toHaveLength(2);
  });
});

describe("netInfoSource", () => {
  it("reports only changes of the connected flag and disables third-party reachability checks", () => {
    const NetInfo = jest.requireMock<{
      default: { configure: jest.Mock; addEventListener: jest.Mock };
    }>("@react-native-community/netinfo").default;
    let emit: (state: { isConnected: boolean | null }) => void = () => undefined;
    const configure = jest.spyOn(NetInfo, "configure");
    jest.spyOn(NetInfo, "addEventListener").mockImplementation((l: typeof emit) => {
      emit = l;
      return () => undefined;
    });
    const seen: boolean[] = [];
    netInfoSource((connected) => seen.push(connected));
    for (const isConnected of [null, true, true, false, false, true]) emit({ isConnected });
    expect(seen).toEqual([true, false, true]);
    const options = configure.mock.calls[0]?.[0] as { reachabilityShouldRun: () => boolean };
    expect(options.reachabilityShouldRun()).toBe(false);
  });
});
