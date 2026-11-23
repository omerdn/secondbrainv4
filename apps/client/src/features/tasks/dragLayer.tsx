import type { ReactNode } from "react";
import { Platform } from "react-native";
import { GestureDetector, usePanGesture } from "react-native-gesture-handler";
import Animated, {
  type SharedValue,
  useAnimatedStyle,
  useSharedValue,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

import type { Rect } from "@/lib/dnd/board";

const IS_WEB = Platform.OS === "web";
/** Long-press delay on touch devices before a card lifts (ADR-0018). */
const LIFT_DELAY_MS = 250;

/** How far the lifted card has moved, updated on the UI thread. */
export interface DragOffset {
  x: SharedValue<number>;
  y: SharedValue<number>;
}

export function useDragOffset(): DragOffset {
  return { x: useSharedValue(0), y: useSharedValue(0) };
}

/** Makes its child draggable: a mouse drag on web, a long press and drag on touch. */
export function DragHandle({
  id,
  offset,
  onStart,
  onMove,
  onEnd,
  children,
}: {
  id: string;
  offset: DragOffset;
  onStart: (id: string) => Promise<void>;
  onMove: (id: string, x: number, y: number) => void;
  onEnd: (id: string, canceled: boolean, x: number, y: number) => Promise<void>;
  children: ReactNode;
}) {
  const { x: translateX, y: translateY } = offset;
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
  return <GestureDetector gesture={pan}>{children}</GestureDetector>;
}

/** The lifted card, drawn at the board root so column scroll views do not clip it. */
export function DragOverlay({
  origin,
  offset,
  children,
}: {
  origin: Rect;
  offset: DragOffset;
  children: ReactNode;
}) {
  const { x: translateX, y: translateY } = offset;
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
      {children}
    </Animated.View>
  );
}
