import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
} from "react";
import { Pressable, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { cx } from "../cx";
import { Text } from "./Text";

export type ToastTone = "info" | "success" | "danger";

export interface ToastInput {
  message: string;
  tone?: ToastTone;
  /** Milliseconds before it disappears. */
  duration?: number;
}

export interface ToastItem {
  id: number;
  message: string;
  tone: ToastTone;
  duration: number;
}

/** Toasts shown at once; older ones are dropped. */
export const MAX_TOASTS = 3;
export const DEFAULT_TOAST_DURATION = 4000;

export type ToastAction = { type: "show"; toast: ToastItem } | { type: "dismiss"; id: number };

/** Toast queue (pure; see tests). */
export function toastReducer(state: readonly ToastItem[], action: ToastAction): ToastItem[] {
  switch (action.type) {
    case "show":
      return [...state, action.toast].slice(-MAX_TOASTS);
    case "dismiss":
      return state.filter((t) => t.id !== action.id);
  }
}

interface ToastApi {
  show: (toast: ToastInput) => number;
  dismiss: (id: number) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

/** Shows transient messages: `const toast = useToast(); toast.show({ message })`. */
export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (api === null) {
    throw new Error("useToast must be used inside <ToastProvider>");
  }
  return api;
}

const toneClass: Record<ToastTone, string> = {
  info: "bg-foreground",
  success: "bg-success",
  danger: "bg-danger",
};

const toneText: Record<ToastTone, string> = {
  info: "text-background",
  success: "text-success-foreground",
  danger: "text-danger-foreground",
};

function ToastView({ toast, onDismiss }: { toast: ToastItem; onDismiss: (id: number) => void }) {
  useEffect(() => {
    const timer = setTimeout(() => onDismiss(toast.id), toast.duration);
    return () => clearTimeout(timer);
  }, [toast.id, toast.duration, onDismiss]);
  return (
    <Pressable
      accessibilityRole="alert"
      accessibilityLiveRegion={toast.tone === "danger" ? "assertive" : "polite"}
      onPress={() => onDismiss(toast.id)}
      className={cx("w-full max-w-md rounded-lg px-4 py-3", toneClass[toast.tone])}
    >
      <Text className={toneText[toast.tone]}>{toast.message}</Text>
    </Pressable>
  );
}

/** Hosts the toast stack above the app content, clear of the home indicator. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, dispatch] = useReducer(toastReducer, []);
  const nextId = useRef(1);
  const insets = useSafeAreaInsets();

  const dismiss = useCallback((id: number) => dispatch({ type: "dismiss", id }), []);
  const show = useCallback((input: ToastInput) => {
    const id = nextId.current++;
    dispatch({
      type: "show",
      toast: {
        id,
        message: input.message,
        tone: input.tone ?? "info",
        duration: input.duration ?? DEFAULT_TOAST_DURATION,
      },
    });
    return id;
  }, []);
  const api = useMemo(() => ({ show, dismiss }), [show, dismiss]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <View
        pointerEvents="box-none"
        className="absolute left-0 right-0 items-center gap-2 px-4"
        style={{ bottom: insets.bottom + 16 }}
      >
        {toasts.map((toast) => (
          <ToastView key={toast.id} toast={toast} onDismiss={dismiss} />
        ))}
      </View>
    </ToastContext.Provider>
  );
}
