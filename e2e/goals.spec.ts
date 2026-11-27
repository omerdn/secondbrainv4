import { expect, test } from "@playwright/test";

import { createTask, localDay, signUp } from "./helpers";

/** Goals (T2.7.7, PRD D1–D6): a goal linked to a task follows the task's progress. */
test("a goal follows the progress of its linked task", async ({ page }) => {
  await signUp(page);
  // A task counts toward a goal through its due day in the goal's range.
  await createTask(page, "Rapor yaz", "Taslak", localDay(5));

  await page.goto("/goals");
  await page.getByRole("button", { name: "Yeni hedef" }).click();
  await page.getByLabel("Ad", { exact: true }).fill("Kitap yılı");
  await page.getByLabel("Başlangıç (YYYY-AA-GG)").fill(localDay(-1));
  await page.getByLabel("Bitiş (YYYY-AA-GG)").fill(localDay(30));
  await page.getByRole("button", { name: "Oluştur" }).click();
  await expect(page.getByRole("heading", { name: "Kitap yılı" })).toBeVisible();
  const goalUrl = page.url();

  await page.getByRole("button", { name: "Nesne bağla" }).click();
  await page.getByRole("button", { name: "Görev: Rapor yaz" }).click();
  await expect(page.getByRole("button", { name: "Rapor yaz bağlantısını kaldır" })).toBeVisible();
  await expect(page.getByText("Gerçekleşen: %0")).toBeVisible();

  // The task is done: the goal's actual progress follows.
  await page.goto("/tasks");
  await page.getByRole("button", { name: "Görev: Rapor yaz" }).click();
  await page.getByRole("checkbox", { name: "Taslak" }).click();
  await expect(page.getByRole("checkbox", { name: "Taslak" })).toBeChecked();
  await page.goto(goalUrl);
  await expect(page.getByText("Gerçekleşen: %100")).toBeVisible();
});
