import type { ReactNode } from "react";
import { View } from "react-native";

import { cx } from "../cx";
import { Button } from "./Button";
import { Text } from "./Text";

export interface EmptyStateProps {
  title: string;
  description?: string;
  /** Optional illustration or icon above the title. */
  icon?: ReactNode;
  /** Primary action, e.g. "Create task". */
  action?: { title: string; onPress: () => void };
  className?: string;
}

/** Placeholder for an empty list or screen, with an optional next step. */
export function EmptyState({ title, description, icon, action, className }: EmptyStateProps) {
  return (
    <View className={cx("items-center justify-center gap-3 px-6 py-10", className)}>
      {icon}
      <Text variant="heading" className="text-center">
        {title}
      </Text>
      {description !== undefined ? (
        <Text tone="muted" className="max-w-sm text-center">
          {description}
        </Text>
      ) : null}
      {action !== undefined ? (
        <Button title={action.title} onPress={action.onPress} className="mt-2" />
      ) : null}
    </View>
  );
}
