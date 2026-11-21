import { act, fireEvent, render, screen } from "@testing-library/react-native";
import { Text as RNText } from "react-native";
import { SafeAreaProvider } from "react-native-safe-area-context";

import {
  Avatar,
  Button,
  Card,
  DIALOG_MIN_WIDTH,
  EmptyState,
  initials,
  Input,
  MAX_TOASTS,
  Sheet,
  sheetLayout,
  TagChip,
  Text,
  type ToastItem,
  ToastProvider,
  toastReducer,
  useToast,
} from "../src";

const metrics = {
  frame: { x: 0, y: 0, width: 390, height: 844 },
  insets: { top: 47, left: 0, right: 0, bottom: 34 },
};

describe("Text", () => {
  it("exposes titles and headings as headers", async () => {
    await render(
      <>
        <Text variant="title">Title</Text>
        <Text>Body</Text>
      </>,
    );
    expect(screen.getByRole("header")).toHaveTextContent("Title");
    expect(screen.queryAllByRole("header")).toHaveLength(1);
  });
});

describe("Button", () => {
  it("calls onPress", async () => {
    const onPress = jest.fn();
    await render(<Button title="Save" onPress={onPress} />);
    await fireEvent.press(screen.getByRole("button", { name: "Save" }));
    expect(onPress).toHaveBeenCalledTimes(1);
  });

  it("blocks presses and reports busy while loading", async () => {
    const onPress = jest.fn();
    await render(<Button title="Save" loading onPress={onPress} />);
    const button = screen.getByRole("button", { name: "Save" });
    await fireEvent.press(button);
    expect(onPress).not.toHaveBeenCalled();
    expect(button).toBeBusy();
    expect(button).toBeDisabled();
    expect(screen.getByTestId("button-spinner")).toBeOnTheScreen();
  });

  it("does nothing when disabled", async () => {
    const onPress = jest.fn();
    await render(<Button title="Save" disabled onPress={onPress} />);
    await fireEvent.press(screen.getByRole("button", { name: "Save" }));
    expect(onPress).not.toHaveBeenCalled();
  });
});

describe("Input", () => {
  it("is labelled and reports typing", async () => {
    const onChangeText = jest.fn();
    await render(<Input label="E-mail" hint="We never share it" onChangeText={onChangeText} />);
    await fireEvent.changeText(screen.getByLabelText("E-mail"), "ada@example.test");
    expect(onChangeText).toHaveBeenCalledWith("ada@example.test");
    expect(screen.getByText("We never share it")).toBeOnTheScreen();
  });

  it("shows the error instead of the hint and marks the field invalid", async () => {
    await render(<Input label="E-mail" hint="hint" error="must be an e-mail address" />);
    expect(screen.getByText("must be an e-mail address")).toBeOnTheScreen();
    expect(screen.queryByText("hint")).toBeNull();
    expect(screen.getByLabelText("E-mail").props["aria-invalid"]).toBe(true);
  });
});

describe("Card", () => {
  it("is a plain container without onPress", async () => {
    await render(
      <Card>
        <RNText>content</RNText>
      </Card>,
    );
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("is pressable with onPress", async () => {
    const onPress = jest.fn();
    await render(
      <Card onPress={onPress} accessibilityLabel="Open task">
        <RNText>content</RNText>
      </Card>,
    );
    await fireEvent.press(screen.getByRole("button", { name: "Open task" }));
    expect(onPress).toHaveBeenCalled();
  });
});

describe("Sheet", () => {
  it("chooses sheet or dialog by width", () => {
    expect(sheetLayout(390)).toBe("sheet");
    expect(sheetLayout(DIALOG_MIN_WIDTH - 1)).toBe("sheet");
    expect(sheetLayout(DIALOG_MIN_WIDTH)).toBe("dialog");
  });

  it("renders only when visible and closes from the backdrop and the button", async () => {
    const onClose = jest.fn();
    const view = await render(
      <Sheet visible={false} onClose={onClose} title="Filters">
        <RNText>body</RNText>
      </Sheet>,
    );
    expect(screen.queryByText("body")).toBeNull();

    await view.rerender(
      <Sheet visible onClose={onClose} title="Filters" closeLabel="Kapat">
        <RNText>body</RNText>
      </Sheet>,
    );
    expect(screen.getByText("body")).toBeOnTheScreen();
    expect(screen.getByRole("header")).toHaveTextContent("Filters");
    // Hidden from screen readers (the sheet is accessibilityViewIsModal); the close button is
    // the accessible way out.
    await fireEvent.press(screen.getByTestId("sheet-backdrop", { includeHiddenElements: true }));
    await fireEvent.press(screen.getByRole("button", { name: "Kapat" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});

describe("toastReducer", () => {
  const item = (id: number): ToastItem => ({ id, message: `m${id}`, tone: "info", duration: 1 });

  it(`keeps at most ${MAX_TOASTS} toasts, dropping the oldest`, () => {
    let state: ToastItem[] = [];
    for (let id = 1; id <= MAX_TOASTS + 2; id++) {
      state = toastReducer(state, { type: "show", toast: item(id) });
    }
    expect(state.map((t) => t.id)).toEqual([3, 4, 5]);
  });

  it("dismisses by id", () => {
    const state = toastReducer([item(1), item(2)], { type: "dismiss", id: 1 });
    expect(state.map((t) => t.id)).toEqual([2]);
  });
});

describe("Toast", () => {
  beforeEach(() => jest.useFakeTimers());
  afterEach(() => jest.useRealTimers());

  function Trigger() {
    const toast = useToast();
    return (
      <Button title="Notify" onPress={() => toast.show({ message: "Saved", duration: 1000 })} />
    );
  }

  it("shows a message and hides it after its duration", async () => {
    await render(
      <SafeAreaProvider initialMetrics={metrics}>
        <ToastProvider>
          <Trigger />
        </ToastProvider>
      </SafeAreaProvider>,
    );
    await fireEvent.press(screen.getByRole("button", { name: "Notify" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Saved");
    await act(() => jest.advanceTimersByTime(1000));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("can be dismissed by pressing it", async () => {
    await render(
      <SafeAreaProvider initialMetrics={metrics}>
        <ToastProvider>
          <Trigger />
        </ToastProvider>
      </SafeAreaProvider>,
    );
    await fireEvent.press(screen.getByRole("button", { name: "Notify" }));
    await fireEvent.press(screen.getByRole("alert"));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("requires the provider", async () => {
    jest.spyOn(console, "error").mockImplementation(() => undefined);
    await expect(render(<Trigger />)).rejects.toThrow(/ToastProvider/);
  });
});

describe("Avatar", () => {
  it("derives initials with Turkish casing", () => {
    expect(initials("Ada Lovelace")).toBe("AL");
    expect(initials("ilker  şahin")).toBe("İŞ");
    expect(initials("  çağla ")).toBe("Ç");
    expect(initials("Mary Ann Evans")).toBe("ME");
    expect(initials("   ")).toBe("?");
  });

  it("shows initials without a URL and the image with one", async () => {
    const view = await render(<Avatar name="Ada Lovelace" />);
    expect(screen.getByText("AL")).toBeOnTheScreen();
    expect(screen.getByLabelText("Ada Lovelace")).toBeOnTheScreen();

    await view.rerender(<Avatar name="Ada Lovelace" url="https://x.test/a.png" />);
    expect(screen.getByTestId("avatar-image")).toBeOnTheScreen();
  });

  it("falls back to initials when the image fails", async () => {
    await render(<Avatar name="Ada Lovelace" url="https://x.test/broken.png" />);
    await fireEvent(screen.getByTestId("avatar-image"), "error");
    expect(screen.getByText("AL")).toBeOnTheScreen();
  });
});

describe("TagChip", () => {
  it("renders personal tags plainly and team tags distinctly (E3)", async () => {
    await render(
      <>
        <TagChip label="health" />
        <TagChip label="sprint" teamColor="#1e3a8a" />
      </>,
    );
    expect(screen.getByText("health")).toBeOnTheScreen();
    expect(screen.getByText("# sprint")).toBeOnTheScreen();
  });

  it("supports press, selection and removal", async () => {
    const onPress = jest.fn();
    const onRemove = jest.fn();
    await render(
      <TagChip
        label="health"
        selected
        onPress={onPress}
        onRemove={onRemove}
        removeLabel="Kaldır"
      />,
    );
    const tag = screen.getByRole("button", { name: "health" });
    expect(tag).toBeSelected();
    await fireEvent.press(tag);
    await fireEvent.press(screen.getByRole("button", { name: "Kaldır" }));
    expect(onPress).toHaveBeenCalled();
    expect(onRemove).toHaveBeenCalled();
  });
});

describe("EmptyState", () => {
  it("shows title, description and the action", async () => {
    const onPress = jest.fn();
    await render(
      <EmptyState
        title="No tasks yet"
        description="Create your first task."
        action={{ title: "Create task", onPress }}
      />,
    );
    expect(screen.getByRole("header")).toHaveTextContent("No tasks yet");
    expect(screen.getByText("Create your first task.")).toBeOnTheScreen();
    await fireEvent.press(screen.getByRole("button", { name: "Create task" }));
    expect(onPress).toHaveBeenCalled();
  });
});
