import { useWindowDimensions } from "react-native";

import { breakpoints } from "./tokens";

/** `compact` = phone (< md), `medium` = tablet / narrow window, `expanded` ≥ lg. */
export type Breakpoint = "compact" | "medium" | "expanded";

/** Pure mapping from a window width to a {@link Breakpoint}. */
export function breakpointFor(width: number): Breakpoint {
  if (width >= breakpoints.lg) return "expanded";
  if (width >= breakpoints.md) return "medium";
  return "compact";
}

/** Current breakpoint; re-renders when the window crosses one (rotation, resize). */
export function useBreakpoint(): Breakpoint {
  return breakpointFor(useWindowDimensions().width);
}
