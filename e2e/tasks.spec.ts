import { expect, test } from "@playwright/test";

import { createTask, signUp } from "./helpers";

/** Tasks (T2.7.7, PRD G1–G6): create with a subtask, complete it, move the card on the board. */
test("a task is created, its subtask completed and its card moved", async ({ page }) => {
  await signUp(page);
  await createTask(page, "Raporu yaz", "Taslak");
  const subtask = page.getByRole("checkbox", { name: "Taslak" });
  await subtask.click();
  await expect(subtask).toBeChecked();
  await page.reload();
  await expect(page.getByRole("checkbox", { name: "Taslak" })).toBeChecked();

  await page.goto("/tasks?view=board");
  await page.getByLabel("Yeni sütun").fill("Yapılıyor");
  await page.getByRole("button", { name: "Sütun ekle" }).click();
  await expect(page.getByRole("heading", { name: "Yapılıyor (0)" })).toBeVisible();
  await page.getByRole("button", { name: "Taşı: Raporu yaz" }).click();
  await page.getByRole("button", { name: "Yapılıyor", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Yapılıyor (1)" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("heading", { name: "Yapılıyor (1)" })).toBeVisible();
});
