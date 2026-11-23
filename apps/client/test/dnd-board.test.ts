import {
  AUTO_SCROLL_MAX_STEP,
  autoScrollStep,
  type Board,
  cardOffsets,
  type ColumnLayout,
  dropTarget,
  indicatorPosition,
  isNoOp,
  moveCard,
  virtualCardLayouts,
  visibleWindow,
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

describe("autoScrollStep", () => {
  it("scrolls only near the edges, faster closer to them", () => {
    expect(autoScrollStep(500, 100, 800)).toBe(0);
    expect(autoScrollStep(150, 100, 800)).toBeLessThan(0);
    expect(autoScrollStep(110, 100, 800)).toBeLessThan(autoScrollStep(150, 100, 800));
    expect(autoScrollStep(850, 100, 800)).toBeGreaterThan(0);
    expect(autoScrollStep(890, 100, 800)).toBeGreaterThan(autoScrollStep(850, 100, 800));
  });

  it("is capped beyond the edge and safe for an empty area", () => {
    expect(autoScrollStep(-500, 100, 800)).toBe(-AUTO_SCROLL_MAX_STEP);
    expect(autoScrollStep(5_000, 100, 800)).toBe(AUTO_SCROLL_MAX_STEP);
    expect(autoScrollStep(10, 0, 0)).toBe(0);
  });

  it("does not scroll both ways in a small area", () => {
    expect(autoScrollStep(50, 0, 100)).toBe(0);
    expect(autoScrollStep(10, 0, 100)).toBeLessThan(0);
    expect(autoScrollStep(90, 0, 100)).toBeGreaterThan(0);
  });
});

describe("virtualized columns", () => {
  const ids = Array.from({ length: 200 }, (_, i) => `c${i}`);
  const { offsets, total } = cardOffsets(ids, new Map([["c1", 200]]), 100, 10);

  it("places cards from measured and estimated heights", () => {
    expect(offsets[0]).toEqual({ id: "c0", top: 0, height: 100 });
    expect(offsets[1]).toEqual({ id: "c1", top: 110, height: 200 });
    expect(offsets[2]?.top).toBe(320);
    expect(total).toBe(199 * 110 + 100 + 100);
    expect(cardOffsets([], new Map(), 100, 10)).toEqual({ offsets: [], total: 0 });
  });

  it("renders only the cards around the viewport", () => {
    const top = visibleWindow(offsets, 0, 480, 2);
    expect(top.first).toBe(0);
    expect(top.last).toBeLessThan(10);
    const middle = visibleWindow(offsets, 11_000, 480, 2);
    expect(middle.first).toBeGreaterThan(90);
    expect(middle.last - middle.first).toBeLessThan(12);
    const end = visibleWindow(offsets, 1_000_000, 480, 2);
    expect(end.last).toBe(200);
    expect(visibleWindow([], 0, 480, 2)).toEqual({ first: 0, last: 0 });
  });

  it("always covers the viewport, wherever it is scrolled", () => {
    for (let scrollTop = 0; scrollTop < total; scrollTop += 137) {
      const { first, last } = visibleWindow(offsets, scrollTop, 480, 0);
      const shown = offsets.slice(first, last);
      // A viewport edge may fall in the 10-point gap between two cards, where nothing is drawn.
      expect(shown[0]?.top ?? 0).toBeLessThanOrEqual(scrollTop + 10);
      const end = shown[shown.length - 1];
      const reaches = (end?.top ?? 0) + (end?.height ?? 0);
      expect(reaches + 10 >= Math.min(scrollTop + 480, total)).toBe(true);
    }
  });

  it("drops at the right index among cards that are not rendered", () => {
    const scrollTop = 11_000;
    const contentY = 50 - scrollTop;
    const cards = virtualCardLayouts(offsets, contentY, 10, 200);
    expect(cards).toHaveLength(200);
    const column: ColumnLayout = { id: "long", rect: rect(0, 0, 220, 600), cards };
    const card = offsets[120];
    const y = contentY + (card?.top ?? 0) + 1;
    expect(dropTarget({ x: 50, y }, [column], "c0")).toEqual({ columnId: "long", index: 119 });
    expect(dropTarget({ x: 50, y }, [column], "c199")).toEqual({ columnId: "long", index: 120 });
  });
});
