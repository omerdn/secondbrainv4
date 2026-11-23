import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type {
  Board,
  BoardCard,
  BoardColumn,
  MoveBoardTaskRequest,
  UpdateBoardColumnRequest,
} from "@secondbrain/api-types";
import { Button, EmptyState, Input, Sheet, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type HostInstance, Pressable, ScrollView, View } from "react-native";

import {
  type ColumnLayout,
  type DropTarget,
  dropTarget,
  indicatorPosition,
  isNoOp,
  type Rect,
} from "@/lib/dnd/board";

import {
  applyMove,
  cardStepTarget,
  columnStepTarget,
  moveRequest,
  pressOpensCard,
  toDndBoard,
} from "./boardModel";
import { DragHandle, type DragOffset, DragOverlay, useDragOffset } from "./dragLayer";

const BOARD_KEY = ["board"];
const COLUMN_WIDTH = 256;

function measure(view: HostInstance | null): Promise<Rect | null> {
  return new Promise((resolve) => {
    if (view === null) {
      resolve(null);
      return;
    }
    view.measureInWindow((x, y, width, height) => {
      resolve({ x, y, width, height });
    });
  });
}

interface Drag {
  card: BoardCard;
  /** Card rectangle relative to the board root when the drag started. */
  origin: Rect;
}

function CardBody({ card }: { card: BoardCard }) {
  const { t } = useTranslation();
  return (
    <View className="gap-1 rounded-md border border-border bg-surface-raised p-3">
      <Text>{card.title}</Text>
      <View className="flex-row justify-between gap-2">
        <Text variant="caption" tone="muted">
          {t("board.progress", { done: card.subtasks_done, total: card.subtasks_total })}
        </Text>
        {card.due_date !== null ? (
          <Text variant="caption" tone="muted">
            {card.due_date}
          </Text>
        ) : null}
      </View>
    </View>
  );
}

function DraggableCard({
  card,
  lifted,
  offset,
  registerRef,
  onStart,
  onMove,
  onEnd,
  onOpen,
  onMenu,
}: {
  card: BoardCard;
  lifted: boolean;
  offset: DragOffset;
  registerRef: (view: HostInstance | null) => void;
  onStart: (id: string) => Promise<void>;
  onMove: (id: string, x: number, y: number) => void;
  onEnd: (id: string, canceled: boolean, x: number, y: number) => Promise<void>;
  onOpen: () => void;
  onMenu: () => void;
}) {
  const { t } = useTranslation();
  const id = card.task_id;
  return (
    <View className="gap-1">
      <DragHandle id={id} offset={offset} onStart={onStart} onMove={onMove} onEnd={onEnd}>
        <View
          ref={registerRef}
          collapsable={false}
          testID={`card-${id}`}
          style={{ opacity: lifted ? 0.3 : 1 }}
        >
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={t("board.openCard", { title: card.title })}
            onPress={onOpen}
          >
            <CardBody card={card} />
          </Pressable>
        </View>
      </DragHandle>
      <View className="self-end">
        <Button
          title={t("board.move")}
          accessibilityLabel={t("board.moveCard", { title: card.title })}
          variant="ghost"
          size="sm"
          onPress={onMenu}
        />
      </View>
    </View>
  );
}

function Indicator() {
  return <View testID="drop-indicator" className="h-1 rounded-full bg-primary" />;
}

/** Keyboard and screen-reader alternative to dragging (ADR-0018). */
function MoveCardSheet({
  board,
  card,
  columnName,
  onMove,
  onClose,
}: {
  board: Board;
  card: BoardCard | null;
  columnName: (column: BoardColumn) => string;
  onMove: (taskId: string, target: DropTarget) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  if (card === null) {
    return null;
  }
  const id = card.task_id;
  const current = board.columns.find((c) => c.cards.some((x) => x.task_id === id));
  const step = (direction: "up" | "down") => cardStepTarget(board, id, direction);
  const stepTo = (direction: "up" | "down") => {
    const target = step(direction);
    if (target !== null) {
      onMove(id, target);
    }
  };
  return (
    <Sheet visible onClose={onClose} title={card.title} closeLabel={t("tags.close")}>
      <View className="gap-2">
        <View className="flex-row gap-2">
          <Button
            title={t("board.up")}
            variant="secondary"
            size="sm"
            disabled={step("up") === null}
            onPress={() => {
              stepTo("up");
            }}
          />
          <Button
            title={t("board.down")}
            variant="secondary"
            size="sm"
            disabled={step("down") === null}
            onPress={() => {
              stepTo("down");
            }}
          />
        </View>
        <Text variant="label">{t("board.moveToColumn")}</Text>
        {board.columns
          .filter((column) => column.id !== current?.id)
          .map((column) => (
            <Button
              key={column.id}
              title={columnName(column)}
              variant="secondary"
              onPress={() => {
                onMove(id, { columnId: column.id, index: column.cards.length });
                onClose();
              }}
            />
          ))}
      </View>
    </Sheet>
  );
}

type ColumnChange =
  | { type: "create"; name: string }
  | { type: "update"; id: string; body: UpdateBoardColumnRequest }
  | { type: "delete"; id: string };

function ColumnSheet({
  board,
  column,
  busy,
  onChange,
  onClose,
}: {
  board: Board;
  column: BoardColumn;
  busy: boolean;
  onChange: (change: ColumnChange) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(column.name);
  const ids = board.columns.map((c) => c.id);
  const index = ids.indexOf(column.id);
  const left = columnStepTarget(ids, index, "left");
  const right = columnStepTarget(ids, index, "right");
  const trimmed = name.trim();
  return (
    <Sheet visible onClose={onClose} title={column.name} closeLabel={t("tags.close")}>
      <View className="gap-3">
        <Input label={t("board.columnName")} value={name} maxLength={60} onChangeText={setName} />
        <Button
          title={t("board.rename")}
          disabled={busy || trimmed === "" || trimmed === column.name}
          onPress={() => {
            onChange({ type: "update", id: column.id, body: { name: trimmed } });
          }}
        />
        <View className="flex-row gap-2">
          <Button
            title={t("board.left")}
            variant="secondary"
            size="sm"
            disabled={busy || left === null}
            onPress={() => {
              if (left !== null) {
                onChange({ type: "update", id: column.id, body: { after_column_id: left } });
              }
            }}
          />
          <Button
            title={t("board.right")}
            variant="secondary"
            size="sm"
            disabled={busy || right === null}
            onPress={() => {
              if (right !== null) {
                onChange({ type: "update", id: column.id, body: { after_column_id: right } });
              }
            }}
          />
        </View>
        <Text variant="caption" tone="muted">
          {t("board.deleteHint")}
        </Text>
        <Button
          title={t("board.deleteColumn")}
          variant="ghost"
          disabled={busy}
          onPress={() => {
            onChange({ type: "delete", id: column.id });
            onClose();
          }}
        />
      </View>
    </Sheet>
  );
}

function BoardColumns({ board }: { board: Board }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const router = useRouter();
  const [drag, setDrag] = useState<Drag | null>(null);
  const [target, setTarget] = useState<DropTarget | null>(null);
  const [menuCardId, setMenuCardId] = useState<string | null>(null);
  const [editingColumnId, setEditingColumnId] = useState<string | null>(null);
  const [newColumn, setNewColumn] = useState("");
  const [failed, setFailed] = useState(false);

  const rootRef = useRef<HostInstance>(null);
  const columnRefs = useRef(new Map<string, HostInstance>());
  const cardRefs = useRef(new Map<string, HostInstance>());
  const layouts = useRef<ColumnLayout[]>([]);
  const targetRef = useRef<DropTarget | null>(null);
  const boardRef = useRef(board);
  boardRef.current = board;
  const measuring = useRef<Promise<void>>(Promise.resolve());
  const offset = useDragOffset();
  const dragging = useRef(false);
  const lastDropAt = useRef<number | null>(null);

  const move = useIdempotentMutation<Board, { taskId: string; body: MoveBoardTaskRequest }>(
    ({ taskId, body }, idempotencyKey) => api.board.moveTask(taskId, body, { idempotencyKey }),
    {
      onSuccess: (next) => {
        client.setQueryData(BOARD_KEY, next);
      },
      onError: async () => {
        setFailed(true);
        await client.invalidateQueries({ queryKey: BOARD_KEY });
      },
    },
  );
  const columnChange = useIdempotentMutation<Board | void, ColumnChange>(
    (change, idempotencyKey) => {
      const options = { idempotencyKey };
      switch (change.type) {
        case "create":
          return api.board.createColumn({ name: change.name }, options);
        case "update":
          return api.board.updateColumn(change.id, change.body, options);
        case "delete":
          return api.board.deleteColumn(change.id, options);
      }
    },
    {
      onMutate: () => {
        setFailed(false);
      },
      onSuccess: async (_next, change) => {
        if (change.type === "create") {
          setNewColumn("");
        }
        await client.invalidateQueries({ queryKey: BOARD_KEY });
      },
      onError: () => {
        setFailed(true);
      },
    },
  );

  const place = useCallback(
    (taskId: string, drop: DropTarget) => {
      const current = boardRef.current;
      const body = moveRequest(current, taskId, drop);
      if (body === null || isNoOp(toDndBoard(current), taskId, drop)) {
        return;
      }
      setFailed(false);
      // Shown at once; a failed save reloads the real order.
      client.setQueryData(BOARD_KEY, applyMove(current, taskId, drop));
      move.mutate({ taskId, body });
    },
    [client, move],
  );

  const measureBoard = useCallback(async (cardId: string) => {
    const root = await measure(rootRef.current);
    const measured = await Promise.all(
      boardRef.current.columns.map(async (column) => ({
        id: column.id,
        rect: await measure(columnRefs.current.get(column.id) ?? null),
        cards: await Promise.all(
          column.cards.map(async (card) => ({
            id: card.task_id,
            rect: await measure(cardRefs.current.get(card.task_id) ?? null),
          })),
        ),
      })),
    );
    layouts.current = measured.flatMap((column) =>
      column.rect === null
        ? []
        : [
            {
              id: column.id,
              rect: column.rect,
              cards: column.cards.flatMap((card) =>
                card.rect === null ? [] : [{ id: card.id, rect: card.rect }],
              ),
            },
          ],
    );
    const rect = layouts.current.flatMap((c) => c.cards).find((c) => c.id === cardId)?.rect;
    const card = boardRef.current.columns.flatMap((c) => c.cards).find((c) => c.task_id === cardId);
    if (root === null || rect === undefined || card === undefined) {
      return;
    }
    setDrag({ card, origin: { ...rect, x: rect.x - root.x, y: rect.y - root.y } });
  }, []);

  const start = useCallback(
    (cardId: string) => {
      dragging.current = true;
      measuring.current = measureBoard(cardId);
      return measuring.current;
    },
    [measureBoard],
  );
  const hover = useCallback((cardId: string, x: number, y: number) => {
    const next = dropTarget({ x, y }, layouts.current, cardId);
    const current = targetRef.current;
    if (next?.columnId !== current?.columnId || next?.index !== current?.index) {
      targetRef.current = next;
      setTarget(next);
    }
  }, []);
  const end = useCallback(
    async (cardId: string, canceled: boolean, x: number, y: number) => {
      // A quick drag can end before the rectangles measured at lift are known.
      await measuring.current;
      const final = dropTarget({ x, y }, layouts.current, cardId);
      if (!canceled && final !== null) {
        place(cardId, final);
      }
      targetRef.current = null;
      setTarget(null);
      setDrag(null);
      dragging.current = false;
      lastDropAt.current = Date.now();
    },
    [place],
  );

  const columnName = (column: BoardColumn) => (column.is_inbox ? t("board.inbox") : column.name);
  const menuCard =
    board.columns.flatMap((c) => c.cards).find((c) => c.task_id === menuCardId) ?? null;
  const editingColumn = board.columns.find((c) => c.id === editingColumnId) ?? null;
  const draggedId = drag?.card.task_id ?? "";

  return (
    <View ref={rootRef} collapsable={false} className="gap-3">
      <ScrollView horizontal contentContainerClassName="gap-3 pb-2">
        {board.columns.map((column) => {
          const cardIds = column.cards.map((card) => card.task_id);
          const indicator =
            target?.columnId === column.id
              ? indicatorPosition(cardIds, draggedId, target.index)
              : null;
          return (
            <View
              key={column.id}
              ref={(view) => {
                if (view === null) columnRefs.current.delete(column.id);
                else columnRefs.current.set(column.id, view);
              }}
              collapsable={false}
              testID={`column-${column.id}`}
              className="gap-2 self-start rounded-lg border border-border bg-surface p-3"
              style={{ width: COLUMN_WIDTH }}
            >
              <View className="min-h-8 flex-row items-center justify-between gap-2">
                <Text variant="label" accessibilityRole="header" className="flex-1">
                  {columnName(column)} ({column.cards.length})
                </Text>
                {column.is_inbox ? null : (
                  <Button
                    title={t("board.editColumn")}
                    accessibilityLabel={t("board.editColumnNamed", { name: column.name })}
                    variant="ghost"
                    size="sm"
                    onPress={() => {
                      setEditingColumnId(column.id);
                    }}
                  />
                )}
              </View>
              <ScrollView className="max-h-[480px]" contentContainerClassName="gap-2">
                {column.cards.map((card, index) => (
                  <View key={card.task_id} className="gap-2">
                    {indicator === index ? <Indicator /> : null}
                    <DraggableCard
                      card={card}
                      lifted={draggedId === card.task_id}
                      offset={offset}
                      registerRef={(view) => {
                        if (view === null) cardRefs.current.delete(card.task_id);
                        else cardRefs.current.set(card.task_id, view);
                      }}
                      onStart={start}
                      onMove={hover}
                      onEnd={end}
                      onOpen={() => {
                        if (!pressOpensCard(dragging.current, lastDropAt.current, Date.now())) {
                          return;
                        }
                        const href = `/tasks/${encodeURIComponent(card.task_id)}` as Href;
                        router.push(href);
                      }}
                      onMenu={() => {
                        setMenuCardId(card.task_id);
                      }}
                    />
                  </View>
                ))}
                {indicator === column.cards.length ? <Indicator /> : null}
                {column.cards.length === 0 ? (
                  <Text variant="caption" tone="muted">
                    {t("board.emptyColumn")}
                  </Text>
                ) : null}
              </ScrollView>
            </View>
          );
        })}
        <View className="gap-2 self-start" style={{ width: COLUMN_WIDTH }}>
          <Input
            label={t("board.newColumn")}
            value={newColumn}
            maxLength={60}
            onChangeText={setNewColumn}
          />
          <Button
            title={t("board.addColumn")}
            variant="secondary"
            size="sm"
            disabled={columnChange.isPending || newColumn.trim() === ""}
            onPress={() => {
              columnChange.mutate({ type: "create", name: newColumn.trim() });
            }}
          />
        </View>
      </ScrollView>
      {failed ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("board.failed")}
        </Text>
      ) : null}
      {drag !== null ? (
        <DragOverlay origin={drag.origin} offset={offset}>
          <CardBody card={drag.card} />
        </DragOverlay>
      ) : null}
      <MoveCardSheet
        board={board}
        card={menuCard}
        columnName={columnName}
        onMove={place}
        onClose={() => {
          setMenuCardId(null);
        }}
      />
      {editingColumn !== null ? (
        <ColumnSheet
          key={editingColumn.id}
          board={board}
          column={editingColumn}
          busy={columnChange.isPending}
          onChange={(change) => {
            columnChange.mutate(change);
          }}
          onClose={() => {
            setEditingColumnId(null);
          }}
        />
      ) : null}
    </View>
  );
}

/** The personal kanban (PRD G6, ADR-0018). */
export function KanbanBoard() {
  const { t } = useTranslation();
  const api = useApi();
  const board = useQuery({ queryKey: BOARD_KEY, queryFn: api.board.get });
  if (board.isPending) {
    return <Text tone="muted">{t("common.loading")}</Text>;
  }
  if (board.isError) {
    return (
      <EmptyState
        title={t("board.loadFailed")}
        action={{
          title: t("items.detail.retry"),
          onPress: () => {
            void board.refetch();
          },
        }}
      />
    );
  }
  return <BoardColumns board={board.data} />;
}
