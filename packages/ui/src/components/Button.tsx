import type { ReactNode } from "react";
import { ActivityIndicator, Pressable, type PressableProps, Text, View } from "react-native";

import { cx } from "../cx";

/** `dangerOutline`: a reversible but consequential action (e.g. sign out), quieter than `danger`. */
export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "dangerOutline";
export type ButtonSize = "sm" | "md";

const container: Record<ButtonVariant, string> = {
  primary: "bg-primary",
  secondary: "bg-surface-raised border border-border",
  ghost: "bg-transparent",
  danger: "bg-danger",
  dangerOutline: "bg-transparent border border-danger",
};

const label: Record<ButtonVariant, string> = {
  primary: "text-primary-foreground",
  secondary: "text-foreground",
  ghost: "text-primary",
  danger: "text-danger-foreground",
  dangerOutline: "text-danger",
};

const sizeClass: Record<ButtonSize, string> = {
  sm: "min-h-8 px-3 py-1.5",
  md: "min-h-10 px-4 py-2",
};

export interface ButtonProps extends Omit<PressableProps, "children" | "style"> {
  title: string;
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Shows a spinner and blocks presses (e.g. while a mutation runs). */
  loading?: boolean;
  /** Optional element before the title (an icon). */
  leading?: ReactNode;
  className?: string;
}

/**
 * Pressable button. Feedback comes from the pressed state only; there is no hover-only
 * behaviour (ADR-0012), so it works the same with touch and mouse.
 */
export function Button({
  title,
  variant = "primary",
  size = "md",
  loading = false,
  disabled,
  leading,
  className,
  ...rest
}: ButtonProps) {
  const inactive = disabled === true || loading;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={title}
      accessibilityState={{ disabled: inactive, busy: loading }}
      disabled={inactive}
      className={cx(
        "flex-row items-center justify-center gap-2 rounded-md active:opacity-80",
        container[variant],
        sizeClass[size],
        inactive && "opacity-50",
        className,
      )}
      {...rest}
    >
      {loading ? (
        <ActivityIndicator testID="button-spinner" size="small" />
      ) : leading !== undefined ? (
        <View>{leading}</View>
      ) : null}
      <Text className={cx(size === "sm" ? "text-sm" : "text-base", "font-medium", label[variant])}>
        {title}
      </Text>
    </Pressable>
  );
}
