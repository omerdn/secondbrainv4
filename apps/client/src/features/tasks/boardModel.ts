import type { Board, BoardCard, MoveBoardTaskRequest } from "@secondbrain/api-types";

import { type Board as DndBoard, type DropTarget, moveCard } from "@/lib/dnd/board";

/** The server board in the shape the drag-and-drop logic works on. */
export function toDndBoard(board: Board): DndBoard {
  return board.columns.map((column) => ({
    id: column.id,
    title: column.name,
    cardIds: column.cards.map((card) => card.task_id),
  }));
}

/** The request for a card dropped at `target`: the server is told which card it lands after. */
export function moveRequest(
  board: Board,
  taskId: string,
  target: DropTarget,
): MoveBoardTaskRequest | null {
  const moved = moveCard(toDndBoard(board), taskId, target);
  const column = moved.find((c) => c.id === target.columnId);
  const at = column?.cardIds.indexOf(taskId) ?? -1;
  if (column === undefined || at === -1) {
    return null;
  }
  const after = column.cardIds[at - 1];
  return after === undefined
    ? { column_id: column.id }
    : { column_id: column.id, after_task_id: after };
}

/** The board as it will look after the move, shown until the server answers. */
export function applyMove(board: Board, taskId: string, target: DropTarget): Board {
  const cards = new Map<string, BoardCard>(
    board.columns.flatMap((column) => column.cards.map((card) => [card.task_id, card] as const)),
  );
  const moved = moveCard(toDndBoard(board), taskId, target);
  return {
    columns: board.columns.map((column) => {
      const ids = moved.find((c) => c.id === column.id)?.cardIds ?? [];
      return {
        ...column,
        cards: ids.flatMap((id) => {
          const card = cards.get(id);
          return card === undefined ? [] : [card];
        }),
      };
    }),
  };
}

/**
 * The column to place column `index` after when moving it one step. The inbox stays first, so
 * `null` means the move is not possible.
 */
export function columnStepTarget(
  columnIds: readonly string[],
  index: number,
  direction: "left" | "right",
): string | null {
  if (index < 1 || index >= columnIds.length) {
    return null;
  }
  const after = direction === "left" ? index - 2 : index + 1;
  return after < 0 ? null : (columnIds[after] ?? null);
}

/** The target that moves a card one step within its column; `null` at either end. */
export function cardStepTarget(
  board: Board,
  taskId: string,
  direction: "up" | "down",
): DropTarget | null {
  const column = board.columns.find((c) => c.cards.some((card) => card.task_id === taskId));
  if (column === undefined) {
    return null;
  }
  const at = column.cards.findIndex((card) => card.task_id === taskId);
  const index = direction === "up" ? at - 1 : at + 1;
  return index < 0 || index >= column.cards.length ? null : { columnId: column.id, index };
}

/** How long after a drop a press on a card still belongs to the drag. */
export const DROP_CLICK_MS = 350;

/**
 * Whether a press should open the card. Releasing the mouse after a drag also fires a click on
 * the card, which must not navigate away from the board.
 */
export function pressOpensCard(dragging: boolean, lastDropAt: number | null, now: number): boolean {
  return !dragging && (lastDropAt === null || now - lastDropAt >= DROP_CLICK_MS);
}
