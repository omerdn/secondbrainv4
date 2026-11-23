/**
 * Pure kanban drag-and-drop logic (spike T0.6.1, ADR-0018): where a dragged card would land
 * and the board after the move. The gesture layer only reports pointer positions and measured
 * rectangles; everything decidable is decided here and unit-tested.
 */

export interface Point {
  x: number;
  y: number;
}

/** A rectangle in window coordinates (`measureInWindow`). */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CardLayout {
  id: string;
  rect: Rect;
}

/** A column and its cards as measured on screen, cards in display order. */
export interface ColumnLayout {
  id: string;
  rect: Rect;
  cards: CardLayout[];
}

/** Where a card would be dropped: `index` counts the column's cards without the dragged one. */
export interface DropTarget {
  columnId: string;
  index: number;
}

export interface BoardColumn {
  id: string;
  title: string;
  cardIds: string[];
}

export type Board = BoardColumn[];

/** Horizontal distance from `x` to the rectangle (0 when inside its x-range). */
function horizontalDistance(x: number, rect: Rect): number {
  if (x < rect.x) return rect.x - x;
  if (x > rect.x + rect.width) return x - (rect.x + rect.width);
  return 0;
}

/**
 * The drop target under `point`: the column whose x-range contains it (or the nearest one, so
 * dropping in a gap or beyond the edge still works), and the index among that column's cards
 * (the dragged card excluded) whose vertical centre lies above the point.
 */
export function dropTarget(
  point: Point,
  columns: readonly ColumnLayout[],
  draggedId: string,
): DropTarget | null {
  let best: ColumnLayout | undefined;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const column of columns) {
    const distance = horizontalDistance(point.x, column.rect);
    if (distance < bestDistance) {
      best = column;
      bestDistance = distance;
    }
  }
  if (best === undefined) return null;
  const index = best.cards.filter(
    (card) => card.id !== draggedId && card.rect.y + card.rect.height / 2 < point.y,
  ).length;
  return { columnId: best.id, index };
}

/**
 * The board with `cardId` moved to `target`. Unknown cards or columns leave the board as it is;
 * an index beyond the end appends.
 */
export function moveCard(board: Board, cardId: string, target: DropTarget): Board {
  const source = board.find((column) => column.cardIds.includes(cardId));
  if (source === undefined || !board.some((column) => column.id === target.columnId)) {
    return board;
  }
  return board.map((column) => {
    const without = column.cardIds.filter((id) => id !== cardId);
    if (column.id !== target.columnId) {
      return without.length === column.cardIds.length ? column : { ...column, cardIds: without };
    }
    const index = Math.max(0, Math.min(target.index, without.length));
    return { ...column, cardIds: [...without.slice(0, index), cardId, ...without.slice(index)] };
  });
}

/** Whether dropping at `target` would leave the card where it is. */
export function isNoOp(board: Board, cardId: string, target: DropTarget): boolean {
  const column = board.find((c) => c.id === target.columnId);
  if (column === undefined) return true;
  const current = column.cardIds.indexOf(cardId);
  return current !== -1 && current === Math.min(target.index, column.cardIds.length - 1);
}

/**
 * Where to draw the drop indicator in a column showing `cardIds` (the dragged card still in
 * place): before the card at the returned position, or after the last card when it equals
 * `cardIds.length`. `target.index` counts cards without the dragged one, so it is mapped back
 * to a display position here.
 */
export function indicatorPosition(
  cardIds: readonly string[],
  draggedId: string,
  targetIndex: number,
): number {
  const others = cardIds.filter((id) => id !== draggedId);
  const before = others[targetIndex];
  return before === undefined ? cardIds.length : cardIds.indexOf(before);
}

/** Distance from a scroll area's edge at which dragging starts to scroll it. */
export const AUTO_SCROLL_EDGE = 56;
/** Fastest auto-scroll, in points per tick. */
export const AUTO_SCROLL_MAX_STEP = 18;

/**
 * How far to scroll this tick for a pointer at `pointer` over an area from `start` spanning
 * `size`: negative near the start, positive near the end, faster closer to the edge, and 0 in
 * the middle.
 */
export function autoScrollStep(pointer: number, start: number, size: number): number {
  if (size <= 0) return 0;
  const edge = Math.min(AUTO_SCROLL_EDGE, size / 2);
  const fromStart = pointer - start;
  const fromEnd = start + size - pointer;
  if (fromStart < edge) {
    const depth = Math.min(1, (edge - fromStart) / edge);
    return -Math.ceil(depth * AUTO_SCROLL_MAX_STEP);
  }
  if (fromEnd < edge) {
    const depth = Math.min(1, (edge - fromEnd) / edge);
    return Math.ceil(depth * AUTO_SCROLL_MAX_STEP);
  }
  return 0;
}

/** A card's place inside its column's scrolled content. */
export interface CardOffset {
  id: string;
  top: number;
  height: number;
}

/** Where each card sits, using measured heights and `estimate` for cards not yet rendered. */
export function cardOffsets(
  ids: readonly string[],
  heights: ReadonlyMap<string, number>,
  estimate: number,
  gap: number,
): { offsets: CardOffset[]; total: number } {
  let top = 0;
  const offsets = ids.map((id) => {
    const height = heights.get(id) ?? estimate;
    const offset = { id, top, height };
    top += height + gap;
    return offset;
  });
  return { offsets, total: offsets.length === 0 ? 0 : top - gap };
}

/**
 * The cards to render for a scroll position: those overlapping the viewport plus `overscan` on
 * each side. `last` is exclusive.
 */
export function visibleWindow(
  offsets: readonly CardOffset[],
  scrollTop: number,
  viewport: number,
  overscan: number,
): { first: number; last: number } {
  if (offsets.length === 0) return { first: 0, last: 0 };
  const bottom = scrollTop + viewport;
  let first = offsets.findIndex((card) => card.top + card.height > scrollTop);
  if (first === -1) first = offsets.length - 1;
  let last = first;
  while (last < offsets.length && (offsets[last]?.top ?? 0) < bottom) last += 1;
  return {
    first: Math.max(0, first - overscan),
    last: Math.min(offsets.length, Math.max(last, first + 1) + overscan),
  };
}

/**
 * Window rectangles for every card of a column, rendered or not, so a drop lands correctly in a
 * virtualized list. `contentY` is the window position of the scrolled content's top.
 */
export function virtualCardLayouts(
  offsets: readonly CardOffset[],
  contentY: number,
  x: number,
  width: number,
): CardLayout[] {
  return offsets.map((card) => ({
    id: card.id,
    rect: { x, y: contentY + card.top, width, height: card.height },
  }));
}
