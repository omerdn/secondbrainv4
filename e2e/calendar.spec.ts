import { expect, test } from "@playwright/test";

import { localDay, signUp } from "./helpers";

/** Calendar (T2.7.7, PRD C1): a task's due day and a routine's occurrences show in the views. */
test("a task's due day and a daily routine show on the calendar", async ({ page }) => {
  await signUp(page);
  await page.goto("/tasks");
  await page.getByRole("button", { name: "Yeni görev" }).click();
  await page.getByLabel("Başlık", { exact: true }).fill("Fatura öde");
  await page.getByLabel("Bitiş (YYYY-AA-GG)").fill(localDay());
  await page.getByRole("button", { name: "Oluştur" }).click();
  await expect(page.getByRole("heading", { name: "Fatura öde" })).toBeVisible();

  await page.goto("/routines");
  await page.getByRole("button", { name: "Yeni rutin" }).click();
  await page.getByLabel("Ad", { exact: true }).fill("Su iç");
  await page.getByRole("button", { name: "Oluştur" }).click();
  await expect(page.getByRole("heading", { name: "Su iç" })).toBeVisible();

  await page.goto("/calendar");
  await page.getByRole("radio", { name: "Gün" }).click();
  await expect(page.getByRole("link", { name: "Görev bitiyor: Fatura öde" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Rutin: Su iç" })).toBeVisible();
  await page.getByRole("radio", { name: "Hafta" }).click();
  await expect(page.getByRole("link", { name: "Rutin: Su iç" }).first()).toBeVisible();
});
