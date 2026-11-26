import { expect, test } from "@playwright/test";

/** The Today screen's critical flow (T2.7.2, PRD H1): a routine due today is marked from it. */
test("a routine due today is marked from the Today screen", async ({ page }) => {
  // A fresh local account (localhost only; e-mail goes to the log driver).
  await page.goto("/register");
  await page.getByLabel("Adın").fill("E2E");
  await page.getByLabel("E-posta").fill(`e2e-today-${Date.now()}@example.test`);
  await page.getByLabel("Şifre").fill("e2e correct horse battery");
  await page.getByRole("button", { name: "Kayıt ol" }).click();
  await page.waitForURL((url) => !url.pathname.startsWith("/register"));

  // A daily routine starting today (the form's defaults).
  await page.goto("/routines");
  await page.getByRole("button", { name: "Yeni rutin" }).click();
  await page.getByLabel("Ad", { exact: true }).fill("Su iç");
  await page.getByRole("button", { name: "Oluştur" }).click();
  await expect(page.getByRole("heading", { name: "Su iç" })).toBeVisible();

  await page.goto("/");
  await expect(page.getByText("Bugünün rutinleri")).toBeVisible();
  await expect(page.getByText("0 / 1")).toBeVisible();
  await page.getByRole("button", { name: "Su iç rutinini işaretle" }).click();
  await expect(page.getByText("1 / 1 · tamam")).toBeVisible();
  // The mark is on the server: a reload shows it.
  await page.reload();
  await expect(page.getByText("1 / 1 · tamam")).toBeVisible();
  await page.getByRole("button", { name: "Su iç işaretini geri al" }).click();
  await expect(page.getByText("0 / 1")).toBeVisible();
});
