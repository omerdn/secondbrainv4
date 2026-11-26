import { type Page, expect, test } from "@playwright/test";

/**
 * Critical flow of the web editor (T2.5.7, PRD Y3, Y4): a page edited in two tabs at once, the
 * Y4 formats, and an offline tab that turns read-only and catches up when it is back.
 */

const editor = (page: Page) => page.getByTestId("collab-editor").locator(".ProseMirror");
const status = (page: Page) => page.getByTestId("collab-status");

async function live(page: Page) {
  await expect(status(page)).toHaveAttribute("data-status", "editable");
}

test("two tabs edit one page live; offline is read-only and catches up", async ({
  browser,
  context,
}) => {
  const first = await context.newPage();
  // A fresh local account (localhost only; e-mail goes to the log driver).
  await first.goto("/register");
  await first.getByLabel("Adın").fill("E2E");
  await first.getByLabel("E-posta").fill(`e2e-${Date.now()}@example.test`);
  await first.getByLabel("Şifre").fill("e2e correct horse battery");
  await first.getByRole("button", { name: "Kayıt ol" }).click();
  await first.waitForURL((url) => !url.pathname.startsWith("/register"));

  await first.goto("/notebooks");
  await first.getByRole("button", { name: "Defterim defterine sayfa ekle" }).click();
  await first.getByLabel("Ad", { exact: true }).fill("Ortak sayfa");
  await first.getByLabel("Ad", { exact: true }).press("Enter");
  await expect(first.getByRole("heading", { name: "Ortak sayfa" })).toBeVisible();
  await live(first);
  const url = first.url();

  // The second tab is its own browser context with the same session, so it stays online
  // while the first one goes offline.
  const other = await browser.newContext({ storageState: await context.storageState() });
  const second = await other.newPage();
  await second.goto(url);
  await live(second);

  // Typing in one tab shows up in the other.
  await editor(first).click();
  await first.keyboard.type("Merhaba");
  await expect(editor(second)).toContainText("Merhaba");
  await editor(second).click();
  await second.keyboard.press("End");
  await second.keyboard.type(" dünya");
  await expect(editor(first)).toContainText("Merhaba dünya");

  // Y4 formats from the toolbar.
  await first.keyboard.press("End");
  await first.keyboard.press("Enter");
  await first.getByRole("button", { name: "Başlık 2" }).click();
  await first.keyboard.type("Başlık");
  await first.keyboard.press("Enter");
  await first.getByRole("button", { name: "Yapılacaklar" }).click();
  await first.keyboard.type("Süt al");
  await first.keyboard.press("Enter");
  await first.keyboard.press("Enter");
  await first.getByRole("button", { name: "Alıntı" }).click();
  await first.keyboard.type("Söz");
  await first.keyboard.press("Enter");
  await first.keyboard.press("Enter");
  await first.getByRole("button", { name: "Tablo" }).click();
  await first.keyboard.type("Hücre");
  await expect(editor(second).locator("h2")).toHaveText("Başlık");
  await expect(editor(second).locator('ul[data-type="taskList"] li')).toContainText("Süt al");
  await expect(editor(second).locator("blockquote")).toContainText("Söz");
  await expect(editor(second).locator("table")).toContainText("Hücre");

  // The first tab loses its connection: read-only; the other keeps writing.
  await first.context().setOffline(true);
  await expect(status(first)).toHaveAttribute("data-status", "readonly");
  await expect(editor(first)).toHaveAttribute("contenteditable", "false");
  await editor(second).locator("p").first().click();
  await second.keyboard.press("End");
  await second.keyboard.type("!");
  // Back online: it resynchronises and can edit again.
  await first.context().setOffline(false);
  await live(first);
  await expect(editor(first)).toContainText("Merhaba dünya!");

  // Reloading shows what was stored (Y3).
  await second.reload();
  await expect(editor(second)).toContainText("Merhaba dünya!");
  await expect(editor(second).locator("table")).toContainText("Hücre");
});
