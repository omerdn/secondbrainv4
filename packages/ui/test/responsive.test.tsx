import { render, screen } from "@testing-library/react-native";
import { Text, useWindowDimensions } from "react-native";

import { breakpointFor, useBreakpoint } from "../src/responsive";
import { presetTheme } from "../src/tailwind-preset";
import { breakpoints } from "../src/tokens";

jest.mock("react-native/Libraries/Utilities/useWindowDimensions", () => ({
  __esModule: true,
  default: jest.fn(),
}));

describe("breakpointFor", () => {
  it("maps widths at and around each breakpoint", () => {
    expect(breakpointFor(0)).toBe("compact");
    expect(breakpointFor(breakpoints.md - 1)).toBe("compact");
    expect(breakpointFor(breakpoints.md)).toBe("medium");
    expect(breakpointFor(breakpoints.lg - 1)).toBe("medium");
    expect(breakpointFor(breakpoints.lg)).toBe("expanded");
    expect(breakpointFor(2560)).toBe("expanded");
  });

  it("uses the same values as the Tailwind screens", () => {
    expect(presetTheme.screens).toEqual({ md: "768px", lg: "1024px", xl: "1280px" });
  });
});

describe("useBreakpoint", () => {
  function Probe() {
    return <Text>{useBreakpoint()}</Text>;
  }

  it("follows the window width", async () => {
    const dims = jest.mocked(useWindowDimensions);
    dims.mockReturnValue({ width: 390, height: 844, scale: 3, fontScale: 1 });
    const view = await render(<Probe />);
    expect(screen.getByText("compact")).toBeOnTheScreen();
    dims.mockReturnValue({ width: 1440, height: 900, scale: 2, fontScale: 1 });
    await view.rerender(<Probe />);
    expect(screen.getByText("expanded")).toBeOnTheScreen();
  });
});
