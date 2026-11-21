import { Pressable, Text, View } from "react-native";

import { cx } from "../cx";
import { TeamScope } from "../ThemeProvider";

export interface TagChipProps {
  label: string;
  /**
   * Team tag: the team's color. Team tags use the team color and a distinct filled style, so
   * they stand apart from personal tags without separate lists (PRD E3). Omit for personal tags.
   */
  teamColor?: string | null;
  /** Selected state, e.g. in a filter bar. */
  selected?: boolean;
  onPress?: () => void;
  /** Shows a remove button. */
  onRemove?: () => void;
  /** Accessibility label of the remove button (localised by the caller, T0.4.7). */
  removeLabel?: string;
}

/** Tag label; personal tags are outlined, team tags are filled with the team color. */
export function TagChip({
  label,
  teamColor,
  selected = false,
  onPress,
  onRemove,
  removeLabel = `Remove ${label}`,
}: TagChipProps) {
  const team = teamColor !== undefined && teamColor !== null;
  const chip = (
    <View
      className={cx(
        "flex-row items-center gap-1 self-start rounded-full px-2 py-0.5",
        team ? "bg-team" : "border bg-surface",
        !team && (selected ? "border-primary" : "border-border"),
        team && selected && "border-2 border-foreground",
      )}
    >
      <Pressable
        disabled={onPress === undefined}
        onPress={onPress}
        accessibilityRole={onPress !== undefined ? "button" : "text"}
        accessibilityState={{ selected }}
        accessibilityLabel={label}
      >
        <Text
          className={cx(
            "text-xs",
            team ? "font-semibold text-team-foreground" : "font-medium text-foreground",
          )}
        >
          {team ? `# ${label}` : label}
        </Text>
      </Pressable>
      {onRemove !== undefined ? (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={removeLabel}
          hitSlop={8}
          onPress={onRemove}
        >
          <Text className={cx("text-xs", team ? "text-team-foreground" : "text-muted")}>✕</Text>
        </Pressable>
      ) : null}
    </View>
  );
  return team ? <TeamScope color={teamColor}>{chip}</TeamScope> : chip;
}
