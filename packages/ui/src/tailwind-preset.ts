/**
 * Tailwind preset for NativeWind (ADR-0017). Colors point at the theme's CSS variables, so a
 * class such as `bg-background` follows the light/dark theme and `bg-team` follows the current
 * team without `dark:` variants or per-team classes.
 */

import { colorVar } from "./theme";
import {
  breakpoints,
  fontFamily,
  fontSize,
  fontWeight,
  radius,
  semanticColorNames,
  spacing,
} from "./tokens";

const px = (n: number) => `${n}px`;

function colorFromVar(name: string): string {
  return `rgb(var(${colorVar(name)}) / <alpha-value>)`;
}

const colors: Record<string, string> = {
  transparent: "transparent",
  current: "currentColor",
};
for (const name of [...semanticColorNames, "team", "team-foreground"]) {
  colors[name] = colorFromVar(name);
}

/** Tailwind `theme` built from the tokens. Exported for tests. */
export const presetTheme = {
  // Replace (not extend) so only token values exist: no stray `bg-blue-500`.
  colors,
  spacing: Object.fromEntries(Object.entries(spacing).map(([k, v]) => [k, px(v)])),
  borderRadius: {
    ...Object.fromEntries(Object.entries(radius).map(([k, v]) => [k, px(v)])),
    DEFAULT: px(radius.md),
  },
  fontFamily: { sans: [...fontFamily.sans], mono: [...fontFamily.mono] },
  fontSize: Object.fromEntries(
    Object.entries(fontSize).map(([k, [size, lineHeight]]): [string, [string, string]] => [
      k,
      [px(size), px(lineHeight)],
    ]),
  ),
  fontWeight: { ...fontWeight },
  screens: Object.fromEntries(Object.entries(breakpoints).map(([k, v]) => [k, px(v)])),
};

const preset = {
  content: [],
  // Colors switch through CSS variables, not `dark:` variants. "class" lets NativeWind's
  // colorScheme be set manually (a later theme toggle) instead of following only the media query.
  darkMode: "class" as const,
  theme: presetTheme,
};

export default preset;
