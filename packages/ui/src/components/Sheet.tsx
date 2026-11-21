import type { ReactNode } from "react";
import { Modal, Pressable, useWindowDimensions, View } from "react-native";

import { cx } from "../cx";
import { ThemeScope } from "../ThemeProvider";
import { Button } from "./Button";
import { Text } from "./Text";

/** Width from which the sheet becomes a centered dialog (tablets, desktop web). */
export const DIALOG_MIN_WIDTH = 768;

export type SheetLayout = "sheet" | "dialog";

/** Bottom sheet on narrow screens, centered dialog on wide ones (pure; see tests). */
export function sheetLayout(windowWidth: number): SheetLayout {
  return windowWidth >= DIALOG_MIN_WIDTH ? "dialog" : "sheet";
}

export interface SheetProps {
  visible: boolean;
  /** Called on backdrop press, the close button and the Android back button. */
  onClose: () => void;
  title: string;
  children: ReactNode;
  /** Optional footer, e.g. confirm/cancel buttons. */
  footer?: ReactNode;
  /** Label of the close button (localised by the caller, T0.4.7). */
  closeLabel?: string;
}

/**
 * Modal surface: slides up from the bottom on phones, appears as a dialog on wide screens.
 * Built on React Native's `Modal`, which react-native-web supports (no DOM-only library).
 */
export function Sheet({
  visible,
  onClose,
  title,
  children,
  footer,
  closeLabel = "Close",
}: SheetProps) {
  const layout = sheetLayout(useWindowDimensions().width);
  const dialog = layout === "dialog";
  return (
    <Modal
      visible={visible}
      transparent
      animationType={dialog ? "fade" : "slide"}
      onRequestClose={onClose}
      statusBarTranslucent
    >
      <ThemeScope>
        <View className={cx("flex-1", dialog ? "items-center justify-center p-6" : "justify-end")}>
          <Pressable
            testID="sheet-backdrop"
            accessibilityRole="button"
            accessibilityLabel={closeLabel}
            onPress={onClose}
            className="absolute bottom-0 left-0 right-0 top-0 bg-foreground/40"
          />
          <View
            accessibilityViewIsModal
            aria-modal
            role="dialog"
            aria-label={title}
            className={cx(
              "gap-4 bg-surface-raised p-6",
              dialog ? "w-full max-w-lg rounded-xl" : "rounded-t-xl pb-10",
            )}
          >
            <View className="flex-row items-center justify-between gap-4">
              <Text variant="heading" className="flex-1">
                {title}
              </Text>
              <Button title={closeLabel} variant="ghost" size="sm" onPress={onClose} />
            </View>
            <View>{children}</View>
            {footer !== undefined ? (
              <View className="flex-row justify-end gap-2">{footer}</View>
            ) : null}
          </View>
        </View>
      </ThemeScope>
    </Modal>
  );
}
