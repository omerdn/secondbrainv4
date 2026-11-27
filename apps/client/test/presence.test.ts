import { cursorColor, presentPeople } from "@/lib/editor/collab";

const state = (id: string | undefined, name: string, color?: string) => ({
  user: { ...(id === undefined ? {} : { id }), name, ...(color === undefined ? {} : { color }) },
});

describe("live presence (Y3, T3.4.2)", () => {
  it("lists everyone else once, the user's other windows last", () => {
    const states = new Map<number, Record<string, unknown>>([
      [1, state("u1", "Ali", "#111")],
      [2, state("u2", "Zeynep", "#222")],
      [3, state("u3", "Ayşe", "#333")],
      [4, state("u3", "Ayşe", "#333")],
      [5, state("u1", "Ali", "#111")],
    ]);
    expect(presentPeople(states, 1, "u1")).toEqual([
      { id: "u3", name: "Ayşe", color: "#333", self: false },
      { id: "u2", name: "Zeynep", color: "#222", self: false },
      { id: "u1", name: "Ali", color: "#111", self: true },
    ]);
  });

  it("drops whoever left and ignores states without a user", () => {
    const before = new Map<number, Record<string, unknown>>([
      [1, state("u1", "Ali")],
      [2, state("u2", "Can")],
      [3, {}],
    ]);
    expect(presentPeople(before, 1, "u1").map((p) => p.name)).toEqual(["Can"]);
    const after = new Map(before);
    after.delete(2);
    expect(presentPeople(after, 1, "u1")).toEqual([]);
  });

  it("falls back to the client id and a stable colour", () => {
    const states = new Map<number, Record<string, unknown>>([[7, state(undefined, "Misafir")]]);
    expect(presentPeople(states, 1, "u1")).toEqual([
      { id: "client:7", name: "Misafir", color: cursorColor("client:7"), self: false },
    ]);
  });
});
