// @vitest-environment happy-dom
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { ApiClient, ApiError, createQueryClient, endpoints } from "../src";
import { ApiProvider, useApi, useIdempotentMutation } from "../src/react";

const api = endpoints(
  new ApiClient({ baseUrl: "http://api.test", auth: { kind: "bearer", token: () => null } }),
);

function wrapper({ children }: { children: ReactNode }) {
  return (
    <ApiProvider api={api} queryClient={createQueryClient()}>
      {children}
    </ApiProvider>
  );
}

/** Wrapper whose provider hands out k1, k2, … as idempotency keys. */
function countingWrapper() {
  let n = 0;
  const generate = () => `k${++n}`;
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <ApiProvider api={api} queryClient={createQueryClient()} idempotencyKey={generate}>
        {children}
      </ApiProvider>
    );
  };
}

describe("useApi", () => {
  it("returns the endpoints inside the provider", () => {
    const { result } = renderHook(() => useApi(), { wrapper });
    expect(typeof result.current.me.get).toBe("function");
  });

  it("throws outside the provider", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(() => renderHook(() => useApi())).toThrow(/ApiProvider/);
  });
});

describe("useIdempotentMutation", () => {
  it("reuses the key when the same input is re-submitted after a failure", async () => {
    const keys: string[] = [];
    let fail = true;
    const fn = vi.fn((_vars: { name: string }, key: string) => {
      keys.push(key);
      return fail ? Promise.reject(new ApiError("network", "x", 0)) : Promise.resolve("ok");
    });
    const { result } = renderHook(() => useIdempotentMutation(fn), {
      wrapper: countingWrapper(),
    });

    await act(async () => {
      await result.current.mutateAsync({ name: "Ada" }).catch(() => undefined);
    });
    // Same input again: same key.
    fail = false;
    await act(async () => {
      await result.current.mutateAsync({ name: "Ada" });
    });
    // After a success the next submission is a new operation.
    await act(async () => {
      await result.current.mutateAsync({ name: "Ada" });
    });
    expect(keys).toEqual(["k1", "k1", "k2"]);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
  });

  it("uses a new key when the input changed", async () => {
    const keys: string[] = [];
    const fn = vi.fn((_vars: { name: string }, key: string) => {
      keys.push(key);
      return Promise.reject(new ApiError("validation_failed", "x", 422));
    });
    const { result } = renderHook(() => useIdempotentMutation(fn), {
      wrapper: countingWrapper(),
    });
    await act(async () => {
      await result.current.mutateAsync({ name: "" }).catch(() => undefined);
      await result.current.mutateAsync({ name: "Ada" }).catch(() => undefined);
    });
    expect(keys).toEqual(["k1", "k2"]);
  });
});
