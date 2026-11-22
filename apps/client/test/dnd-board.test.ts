import {
  type Board,
  type ColumnLayout,
  dropTarget,
  indicatorPosition,
  isNoOp,
  moveCard,
} from "@/lib/dnd/board";

const rect = (x: number, y: number, width = 100, height = 40) => ({ x, y, width, height });

/** Two columns 100 wide with a 20 gap; cards 40 tall with a 10 gap, starting at y=50. */
const layouts: ColumnLayout[] = [
  {
    id: "todo",
    rect: rect(0, 0, 100, 400),
    cards: [
      { id: "a", rect: rect(0, 50) },
      { id: "b", rect: rect(0, 100) },
      { id: "c", rect: rect(0, 150) },
    ],
  },
  { id: "done", rect: rect(120, 0, 100, 400), cards: [{ id: "d", rect: rect(120, 50) }] },
];

const board: Board = [
  { id: "todo", title: "Yapılacak", cardIds: ["a", "b", "c"] },
  { id: "done", title: "Bitti", cardIds: ["d"] },
];

describe("dropTarget", () => {
  it("finds the column under the pointer and the slot by card centres", () => {
    expect(dropTarget({ x: 50, y: 60 }, layouts, "c")).toEqual({ columnId: "todo", index: 0 });
    expect(dropTarget({ x: 50, y: 125 }, layouts, "c")).toEqual({ columnId: "todo", index: 2 });
    expect(dropTarget({ x: 150, y: 300 }, layouts, "a")).toEqual({ columnId: "done", index: 1 });
    expect(dropTarget({ x: 150, y: 10 }, layouts, "a")).toEqual({ columnId: "done", index: 0 });
  });

  it("ignores the dragged card when counting", () => {
    // Below a and b, but b is the one being dragged.
    expect(dropTarget({ x: 50, y: 140 }, layouts, "b")).toEqual({ columnId: "todo", index: 1 });
  });

  it("uses the nearest column in gaps and beyond the edges", () => {
    expect(dropTarget({ x: 105, y: 60 }, layouts, "a")?.columnId).toBe("todo");
    expect(dropTarget({ x: 116, y: 60 }, layouts, "a")?.columnId).toBe("done");
    expect(dropTarget({ x: -50, y: 60 }, layouts, "d")?.columnId).toBe("todo");
    expect(dropTarget({ x: 900, y: 60 }, layouts, "a")?.columnId).toBe("done");
  });

  it("has no target without columns", () => {
    expect(dropTarget({ x: 0, y: 0 }, [], "a")).toBeNull();
  });
});

describe("moveCard", () => {
  it("reorders within a column", () => {
    expect(moveCard(board, "c", { columnId: "todo", index: 0 })[0]?.cardIds).toEqual([
      "c",
      "a",
      "b",
    ]);
  });

  it("moves across columns", () => {
    const moved = moveCard(board, "a", { columnId: "done", index: 0 });
    expect(moved.map((c) => c.cardIds)).toEqual([
      ["b", "c"],
      ["a", "d"],
    ]);
  });

  it("clamps the index and ignores unknown cards or columns", () => {
    expect(moveCard(board, "a", { columnId: "done", index: 99 })[1]?.cardIds).toEqual(["d", "a"]);
    expect(moveCard(board, "zzz", { columnId: "done", index: 0 })).toBe(board);
    expect(moveCard(board, "a", { columnId: "nope", index: 0 })).toBe(board);
  });

  it("keeps every card exactly once and other cards in order (random moves)", () => {
    let seed = 42;
    const random = () => {
      seed = (seed * 1_103_515_245 + 12_345) % 2 ** 31;
      return seed / 2 ** 31;
    };
    let current: Board = [
      { id: "1", title: "1", cardIds: ["a", "b", "c", "d"] },
      { id: "2", title: "2", cardIds: ["e", "f"] },
      { id: "3", title: "3", cardIds: [] },
    ];
    const all = current.flatMap((c) => c.cardIds).sort();
    for (let i = 0; i < 500; i += 1) {
      const cards = current.flatMap((c) => c.cardIds);
      const card = cards[Math.floor(random() * cards.length)] ?? "a";
      const column = current[Math.floor(random() * current.length)]?.id ?? "1";
      const index = Math.floor(random() * 6);
      const before = current;
      current = moveCard(current, card, { columnId: column, index });

      expect(current.flatMap((c) => c.cardIds).sort()).toEqual(all);
      const landed = current.find((c) => c.id === column)?.cardIds ?? [];
      expect(landed.indexOf(card)).toBe(Math.min(index, landed.length - 1));
      // Every other card keeps its relative order within its column.
      for (const col of current) {
        const others = col.cardIds.filter((id) => id !== card);
        const was = (before.find((c) => c.id === col.id)?.cardIds ?? []).filter(
          (id) => id !== card,
        );
        expect(others).toEqual(was);
      }
    }
  });
});

describe("isNoOp", () => {
  it("detects drops that leave the card in place", () => {
    expect(isNoOp(board, "b", { columnId: "todo", index: 1 })).toBe(true);
    expect(isNoOp(board, "b", { columnId: "todo", index: 0 })).toBe(false);
    expect(isNoOp(board, "b", { columnId: "done", index: 0 })).toBe(false);
  });
});

describe("indicatorPosition", () => {
  it("maps the target slot back to a display position with the dragged card in place", () => {
    const ids = ["a", "b", "c"];
    expect(indicatorPosition(ids, "c", 0)).toBe(0);
    expect(indicatorPosition(ids, "c", 1)).toBe(1);
    // Dragging the last card to the end: one indicator, after the last card.
    expect(indicatorPosition(ids, "c", 2)).toBe(3);
    // Dragging the first card: slot 0 is before b.
    expect(indicatorPosition(ids, "a", 0)).toBe(1);
    // From another column: slots map one to one.
    expect(indicatorPosition(ids, "x", 3)).toBe(3);
  });
});
