import { useColorScheme, vars } from "nativewind";
import { createContext, type ReactNode, useContext, useMemo } from "react";
import { StyleSheet, View } from "react-native";

import { teamVars, themeVars } from "./theme";
import type { ColorScheme } from "./tokens";

interface ThemeContextValue {
  scheme: ColorScheme;
  /** NativeWind `vars()` style carrying the theme variables. */
  style: ReturnType<typeof vars> | undefined;
}

const ThemeContext = createContext<ThemeContextValue>({ scheme: "light", style: undefined });

/** The color scheme the nearest {@link ThemeProvider} applies. */
export function useScheme(): ColorScheme {
  return useContext(ThemeContext).scheme;
}

export interface ThemeProviderProps {
  /** Forces a scheme; by default the device setting is followed. */
  scheme?: ColorScheme | undefined;
  /** Team color for `bg-team` / `text-team-foreground`; invalid values use the default. */
  teamColor?: string | null | undefined;
  children: ReactNode;
}

/**
 * Applies the theme's CSS variables to its subtree (web and native, via NativeWind `vars`).
 * Mount once at the root; token classes such as `bg-background` then follow light/dark.
 */
export function ThemeProvider({ scheme, teamColor, children }: ThemeProviderProps) {
  const { colorScheme } = useColorScheme();
  const resolved: ColorScheme = scheme ?? (colorScheme === "dark" ? "dark" : "light");
  const style = useMemo(
    () => vars(themeVars({ scheme: resolved, teamColor })),
    [resolved, teamColor],
  );
  const value = useMemo(() => ({ scheme: resolved, style }), [resolved, style]);
  return (
    <ThemeContext.Provider value={value}>
      <View testID="theme-root" style={[styles.fill, style]}>
        {children}
      </View>
    </ThemeContext.Provider>
  );
}

/**
 * Re-applies the current theme variables. Needed inside portals: react-native-web renders a
 * `Modal` under `<body>`, outside the {@link ThemeProvider} element that defines the variables.
 */
export function ThemeScope({ children }: { children: ReactNode }) {
  const { style } = useContext(ThemeContext);
  return (
    <View testID="theme-scope" style={[styles.fill, style]}>
      {children}
    </View>
  );
}

export interface TeamScopeProps {
  /** The team's color. */
  color: string | null | undefined;
  children: ReactNode;
}

/** Overrides only the team color for its subtree, e.g. a card belonging to another team. */
export function TeamScope({ color, children }: TeamScopeProps) {
  const style = useMemo(() => vars(teamVars(color)), [color]);
  return <View style={style}>{children}</View>;
}

const styles = StyleSheet.create({ fill: { flex: 1 } });
