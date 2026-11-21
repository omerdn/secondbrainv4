import { describe, expect, it } from "vitest";

import preset, { presetTheme } from "../src/tailwind-preset";
import {
  contrastRatio,
  parseHex,
  readableForeground,
  relativeLuminance,
  rgbChannels,
  themeVars,
} from "../src/theme";
import { DEFAULT_TEAM_COLOR, semanticColorNames, spacing, themes } from "../src/tokens";

const lum = (hex: string) => relativeLuminance(parseHex(hex) ?? [0, 0, 0]);

describe("parseHex", () => {
  it("accepts #rgb and #rrggbb in any case", () => {
    expect(parseHex("#4F46E5")).toEqual([79, 70, 229]);
    expect(parseHex("#fff")).toEqual([255, 255, 255]);
    expect(parseHex(" #000000 ")).toEqual([0, 0, 0]);
  });

  it("rejects anything else", () => {
    for (const bad of ["", "fff", "#ffff", "#gggggg", "rgb(0,0,0)", "#12345", "red"]) {
      expect(parseHex(bad), bad).toBeNull();
    }
  });
});

describe("contrast", () => {
  it("matches the WCAG reference values", () => {
    expect(contrastRatio(lum("#ffffff"), lum("#000000"))).toBeCloseTo(21, 5);
    expect(contrastRatio(lum("#777777"), lum("#ffffff"))).toBeCloseTo(4.48, 2);
  });

  it("picks dark text on light team colors and white on dark ones", () => {
    expect(readableForeground("#fde047")).toBe("#030712");
    expect(readableForeground("#1e3a8a")).toBe("#ffffff");
  });

  it("falls back to the default team color for invalid input", () => {
    expect(readableForeground("nope")).toBe(readableForeground(DEFAULT_TEAM_COLOR));
  });
});

describe("themes", () => {
  it.each(["light", "dark"] as const)("%s theme keeps body text readable (WCAG AA)", (scheme) => {
    const t = themes[scheme];
    expect(contrastRatio(lum(t.foreground), lum(t.background))).toBeGreaterThanOrEqual(7);
    expect(contrastRatio(lum(t.muted), lum(t.background))).toBeGreaterThanOrEqual(4.5);
    for (const role of ["primary", "danger", "success", "warning"] as const) {
      expect(
        contrastRatio(lum(t[`${role}-foreground`]), lum(t[role])),
        `${scheme} ${role}`,
      ).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("light and dark differ", () => {
    expect(themes.light.background).not.toBe(themes.dark.background);
  });
});

describe("themeVars", () => {
  it("defines every semantic color plus the team pair", () => {
    const vars = themeVars({ scheme: "light" });
    for (const name of semanticColorNames) {
      expect(vars[`--color-${name}`]).toBe(rgbChannels(themes.light[name]));
    }
    expect(vars["--color-team"]).toBe(rgbChannels(DEFAULT_TEAM_COLOR));
    expect(vars["--color-team-foreground"]).toBeDefined();
  });

  it("uses a valid team color and derives a readable foreground", () => {
    const vars = themeVars({ scheme: "dark", teamColor: "#fde047" });
    expect(vars["--color-team"]).toBe("253 224 71");
    expect(vars["--color-team-foreground"]).toBe(rgbChannels("#030712"));
  });

  it("ignores an invalid team color", () => {
    for (const teamColor of ["javascript:alert(1)", "", null, undefined, "#12"]) {
      expect(themeVars({ scheme: "light", teamColor })["--color-team"]).toBe(
        rgbChannels(DEFAULT_TEAM_COLOR),
      );
    }
  });

  it("switches values with the scheme", () => {
    expect(themeVars({ scheme: "light" })["--color-background"]).not.toBe(
      themeVars({ scheme: "dark" })["--color-background"],
    );
  });
});

describe("tailwind preset", () => {
  it("maps every color to its CSS variable with alpha support", () => {
    expect(presetTheme.colors["background"]).toBe("rgb(var(--color-background) / <alpha-value>)");
    expect(presetTheme.colors["team-foreground"]).toBe(
      "rgb(var(--color-team-foreground) / <alpha-value>)",
    );
    for (const name of semanticColorNames) {
      expect(presetTheme.colors[name]).toContain(`--color-${name}`);
    }
  });

  it("derives spacing and type scale from the tokens", () => {
    expect(presetTheme.spacing["4"]).toBe(`${spacing[4]}px`);
    expect(presetTheme.fontSize["base"]).toEqual(["16px", "24px"]);
    expect(preset.theme).toBe(presetTheme);
  });
});

describe("teamVars", () => {
  it("contains only the team pair", async () => {
    const { teamVars } = await import("../src/theme");
    expect(Object.keys(teamVars("#1e3a8a")).sort()).toEqual([
      "--color-team",
      "--color-team-foreground",
    ]);
    expect(teamVars("#1e3a8a")["--color-team-foreground"]).toBe("255 255 255");
  });
});
