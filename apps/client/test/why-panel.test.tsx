import type { AccessExplanation, AccessReason } from "@secondbrain/api-types";
import { ThemeProvider } from "@secondbrain/ui";
import { act, fireEvent, render, screen } from "@testing-library/react-native";

import { i18n } from "@/lib/i18n";
import { type Loadable, WhyPanel } from "@/lib/items";

const explanation = (
  reasons: AccessReason[],
  permissions: AccessExplanation["permissions"] = ["view"],
): Loadable<AccessExplanation> => ({ status: "ready", value: { permissions, reasons } });

async function open(
  data: Loadable<AccessExplanation>,
  titles: ReadonlyMap<string, string> = new Map(),
) {
  const onOpenItem = jest.fn();
  await render(
    <ThemeProvider>
      <WhyPanel explanation={data} titles={titles} onOpenItem={onOpenItem} />
    </ThemeProvider>,
  );
  await fireEvent.press(screen.getByRole("button", { name: "Bunu neden görüyorum?" }));
  return onOpenItem;
}

beforeEach(async () => {
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("why am I seeing this (V11)", () => {
  it("stays closed until asked", async () => {
    await render(
      <ThemeProvider>
        <WhyPanel
          explanation={explanation([{ kind: "owner" }])}
          titles={new Map()}
          onOpenItem={() => undefined}
        />
      </ThemeProvider>,
    );
    expect(screen.queryByText("Bu nesnenin sahibisin.")).toBeNull();
  });

  it.each<[AccessReason, string]>([
    [{ kind: "owner" }, "Bu nesnenin sahibisin."],
    [{ kind: "assignee" }, "Bu nesnenin sorumlususun."],
    [{ kind: "watcher" }, "Bu nesneyi izliyorsun."],
    [{ kind: "editor" }, "Bu defterin düzenleyicisisin."],
    [{ kind: "viewer" }, "Bu defterin görüntüleyicisisin."],
    [{ kind: "team_role", team_id: "x", role: "admin" }, "Takımda admin rolündesin."],
    [{ kind: "team_role", team_id: "x", role: "manager" }, "Takımda yönetici rolündesin."],
    [{ kind: "team_role", team_id: "x", role: "member" }, "Takımda üye rolündesin."],
    [{ kind: "team_role", team_id: "x", role: "guest" }, "Takımda misafir rolündesin."],
    [{ kind: "notebook", notebook_id: "n1" }, "Bu sayfanın defterine erişimin var."],
    [{ kind: "linked", item_id: "t1" }, "Bağlı olduğu bir nesneyi görebiliyorsun."],
  ])("explains %j", async (reason, text) => {
    await open(explanation([reason]));
    expect(screen.getByText(text)).toBeTruthy();
  });

  it("lists every path and what the viewer may do", async () => {
    await open(explanation([{ kind: "assignee" }, { kind: "watcher" }], ["view", "edit"]));
    expect(screen.getByText("Bu nesnenin sorumlususun.")).toBeTruthy();
    expect(screen.getByText("Bu nesneyi izliyorsun.")).toBeTruthy();
    expect(screen.getByText("Görüntüleme, Düzenleme")).toBeTruthy();
  });

  it("names and opens the linked item it came through", async () => {
    const onOpenItem = await open(
      explanation([{ kind: "linked", item_id: "t1" }]),
      new Map([["t1", "Tatil planı"]]),
    );
    expect(screen.getByText("“Tatil planı” nesnesine bağlı olduğu için görüyorsun.")).toBeTruthy();
    await fireEvent.press(screen.getByRole("button", { name: "Nesneyi aç" }));
    expect(onOpenItem).toHaveBeenCalledWith("t1");
  });

  it("opens the notebook", async () => {
    const onOpenItem = await open(explanation([{ kind: "notebook", notebook_id: "n1" }]));
    await fireEvent.press(screen.getByRole("button", { name: "Defteri aç" }));
    expect(onOpenItem).toHaveBeenCalledWith("n1");
  });

  it("shows only what the server reported: no hint of a hidden link (V9)", async () => {
    await open(explanation([{ kind: "watcher" }]), new Map([["hidden", "Gizli başlık"]]));
    expect(screen.queryByText(/Gizli başlık/u)).toBeNull();
    expect(screen.queryByText(/bağlı/iu)).toBeNull();
    expect(screen.queryByRole("button", { name: "Nesneyi aç" })).toBeNull();
  });

  it("shows loading and a retry", async () => {
    await open({ status: "loading" });
    expect(screen.getByText("Yükleniyor")).toBeTruthy();
  });

  it("retries a failed explanation", async () => {
    const retry = jest.fn();
    await open({ status: "error", retry });
    await fireEvent.press(screen.getByRole("button", { name: "Tekrar dene" }));
    expect(retry).toHaveBeenCalledTimes(1);
  });
});
