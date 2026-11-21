import { Text as RNText, type TextProps as RNTextProps } from "react-native";

import { cx } from "../cx";

export type TextVariant = "title" | "heading" | "body" | "label" | "caption";
export type TextTone = "default" | "muted" | "primary" | "danger" | "success";

const variantClass: Record<TextVariant, string> = {
  title: "text-2xl font-semibold",
  heading: "text-lg font-semibold",
  body: "text-base",
  label: "text-sm font-medium",
  caption: "text-xs",
};

const toneClass: Record<TextTone, string> = {
  default: "text-foreground",
  muted: "text-muted",
  primary: "text-primary",
  danger: "text-danger",
  success: "text-success",
};

export interface TextProps extends RNTextProps {
  variant?: TextVariant;
  tone?: TextTone;
  className?: string;
}

/** Text in the type scale. Titles and headings are exposed as headers to screen readers. */
export function Text({
  variant = "body",
  tone = "default",
  className,
  accessibilityRole,
  ...rest
}: TextProps) {
  const role =
    accessibilityRole ?? (variant === "title" || variant === "heading" ? "header" : undefined);
  return (
    <RNText
      accessibilityRole={role}
      className={cx(variantClass[variant], toneClass[tone], className)}
      {...rest}
    />
  );
}
