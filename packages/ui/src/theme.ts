/**
 * Runtime theme: turns tokens into CSS variables (ARCHITECTURE §2: team colors are given as CSS
 * variables). Values are space-separated RGB channels (`"79 70 229"`) so Tailwind can apply
 * opacity: `rgb(var(--color-primary) / <alpha-value>)`.
 *
 * Pure functions only; the React side applies the result with NativeWind's `vars()`.
 */

import {
  type ColorScheme,
  DEFAULT_TEAM_COLOR,
  type Hex,
  palette,
  semanticColorNames,
  themes,
} from "./tokens";

/** CSS variable name for a color role, e.g. `--color-primary`. */
export function colorVar(name: string): `--color-${string}` {
  return `--color-${name}`;
}

const HEX = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;

/** Parses `#rgb` or `#rrggbb`; `null` for anything else. */
export function parseHex(value: string): [number, number, number] | null {
  const match = HEX.exec(value.trim());
  const digits = match?.[1];
  if (digits === undefined) {
    return null;
  }
  const full =
    digits.length === 3
      ? digits
          .split("")
          .map((d) => d + d)
          .join("")
      : digits;
  const channel = (i: number) => Number.parseInt(full.slice(i, i + 2), 16);
  return [channel(0), channel(2), channel(4)];
}

/** `"r g b"` channels of a valid hex color. */
export function rgbChannels(hex: string): string {
  const rgb = parseHex(hex);
  if (rgb === null) {
    throw new Error(`invalid hex color: ${hex}`);
  }
  return rgb.join(" ");
}

/** WCAG 2.x relative luminance (0 = black, 1 = white). */
export function relativeLuminance([r, g, b]: readonly [number, number, number]): number {
  const linear = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}

/** WCAG contrast ratio between two luminances (1–21). */
export function contrastRatio(a: number, b: number): number {
  const [hi, lo] = a >= b ? [a, b] : [b, a];
  return (hi + 0.05) / (lo + 0.05);
}

/**
 * Text color for content on a `background` color: whichever of near-black and white contrasts
 * more. Team colors are chosen by users, so this keeps labels on `bg-team` readable.
 */
export function readableForeground(background: string): Hex {
  const rgb = parseHex(background) ?? parseHex(DEFAULT_TEAM_COLOR);
  if (rgb === null) {
    return palette.white;
  }
  const lum = relativeLuminance(rgb);
  const dark = palette.gray[950];
  const darkRgb = parseHex(dark) ?? [0, 0, 0];
  return contrastRatio(lum, relativeLuminance(darkRgb)) >= contrastRatio(lum, 1)
    ? dark
    : palette.white;
}

/** CSS variable map for one theme. */
export type ThemeVars = Record<`--color-${string}`, string>;

export interface ThemeOptions {
  /** Current color scheme. */
  scheme: ColorScheme;
  /** Team color (`#rgb`/`#rrggbb`). Missing or invalid → {@link DEFAULT_TEAM_COLOR}. */
  teamColor?: string | null | undefined;
}

/** All `--color-*` variables for a scheme and an optional team color. */
export function themeVars({ scheme, teamColor }: ThemeOptions): ThemeVars {
  const colors = themes[scheme];
  const vars: ThemeVars = {};
  for (const name of semanticColorNames) {
    vars[colorVar(name)] = rgbChannels(colors[name]);
  }
  return { ...vars, ...teamVars(teamColor) };
}

/**
 * Only the team variables (`--color-team`, `--color-team-foreground`), for a subtree that shows
 * another team than the rest of the screen. Missing or invalid → {@link DEFAULT_TEAM_COLOR}.
 */
export function teamVars(teamColor: string | null | undefined): ThemeVars {
  const team =
    teamColor !== null && teamColor !== undefined && parseHex(teamColor) !== null
      ? teamColor.trim()
      : DEFAULT_TEAM_COLOR;
  return {
    [colorVar("team")]: rgbChannels(team),
    [colorVar("team-foreground")]: rgbChannels(readableForeground(team)),
  };
}
