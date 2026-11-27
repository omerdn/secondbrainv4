import { expect, test } from "@playwright/test";

import { signUp } from "./helpers";

/** Trackers (T2.7.7, PRD K1, S3): an entry with a note becomes the item's status line. */
test("an entry with a note shows on the tracker and as its status line", async ({ page }) => {
  await signUp(page);
  await page.goto("/trackers");
  await page.getByRole("button", { name: "Yeni takip" }).click();
  await page.getByLabel("Ad", { exact: true }).fill("Gitar");
  await page.getByLabel("Birim").fill("dk");
  await page.getByLabel("Hedef değer").fill("30");
  await page.getByRole("button", { name: "Oluştur" }).click();
  await expect(page.getByRole("heading", { name: "Gitar" })).toBeVisible();

  await page.getByLabel("Değer (dk)").fill("20");
  await page.getByLabel("Not (isteğe bağlı)").fill("Barre'da kaldım");
  await page.getByRole("button", { name: "Kaydı ekle" }).click();
  await expect(page.getByText("20 dk / 30 dk")).toBeVisible();

  // The note reaches the process page through the outbox, a moment later; the card reads it.
  await expect(async () => {
    await page.goto("/trackers");
    await expect(page.getByTestId("status-line")).toContainText("Barre'da kaldım", {
      timeout: 2_000,
    });
  }).toPass({ timeout: 30_000 });
});
