import { Text } from "@secondbrain/ui";
import { Redirect } from "expo-router";
import { useCallback, useRef, useState } from "react";
import { type HostInstance, Platform, ScrollView, View } from "react-native";
import { GestureDetector, usePanGesture } from "react-native-gesture-handler";
import Animated, {
  type SharedValue,
  useAnimatedStyle,
  useSharedValue,
} from "react-native-reanimated";
import { SafeAreaView } from "react-native-safe-area-context";
import { scheduleOnRN } from "react-native-worklets";

import {
  type Board,
  type ColumnLayout,
  type DropTarget,
  dropTarget,
  indicatorPosition,
  isNoOp,
  moveCard,
  type Rect,
} from "@/lib/dnd/board";

/**
 * Spike T0.6.1 (ADR-0018): drag-and-drop kanban on react-native-web and iOS with Gesture
 * Handler + Reanimated only. Development-only; the real kanban is T2.1.5.
 *
 * - Native: long press (250 ms) lifts a card, so columns still scroll with a normal swipe.
 *   Web: a mouse drag of a few pixels lifts it.
 * - The lifted card is drawn in an overlay at the board root (not clipped by column
 *   scroll views) and follows the pointer on the UI thread.
 * - Where it lands is computed by pure functions (`@/lib/dnd/board`) from rectangles
 *   measured when the drag starts.
 */
export default function KanbanSpike() {
  if (!__DEV__) {
    return <Redirect href="/" />;
  }
  return <KanbanBoard />;
}

const initialBoard: Board = [
  { id: "todo", title: "Yapılacak", cardIds: ["t1", "t2", "t3", "t4"] },
  { id: "doing", title: "Yapılıyor", cardIds: ["t5", "t6"] },
  { id: "done", title: "Bitti", cardIds: ["t7"] },
];

const titles: Record<string, string> = {
  t1: "Süt al",
  t2: "Rapor taslağı",
  t3: "Diş hekimi randevusu",
  t4: "Kitap: 3. bölüm",
  t5: "Kanban spike'ı",
  t6: "Haftalık plan",
  t7: "Fatura öde",
};

const IS_WEB = Platform.OS === "web";

/** Column width in points. */
const COLUMN_WIDTH = 256;

/** Long-press delay on touch devices before a card lifts. */
const LIFT_DELAY_MS = 250;

type Measurable = HostInstance | null;

function measure(view: Measurable): Promise<Rect | null> {
  return new Promise((resolve) => {
    if (view === null) {
      resolve(null);
      return;
    }
    view.measureInWindow((x, y, width, height) => resolve({ x, y, width, height }));
  });
}

interface Drag {
  cardId: string;
  /** Card rectangle relative to the board root when the drag started. */
  origin: Rect;
}

function KanbanBoard() {
  const [board, setBoard] = useState(initialBoard);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [target, setTarget] = useState<DropTarget | null>(null);
  const [log, setLog] = useState<string[]>([]);

  const rootRef = useRef<HostInstance>(null);
  const columnRefs = useRef(new Map<string, HostInstance>());
  const cardRefs = useRef(new Map<string, HostInstance>());
  const layouts = useRef<ColumnLayout[]>([]);
  const targetRef = useRef<DropTarget | null>(null);
  const boardRef = useRef(board);
  boardRef.current = board;

  const translateX = useSharedValue(0);
  const translateY = useSharedValue(0);

  const measureBoard = useCallback(async (cardId: string) => {
    const [root, ...columns] = await Promise.all([
      measure(rootRef.current),
      ...boardRef.current.map(async (column) => ({
        id: column.id,
        rect: await measure(columnRefs.current.get(column.id) ?? null),
        cards: await Promise.all(
          column.cardIds.map(async (id) => ({
            id,
            rect: await measure(cardRefs.current.get(id) ?? null),
          })),
        ),
      })),
    ]);
    const measured = columns as {
      id: string;
      rect: Rect | null;
      cards: { id: string; rect: Rect | null }[];
    }[];
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
    const card = layouts.current.flatMap((c) => c.cards).find((c) => c.id === cardId);
    if (root === null || root === undefined || card === undefined) return;
    setDrag({
      cardId,
      origin: { ...card.rect, x: card.rect.x - root.x, y: card.rect.y - root.y },
    });
  }, []);

  /** Resolves when the rectangles for the current drag are measured. */
  const measuring = useRef<Promise<void>>(Promise.resolve());

  const start = useCallback(
    (cardId: string) => {
      measuring.current = measureBoard(cardId);
      return measuring.current;
    },
    [measureBoard],
  );

  const move = useCallback((cardId: string, x: number, y: number) => {
    const next = dropTarget({ x, y }, layouts.current, cardId);
    const current = targetRef.current;
    if (next?.columnId !== current?.columnId || next?.index !== current?.index) {
      targetRef.current = next;
      setTarget(next);
    }
  }, []);

  const end = useCallback(async (cardId: string, canceled: boolean, x: number, y: number) => {
    // A quick drag can end before the rectangles measured at lift are known.
    await measuring.current;
    const final = dropTarget({ x, y }, layouts.current, cardId);
    if (!canceled && final !== null && !isNoOp(boardRef.current, cardId, final)) {
      setBoard((b) => moveCard(b, cardId, final));
      setLog((l) =>
        [`${titles[cardId] ?? cardId} → ${final.columnId}[${final.index}]`, ...l].slice(0, 5),
      );
    }
    targetRef.current = null;
    setTarget(null);
    setDrag(null);
  }, []);

  return (
    <SafeAreaView className="flex-1 bg-background">
      <View ref={rootRef} className="flex-1" collapsable={false}>
        <View className="gap-1 px-4 pb-2 pt-4">
          <Text variant="heading">Kanban spike</Text>
          <Text tone="muted">
            {Platform.OS === "web" ? "Kartı fareyle sürükle." : "Karta basılı tut ve sürükle."}
          </Text>
        </View>
        <ScrollView horizontal contentContainerClassName="gap-3 px-4 pb-4">
          {board.map((column) => (
            <View
              key={column.id}
              ref={(view) => {
                if (view === null) columnRefs.current.delete(column.id);
                else columnRefs.current.set(column.id, view);
              }}
              collapsable={false}
              testID={`column-${column.id}`}
              className="gap-2 rounded-lg border border-border bg-surface p-3"
              style={{ width: COLUMN_WIDTH }}
            >
              <Text variant="label">
                {column.title} ({column.cardIds.length})
              </Text>
              <ScrollView className="max-h-[480px]" contentContainerClassName="gap-2">
                {column.cardIds.map((cardId, index) => (
                  <View key={cardId} className="gap-2">
                    {target?.columnId === column.id &&
                    indicatorPosition(column.cardIds, drag?.cardId ?? "", target.index) ===
                      index ? (
                      <Indicator />
                    ) : null}
                    <DraggableCard
                      id={cardId}
                      title={titles[cardId] ?? cardId}
                      lifted={drag?.cardId === cardId}
                      translateX={translateX}
                      translateY={translateY}
                      registerRef={(view) => {
                        if (view === null) cardRefs.current.delete(cardId);
                        else cardRefs.current.set(cardId, view);
                      }}
                      onStart={start}
                      onMove={move}
                      onEnd={end}
                    />
                  </View>
                ))}
                {target?.columnId === column.id &&
                indicatorPosition(column.cardIds, drag?.cardId ?? "", target.index) ===
                  column.cardIds.length ? (
                  <Indicator />
                ) : null}
              </ScrollView>
            </View>
          ))}
        </ScrollView>
        <View className="gap-1 px-4 pb-4" testID="move-log">
          {log.map((line, i) => (
            <Text key={`${line}-${i}`} tone="muted">
              {line}
            </Text>
          ))}
        </View>
        {drag !== null ? (
          <Overlay
            origin={drag.origin}
            title={titles[drag.cardId] ?? drag.cardId}
            translateX={translateX}
            translateY={translateY}
          />
        ) : null}
      </View>
    </SafeAreaView>
  );
}

function Indicator() {
  return <View testID="drop-indicator" className="h-1 rounded-full bg-primary" />;
}

function CardBody({ title }: { title: string }) {
  return (
    <View className="rounded-md border border-border bg-surface-raised p-3">
      <Text>{title}</Text>
    </View>
  );
}

function DraggableCard({
  id,
  title,
  lifted,
  translateX,
  translateY,
  registerRef,
  onStart,
  onMove,
  onEnd,
}: {
  id: string;
  title: string;
  lifted: boolean;
  translateX: SharedValue<number>;
  translateY: SharedValue<number>;
  registerRef: (view: HostInstance | null) => void;
  onStart: (id: string) => Promise<void>;
  onMove: (id: string, x: number, y: number) => void;
  onEnd: (id: string, canceled: boolean, x: number, y: number) => Promise<void>;
}) {
  const startDrag = (cardId: string) => {
    void onStart(cardId);
  };
  const endDrag = (cardId: string, canceled: boolean, x: number, y: number) => {
    void onEnd(cardId, canceled, x, y);
  };
  const pan = usePanGesture({
    // No spread here: the worklets Babel plugin rewrites this object literal.
    minDistance: IS_WEB ? 4 : 0,
    activateAfterLongPress: IS_WEB ? 0 : LIFT_DELAY_MS,
    onActivate: () => {
      "worklet";
      translateX.value = 0;
      translateY.value = 0;
      scheduleOnRN(startDrag, id);
    },
    onUpdate: (event) => {
      "worklet";
      translateX.value = event.translationX;
      translateY.value = event.translationY;
      scheduleOnRN(onMove, id, event.absoluteX, event.absoluteY);
    },
    onDeactivate: (event) => {
      "worklet";
      scheduleOnRN(endDrag, id, event.canceled, event.absoluteX, event.absoluteY);
    },
  });
  return (
    <GestureDetector gesture={pan}>
      <View
        ref={registerRef}
        collapsable={false}
        testID={`card-${id}`}
        style={{ opacity: lifted ? 0.3 : 1 }}
      >
        <CardBody title={title} />
      </View>
    </GestureDetector>
  );
}

function Overlay({
  origin,
  title,
  translateX,
  translateY,
}: {
  origin: Rect;
  title: string;
  translateX: SharedValue<number>;
  translateY: SharedValue<number>;
}) {
  const style = useAnimatedStyle(() => ({
    transform: [
      { translateX: translateX.value },
      { translateY: translateY.value },
      { scale: 1.03 },
    ],
  }));
  return (
    <Animated.View
      pointerEvents="none"
      testID="drag-overlay"
      style={[
        {
          position: "absolute",
          left: origin.x,
          top: origin.y,
          width: origin.width,
          shadowOpacity: 0.2,
          shadowRadius: 8,
          shadowOffset: { width: 0, height: 4 },
          elevation: 6,
        },
        style,
      ]}
    >
      <CardBody title={title} />
    </Animated.View>
  );
}
