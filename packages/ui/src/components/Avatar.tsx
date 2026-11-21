import { useState } from "react";
import { Image, View } from "react-native";

import { cx } from "../cx";
import { Text } from "./Text";

export type AvatarSize = "sm" | "md" | "lg";

const sizeClass: Record<AvatarSize, string> = {
  sm: "h-6 w-6",
  md: "h-10 w-10",
  lg: "h-16 w-16",
};

const textClass: Record<AvatarSize, string> = {
  sm: "text-xs",
  md: "text-sm",
  lg: "text-xl",
};

/**
 * Up to two initials from a display name, upper-cased with Turkish rules ("ilker" → "İ").
 * Empty names give "?".
 */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/u).filter(Boolean);
  const first = words[0];
  if (first === undefined) {
    return "?";
  }
  const last = words.length > 1 ? words[words.length - 1] : undefined;
  const letters = [first, last]
    .filter((w): w is string => w !== undefined)
    .map((w) => Array.from(w)[0] ?? "")
    .join("");
  return letters.toLocaleUpperCase("tr");
}

export interface AvatarProps {
  /** Person's display name; used for initials and the accessibility label. */
  name: string;
  /** Image URL (profile `avatar_url`). Falls back to initials if missing or broken. */
  url?: string | null;
  size?: AvatarSize;
  className?: string;
}

/** Round avatar: the profile image, or initials when there is none. */
export function Avatar({ name, url, size = "md", className }: AvatarProps) {
  const [failed, setFailed] = useState(false);
  const showImage = typeof url === "string" && url.length > 0 && !failed;
  return (
    <View
      accessibilityRole="image"
      accessibilityLabel={name}
      className={cx(
        "items-center justify-center overflow-hidden rounded-full border border-border bg-surface",
        sizeClass[size],
        className,
      )}
    >
      {showImage ? (
        <Image
          testID="avatar-image"
          source={{ uri: url }}
          onError={() => setFailed(true)}
          className="h-full w-full"
          accessibilityIgnoresInvertColors
        />
      ) : (
        <Text className={cx("font-semibold text-muted", textClass[size])}>{initials(name)}</Text>
      )}
    </View>
  );
}
