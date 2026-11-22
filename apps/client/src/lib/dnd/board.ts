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
