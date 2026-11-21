/**
 * Design tokens (ADR-0017). The single source for colors, typography, spacing and radii; the
 * Tailwind preset and the runtime theme are both derived from here.
 */

/** A `#rrggbb` color. */
export type Hex = `#${string}`;

/** Raw palette. Components never use these directly; they use {@link SemanticColors}. */
export const palette = {
  white: "#ffffff",
  black: "#000000",
  gray: {
    50: "#f9fafb",
    100: "#f3f4f6",
    200: "#e5e7eb",
    300: "#d1d5db",
    400: "#9ca3af",
    500: "#6b7280",
    600: "#4b5563",
    700: "#374151",
    800: "#1f2937",
    900: "#111827",
    950: "#030712",
  },
  indigo: { 300: "#a5b4fc", 400: "#818cf8", 500: "#6366f1", 600: "#4f46e5", 700: "#4338ca" },
  red: { 400: "#f87171", 500: "#ef4444", 600: "#dc2626" },
  green: { 400: "#4ade80", 500: "#22c55e", 600: "#16a34a", 700: "#15803d" },
  amber: { 400: "#fbbf24", 500: "#f59e0b", 600: "#d97706" },
} as const satisfies Record<string, Hex | Record<number, Hex>>;

/** Role-based colors; each theme assigns every role. */
export const semanticColorNames = [
  "background",
  "surface",
  "surface-raised",
  "foreground",
  "muted",
  "border",
  "primary",
  "primary-foreground",
  "danger",
  "danger-foreground",
  "success",
  "success-foreground",
  "warning",
  "warning-foreground",
] as const;

export type SemanticColorName = (typeof semanticColorNames)[number];
export type SemanticColors = Record<SemanticColorName, Hex>;

export type ColorScheme = "light" | "dark";

/** Semantic colors per color scheme. */
export const themes: Record<ColorScheme, SemanticColors> = {
  light: {
    background: palette.white,
    surface: palette.gray[50],
    "surface-raised": palette.white,
    foreground: palette.gray[900],
    muted: palette.gray[500],
    border: palette.gray[200],
    primary: palette.indigo[600],
    "primary-foreground": palette.white,
    danger: palette.red[600],
    "danger-foreground": palette.white,
    // green-600 on white is only 3.3:1; 700 passes WCAG AA (5:1).
    success: palette.green[700],
    "success-foreground": palette.white,
    warning: palette.amber[500],
    "warning-foreground": palette.gray[950],
  },
  dark: {
    background: palette.gray[950],
    surface: palette.gray[900],
    "surface-raised": palette.gray[800],
    foreground: palette.gray[50],
    muted: palette.gray[400],
    border: palette.gray[700],
    primary: palette.indigo[400],
    "primary-foreground": palette.gray[950],
    danger: palette.red[400],
    "danger-foreground": palette.gray[950],
    success: palette.green[400],
    "success-foreground": palette.gray[950],
    warning: palette.amber[400],
    "warning-foreground": palette.gray[950],
  },
};

/** Team color used when a team has none (or an invalid one). */
export const DEFAULT_TEAM_COLOR: Hex = palette.indigo[500];

/** Font families. `System` maps to the platform UI font (San Francisco, Roboto, system-ui). */
export const fontFamily = {
  sans: ["System", "ui-sans-serif", "system-ui", "sans-serif"],
  mono: ["ui-monospace", "Menlo", "monospace"],
} as const;

/** Type scale: `[fontSize, lineHeight]` in px. */
export const fontSize = {
  xs: [12, 16],
  sm: [14, 20],
  base: [16, 24],
  lg: [18, 28],
  xl: [20, 28],
  "2xl": [24, 32],
  "3xl": [30, 36],
} as const satisfies Record<string, readonly [number, number]>;

export const fontWeight = {
  regular: "400",
  medium: "500",
  semibold: "600",
  bold: "700",
} as const;

/** Spacing scale in px, on a 4 px grid. Keys follow Tailwind (`p-4` = 16 px). */
export const spacing = {
  0: 0,
  0.5: 2,
  1: 4,
  1.5: 6,
  2: 8,
  3: 12,
  4: 16,
  5: 20,
  6: 24,
  8: 32,
  10: 40,
  12: 48,
  16: 64,
} as const satisfies Record<number, number>;

/** Corner radii in px. */
export const radius = {
  none: 0,
  sm: 4,
  md: 8,
  lg: 12,
  xl: 16,
  full: 9999,
} as const;
