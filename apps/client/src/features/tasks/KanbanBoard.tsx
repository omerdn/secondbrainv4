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
import { type ComponentRef, type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type HostInstance, Pressable, ScrollView, View } from "react-native";

import {
  autoScrollStep,
  type CardLayout,
  cardOffsets,
  type ColumnLayout,
  type DropTarget,
  dropTarget,
  indicatorPosition,
  isNoOp,
  type Rect,
  virtualCardLayouts,
  visibleWindow,
} from "@/lib/dnd/board";

import {
  applyMove,
  cardStepTarget,
  columnStepTarget,
  moveRequest,
  pressOpensCard,
  toDndBoard,
} from "./boardModel";
import {
  DragHandle,
  type DragOffset,
  DragOverlay,
  useDragOffset,
  useWebPointer,
} from "./dragLayer";

const BOARD_KEY = ["board"];
const COLUMN_WIDTH = 256;
const COLUMN_MAX_HEIGHT = 480;
/** A card with its "move" button, until it is measured. */
const CARD_HEIGHT_ESTIMATE = 106;
const CARD_GAP = 8;
/** Cards kept rendered beyond each edge of a column's viewport. */
const OVERSCAN = 4;
const AUTO_SCROLL_TICK_MS = 16;
/** Ticks between re-measuring the board while it auto-scrolls. */
const REMEASURE_TICKS = 4;

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

/** What the board needs from a column while dragging. */
interface ColumnHandle {
  /** Every card's window rectangle, rendered or not. */
  cards: () => Promise<CardLayout[]>;
  viewport: () => Promise<Rect | null>;
  /** Scrolls by `delta`; returns whether the position changed. */
  scrollBy: (delta: number) => boolean;
}

/**
 * A column's cards, rendering only those near the viewport (T2.1.7). Rows are positioned
 * absolutely from measured heights, so the card being dragged can stay mounted (its gesture
 * lives on it) even after auto-scroll carries it out of the window.
 */
function ColumnCards({
  cards,
  draggedId,
  indicator,
  emptyLabel,
  register,
  renderCard,
}: {
  cards: readonly BoardCard[];
  draggedId: string;
  /** Display position of the drop indicator in this column. */
  indicator: number | null;
  emptyLabel: string;
  register: (handle: ColumnHandle | null) => void;
  renderCard: (card: BoardCard) => ReactNode;
}) {
  const scrollRef = useRef<ComponentRef<typeof ScrollView>>(null);
  const viewportRef = useRef<HostInstance>(null);
  const heights = useRef(new Map<string, number>());
  const scrollTop = useRef(0);
  const viewportHeight = useRef(COLUMN_MAX_HEIGHT);
  const idsRef = useRef<string[]>([]);
  const [, rerender] = useState(0);
  const [windowTop, setWindowTop] = useState(0);

  const ids = cards.map((card) => card.task_id);
  idsRef.current = ids;
  const layout = () => cardOffsets(idsRef.current, heights.current, CARD_HEIGHT_ESTIMATE, CARD_GAP);
  const { offsets, total } = layout();
  const range = visibleWindow(offsets, windowTop, COLUMN_MAX_HEIGHT, OVERSCAN);
  const draggedIndex = ids.indexOf(draggedId);
  const shown = offsets.filter(
    (_, index) => (index >= range.first && index < range.last) || index === draggedIndex,
  );
  const byId = new Map(cards.map((card) => [card.task_id, card]));

  const follow = (top: number) => {
    scrollTop.current = top;
    // Re-render only when the set of cards could change.
    setWindowTop((current) =>
      Math.abs(current - top) >= CARD_HEIGHT_ESTIMATE / 2 ? top : current,
    );
  };

  useEffect(() => {
    register({
      cards: async () => {
        const viewport = await measure(viewportRef.current);
        if (viewport === null) {
          return [];
        }
        const { offsets: current } = cardOffsets(
          idsRef.current,
          heights.current,
          CARD_HEIGHT_ESTIMATE,
          CARD_GAP,
        );
        return virtualCardLayouts(
          current,
          viewport.y - scrollTop.current,
          viewport.x,
          viewport.width,
        );
      },
      viewport: () => measure(viewportRef.current),
      scrollBy: (delta) => {
        const { total: height } = cardOffsets(
          idsRef.current,
          heights.current,
          CARD_HEIGHT_ESTIMATE,
          CARD_GAP,
        );
        const limit = Math.max(0, height - viewportHeight.current);
        const next = Math.max(0, Math.min(limit, scrollTop.current + delta));
        if (next === scrollTop.current) {
          return false;
        }
        scrollRef.current?.scrollTo({ y: next, animated: false });
        follow(next);
        return true;
      },
    });
    return () => {
      register(null);
    };
    // `register` is recreated by the parent; the handle reads everything through refs.
  }, []);

  if (cards.length === 0) {
    return (
      <View ref={viewportRef} collapsable={false} className="gap-2">
        {indicator !== null ? <Indicator /> : null}
        <Text variant="caption" tone="muted">
          {emptyLabel}
        </Text>
      </View>
    );
  }
  const indicatorTop =
    indicator === null
      ? null
      : indicator >= offsets.length
        ? total + 2
        : Math.max(0, (offsets[indicator]?.top ?? 0) - CARD_GAP / 2 - 2);
  return (
    <View ref={viewportRef} collapsable={false}>
      <ScrollView
        ref={scrollRef}
        style={{ maxHeight: COLUMN_MAX_HEIGHT }}
        scrollEventThrottle={16}
        onLayout={(event) => {
          viewportHeight.current = event.nativeEvent.layout.height;
        }}
        onScroll={(event) => {
          follow(event.nativeEvent.contentOffset.y);
        }}
      >
        <View style={{ height: total + (indicator === offsets.length ? CARD_GAP : 0) }}>
          {shown.map((row) => {
            const card = byId.get(row.id);
            return card === undefined ? null : (
              <View
                key={row.id}
                style={{ position: "absolute", top: row.top, left: 0, right: 0 }}
                onLayout={(event) => {
                  const height = event.nativeEvent.layout.height;
                  if (heights.current.get(row.id) !== height) {
                    heights.current.set(row.id, height);
                    rerender((version) => version + 1);
                  }
                }}
              >
                {renderCard(card)}
              </View>
            );
          })}
          {indicatorTop !== null ? (
            <View style={{ position: "absolute", top: indicatorTop, left: 0, right: 0 }}>
              <Indicator />
            </View>
          ) : null}
        </View>
      </ScrollView>
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
  const boardScrollRef = useRef<ComponentRef<typeof ScrollView>>(null);
  const boardViewportRef = useRef<HostInstance>(null);
  const boardScrollX = useRef(0);
  const boardContentWidth = useRef(0);
  const boardWidth = useRef(0);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const columnHandles = useRef(new Map<string, ColumnHandle>());
  const columnRefs = useRef(new Map<string, HostInstance>());
  const cardRefs = useRef(new Map<string, HostInstance>());
  const layouts = useRef<ColumnLayout[]>([]);
  const targetRef = useRef<DropTarget | null>(null);
  const boardRef = useRef(board);
  boardRef.current = board;
  const measuring = useRef<Promise<void>>(Promise.resolve());
  const offset = useDragOffset();
  const dragging = useRef(false);
  const dragIdRef = useRef("");
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

  const refreshLayouts = useCallback(async () => {
    const measured = await Promise.all(
      boardRef.current.columns.map(async (column) => ({
        id: column.id,
        rect: await measure(columnRefs.current.get(column.id) ?? null),
        cards: (await columnHandles.current.get(column.id)?.cards()) ?? [],
      })),
    );
    layouts.current = measured.flatMap((column) =>
      column.rect === null ? [] : [{ id: column.id, rect: column.rect, cards: column.cards }],
    );
  }, []);

  const measureBoard = useCallback(
    async (cardId: string) => {
      const root = await measure(rootRef.current);
      await refreshLayouts();
      const rect = await measure(cardRefs.current.get(cardId) ?? null);
      const card = boardRef.current.columns
        .flatMap((c) => c.cards)
        .find((c) => c.task_id === cardId);
      if (root === null || rect === null || card === undefined) {
        return;
      }
      setDrag({ card, origin: { ...rect, x: rect.x - root.x, y: rect.y - root.y } });
    },
    [refreshLayouts],
  );

  const start = useCallback(
    (cardId: string) => {
      dragging.current = true;
      pointer.current = null;
      measuring.current = measureBoard(cardId);
      return measuring.current;
    },
    [measureBoard],
  );
  const hover = useCallback((cardId: string, x: number, y: number) => {
    pointer.current = { x, y };
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
      // The end event can repeat an older position than the last move the card followed.
      const at = pointer.current ?? { x, y };
      pointer.current = null;
      // Auto-scroll may have moved everything since the last measurement.
      await refreshLayouts();
      const final = dropTarget(at, layouts.current, cardId);
      if (!canceled && final !== null) {
        place(cardId, final);
      }
      targetRef.current = null;
      setTarget(null);
      setDrag(null);
      dragging.current = false;
      lastDropAt.current = Date.now();
    },
    [place, refreshLayouts],
  );

  const columnName = (column: BoardColumn) => (column.is_inbox ? t("board.inbox") : column.name);
  const menuCard =
    board.columns.flatMap((c) => c.cards).find((c) => c.task_id === menuCardId) ?? null;
  const editingColumn = board.columns.find((c) => c.id === editingColumnId) ?? null;
  const draggedId = drag?.card.task_id ?? "";
  dragIdRef.current = draggedId;

  const followPointer = useCallback(
    (x: number, y: number) => {
      if (!dragging.current) {
        return;
      }
      pointer.current = { x, y };
      const id = dragIdRef.current;
      if (id !== "") {
        hover(id, x, y);
      }
    },
    [hover],
  );
  useWebPointer(draggedId !== "", followPointer);

  // Near an edge, the board scrolls sideways and the column under the card scrolls up or down.
  useEffect(() => {
    if (draggedId === "") {
      return;
    }
    let ticks = 0;
    let boardRect: Rect | null = null;
    void measure(boardViewportRef.current).then((rect) => {
      boardRect = rect;
    });
    const timer = setInterval(() => {
      const at = pointer.current;
      if (at === null || boardRect === null) {
        return;
      }
      let moved = false;
      const dx = autoScrollStep(at.x, boardRect.x, boardRect.width);
      if (dx !== 0) {
        const limit = Math.max(0, boardContentWidth.current - boardWidth.current);
        const next = Math.max(0, Math.min(limit, boardScrollX.current + dx));
        if (next !== boardScrollX.current) {
          boardScrollX.current = next;
          boardScrollRef.current?.scrollTo({ x: next, animated: false });
          moved = true;
        }
      }
      const columnId = targetRef.current?.columnId;
      const handle = columnId === undefined ? undefined : columnHandles.current.get(columnId);
      const column = layouts.current.find((c) => c.id === columnId);
      if (handle !== undefined && column !== undefined) {
        const dy = autoScrollStep(at.y, column.rect.y, column.rect.height);
        if (dy !== 0 && handle.scrollBy(dy)) {
          moved = true;
        }
      }
      ticks += 1;
      if (moved && ticks % REMEASURE_TICKS === 0) {
        void refreshLayouts().then(() => {
          const latest = pointer.current;
          if (latest !== null) {
            hover(draggedId, latest.x, latest.y);
          }
        });
      }
    }, AUTO_SCROLL_TICK_MS);
    return () => {
      clearInterval(timer);
    };
  }, [draggedId, hover, refreshLayouts]);

  return (
    <View ref={rootRef} collapsable={false} className="gap-3">
      <View ref={boardViewportRef} collapsable={false}>
        <ScrollView
          ref={boardScrollRef}
          horizontal
          contentContainerClassName="gap-3 pb-2"
          scrollEventThrottle={16}
          onLayout={(event) => {
            boardWidth.current = event.nativeEvent.layout.width;
          }}
          onContentSizeChange={(width) => {
            boardContentWidth.current = width;
          }}
          onScroll={(event) => {
            boardScrollX.current = event.nativeEvent.contentOffset.x;
          }}
        >
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
                <ColumnCards
                  cards={column.cards}
                  draggedId={draggedId}
                  indicator={indicator}
                  emptyLabel={t("board.emptyColumn")}
                  register={(handle) => {
                    if (handle === null) columnHandles.current.delete(column.id);
                    else columnHandles.current.set(column.id, handle);
                  }}
                  renderCard={(card) => (
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
                  )}
                />
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
      </View>
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
