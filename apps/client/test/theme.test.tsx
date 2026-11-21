import { TeamScope, ThemeProvider, themeVars, useScheme } from "@secondbrain/ui";
import { render, screen } from "@testing-library/react-native";
import { Text } from "react-native";

// NativeWind stores `vars()` in its own style context, invisible in props. Pass the variables
// through unchanged so the test sees exactly what the provider applies; the CSS side is
// verified on web and iOS.
jest.mock("nativewind", () => ({
  ...jest.requireActual<object>("nativewind"),
  vars: (v: Record<string, string>) => v,
}));

function SchemeLabel() {
  return <Text>{useScheme()}</Text>;
}

/** NativeWind `vars()` styles flattened into one object. */
function rootVars(): Record<string, unknown> {
  const style: unknown = screen.getByTestId("theme-root").props.style;
  const list = (Array.isArray(style) ? style.flat(Infinity) : [style]) as unknown[];
  return Object.assign({}, ...list.filter((s) => typeof s === "object" && s !== null)) as Record<
    string,
    unknown
  >;
}

describe("ThemeProvider", () => {
  it.each(["light", "dark"] as const)("applies the %s theme variables", async (scheme) => {
    await render(
      <ThemeProvider scheme={scheme}>
        <SchemeLabel />
      </ThemeProvider>,
    );
    expect(screen.getByText(scheme)).toBeOnTheScreen();
    const expected = themeVars({ scheme });
    const applied = JSON.stringify(rootVars());
    expect(applied).toContain(expected["--color-background"] ?? "missing");
  });

  it("applies the team color", async () => {
    await render(
      <ThemeProvider scheme="light" teamColor="#fde047">
        <TeamScope color="#1e3a8a">
          <Text>inside</Text>
        </TeamScope>
      </ThemeProvider>,
    );
    expect(JSON.stringify(rootVars())).toContain("253 224 71");
    expect(screen.getByText("inside")).toBeOnTheScreen();
  });
});
