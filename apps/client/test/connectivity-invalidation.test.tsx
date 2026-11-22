import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react-native";
import { Text } from "react-native";

import { ConnectivityProvider } from "@/connectivity";
import { defaultRules, eventOf, invalidationsFor } from "@/connectivity/invalidation";

import { fakeLinks, fakeNetwork } from "./support/connectivity";

const event = (name: string, data: unknown) => ({ name, data });

describe("invalidation rules (ARCHITECTURE §10.1)", () => {
  it("item.changed invalidates every query of that kind", () => {
    expect(
      invalidationsFor(event("item.changed", { id: "1", kind: "task" }), defaultRules),
    ).toEqual([["task"]]);
  });

  it("notification.new invalidates notifications", () => {
    expect(invalidationsFor(event("notification.new", { id: "n1" }), defaultRules)).toEqual([
      ["notifications"],
    ]);
  });

  it("ignores malformed data and unknown events", () => {
    for (const data of [
      null,
      "task",
      [],
      { id: "1" },
      { kind: "task" },
      { id: 1, kind: "task" },
      { id: "1", kind: "" },
    ]) {
      expect(invalidationsFor(event("item.changed", data), defaultRules)).toEqual([]);
    }
    expect(invalidationsFor(event("unknown.event", {}), defaultRules)).toEqual([]);
    expect(invalidationsFor(event("toString", {}), defaultRules)).toEqual([]);
    expect(invalidationsFor(event("constructor", {}), defaultRules)).toEqual([]);
  });

  it("extracts events from server messages only", () => {
    const e = event("item.changed", { id: "1", kind: "task" });
    expect(eventOf({ type: "event", channel: "user:u1", event: e })).toEqual(e);
    expect(eventOf({ type: "subscribed", channel: "user:u1" })).toBeNull();
  });
});

/** Counts how often each query was fetched. */
function Queries({ fetches }: { fetches: Record<string, number> }) {
  const make = (key: string) => () => {
    fetches[key] = (fetches[key] ?? 0) + 1;
    return Promise.resolve(fetches[key]);
  };
  const task = useQuery({ queryKey: ["task", "1"], queryFn: make("task:1") });
  const tasks = useQuery({ queryKey: ["task", "list"], queryFn: make("task:list") });
  const routine = useQuery({ queryKey: ["routine", "1"], queryFn: make("routine:1") });
  return <Text testID="loaded">{[task.data, tasks.data, routine.data].join(",")}</Text>;
}

describe("ConnectivityProvider invalidation", () => {
  async function setup() {
    const links = fakeLinks();
    const network = fakeNetwork();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: Infinity, staleTime: Infinity } },
    });
    const fetches: Record<string, number> = {};
    await render(
      <QueryClientProvider client={queryClient}>
        <ConnectivityProvider
          userId="u1"
          openLink={links.open}
          network={network.source}
          random={() => 0}
        >
          <Queries fetches={fetches} />
        </ConnectivityProvider>
      </QueryClientProvider>,
    );
    await screen.findByText("1,1,1");
    return { ...links, fetches };
  }

  it("an event refetches only the queries it concerns; data comes over HTTP", async () => {
    const { last, fetches } = await setup();
    await act(() => {
      last().handlers.up();
      last().handlers.message?.({
        type: "event",
        channel: "user:u1",
        event: event("item.changed", { id: "1", kind: "task" }),
      });
    });
    await screen.findByText("2,2,1");
    expect(fetches).toEqual({ "task:1": 2, "task:list": 2, "routine:1": 1 });
  });

  it("other messages and unknown events refetch nothing", async () => {
    const { last, fetches } = await setup();
    await act(() => {
      last().handlers.up();
      last().handlers.message?.({ type: "subscribed", channel: "user:u1" });
      last().handlers.message?.({
        type: "event",
        channel: "user:u1",
        event: event("something.else", {}),
      });
    });
    expect(fetches).toEqual({ "task:1": 1, "task:list": 1, "routine:1": 1 });
  });

  it("a reconnect refetches everything", async () => {
    const { last, fetches } = await setup();
    await act(() => {
      last().handlers.up();
      last().handlers.lost("socket_closed");
    });
    await act(() => {
      last().handlers.up();
    });
    await screen.findByText("2,2,2");
    expect(fetches).toEqual({ "task:1": 2, "task:list": 2, "routine:1": 2 });
  });
});
