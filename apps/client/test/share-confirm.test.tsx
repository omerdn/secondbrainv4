import { ApiProvider } from "@secondbrain/api-client/react";
import type { AccessPreview, AccessPreviewRequest } from "@secondbrain/api-types";
import { ThemeProvider } from "@secondbrain/ui";
import { QueryClient } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react-native";

import { i18n } from "@/lib/i18n";
import { ShareConfirmDialog } from "@/lib/share";

import { apiError, fakeServer, json } from "./support/api";

const link: AccessPreviewRequest = { action: "link", page_id: "p1", item_id: "t1" };

const preview = (over: Partial<AccessPreview> = {}): AccessPreview => ({
  items: [{ item_id: "p1", new_viewers: ["u2", "u3"] }],
  people: [
    { id: "u2", display_name: "Ayşe" },
    { id: "u3", display_name: "Mehmet" },
  ],
  ...over,
});

async function setup(
  handler: () => Response | Promise<Response>,
  {
    request = link,
    busy = false,
    knownNames,
  }: {
    request?: AccessPreviewRequest | null;
    busy?: boolean;
    knownNames?: Record<string, string>;
  } = {},
) {
  const server = fakeServer({ "POST /access/preview": handler });
  const onConfirm = jest.fn();
  const onCancel = jest.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = await render(
    <ApiProvider api={server.api} queryClient={client}>
      <ThemeProvider>
        <ShareConfirmDialog
          request={request}
          {...(knownNames !== undefined ? { knownNames } : {})}
          confirmLabel="Bağla"
          onConfirm={onConfirm}
          onCancel={onCancel}
          busy={busy}
        />
      </ThemeProvider>
    </ApiProvider>,
  );
  return { server, onConfirm, onCancel, ui };
}

const confirm = () => screen.getByRole("button", { name: "Bağla" });

beforeEach(async () => {
  await act(async () => {
    await i18n.changeLanguage("tr");
  });
});

describe("share confirmation (V7)", () => {
  it("lists who gains access and sends the action only after approval", async () => {
    const { server, onConfirm } = await setup(() => json(200, preview()));
    expect(await screen.findByText("Ayşe")).toBeTruthy();
    expect(screen.getByText("Mehmet")).toBeTruthy();
    expect(screen.getByText("Bu işlemle şu kişiler 1 nesneyi görebilecek:")).toBeTruthy();
    expect(server.calls[0]?.body).toEqual(link);
    expect(onConfirm).not.toHaveBeenCalled();
    await fireEvent.press(confirm());
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("cannot be confirmed while the answer is pending", async () => {
    let answer: (response: Response) => void = () => undefined;
    const { onConfirm } = await setup(
      () =>
        new Promise<Response>((resolve) => {
          answer = resolve;
        }),
    );
    expect(await screen.findByText("Kimlerin erişeceği kontrol ediliyor…")).toBeTruthy();
    expect(confirm()).toBeDisabled();
    await fireEvent.press(confirm());
    expect(onConfirm).not.toHaveBeenCalled();
    await act(async () => {
      answer(json(200, preview()));
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(confirm()).toBeEnabled();
    });
  });

  it("cannot be confirmed when the preview fails, until a retry succeeds", async () => {
    let fail = true;
    const { onConfirm } = await setup(() =>
      fail ? apiError(500, "internal") : json(200, preview()),
    );
    expect(await screen.findByText("Kimlerin erişeceği görülemedi.")).toBeTruthy();
    expect(confirm()).toBeDisabled();
    await fireEvent.press(confirm());
    expect(onConfirm).not.toHaveBeenCalled();
    fail = false;
    await fireEvent.press(screen.getByRole("button", { name: "Tekrar dene" }));
    expect(await screen.findByText("Ayşe")).toBeTruthy();
    expect(confirm()).toBeEnabled();
  });

  it("says so when nobody gains access, and still allows the action", async () => {
    const { onConfirm } = await setup(() => json(200, { items: [], people: [] }));
    expect(await screen.findByText("Kimse yeni erişim kazanmayacak.")).toBeTruthy();
    await fireEvent.press(confirm());
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("counts several items and names an unknown person neutrally", async () => {
    await setup(() =>
      json(
        200,
        preview({
          items: [
            { item_id: "a", new_viewers: ["u9"] },
            { item_id: "b", new_viewers: ["u9"] },
          ],
          people: [{ id: "u9", display_name: null }],
        }),
      ),
    );
    expect(await screen.findByText("Bu işlemle şu kişiler 2 nesneyi görebilecek:")).toBeTruthy();
    expect(screen.getByText("Bir kişi")).toBeTruthy();
  });

  it("names the person being added from what the screen already knows", async () => {
    await setup(
      () =>
        json(
          200,
          preview({
            items: [{ item_id: "t1", new_viewers: ["u5"] }],
            people: [{ id: "u5", display_name: null }],
          }),
        ),
      {
        request: { action: "add_member", item_id: "t1", user_id: "u5", role: "watcher" },
        knownNames: { u5: "Can" },
      },
    );
    expect(await screen.findByText("Can")).toBeTruthy();
    expect(screen.queryByText("Bir kişi")).toBeNull();
  });

  it("can be cancelled without sending the action", async () => {
    const { onCancel, onConfirm } = await setup(() => json(200, preview()));
    await screen.findByText("Ayşe");
    await fireEvent.press(screen.getAllByRole("button", { name: "Vazgeç" }).at(-1)!);
    expect(onCancel).toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("asks nothing while closed", async () => {
    const { server } = await setup(() => json(200, preview()), { request: null });
    await act(async () => {
      await Promise.resolve();
    });
    expect(server.calls).toEqual([]);
    expect(screen.queryByText("Paylaşımı onayla")).toBeNull();
  });
});
