import type { ReactNode } from "react";
import { Pressable, View, type ViewProps } from "react-native";

import { cx } from "../cx";

export interface CardProps extends Omit<ViewProps, "children"> {
  children: ReactNode;
  /** Makes the whole card pressable (e.g. opens the item). */
  onPress?: () => void;
  /** Screen-reader label when pressable. */
  accessibilityLabel?: string;
  className?: string;
}

const base = "rounded-lg border border-border bg-surface-raised p-4";

/** Raised surface for grouping content; pressable when `onPress` is given. */
export function Card({ children, onPress, className, accessibilityLabel, ...rest }: CardProps) {
  if (onPress !== undefined) {
    return (
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={accessibilityLabel}
        onPress={onPress}
        className={cx(base, "active:opacity-80", className)}
        {...rest}
      >
        {children}
      </Pressable>
    );
  }
  return (
    <View accessibilityLabel={accessibilityLabel} className={cx(base, className)} {...rest}>
      {children}
    </View>
  );
}
