import { act } from "@testing-library/react-native";

import type { LinkHandlers, LinkOpener } from "@/connectivity";

/** Fake links and OS network events for driving `ConnectivityProvider` in tests. */
export interface FakeLink {
  authenticated: boolean;
  handlers: LinkHandlers;
  checks: number;
  closed: boolean;
}

export function fakeLinks() {
  const links: FakeLink[] = [];
  const open: LinkOpener = (authenticated, handlers) => {
    const link: FakeLink = { authenticated, handlers, checks: 0, closed: false };
    links.push(link);
    return {
      check: () => {
        link.checks += 1;
      },
      close: () => {
        link.closed = true;
      },
    };
  };
  const last = () => {
    const link = links.at(-1);
    if (link === undefined) throw new Error("no link opened");
    return link;
  };
  return { links, open, last };
}

export function fakeNetwork() {
  let listener: ((connected: boolean) => void) | undefined;
  return {
    source: (l: (connected: boolean) => void) => {
      listener = l;
      return () => {
        listener = undefined;
      };
    },
    set: (connected: boolean) =>
      act(() => {
        listener?.(connected);
      }),
  };
}
