import { expect, test } from "@playwright/test";

import { createTask, signUp } from "./helpers";

/** Search (T2.7.7, PRD AR1): the shortcut opens it; Turkish letters match; a result opens. */
test("search finds a task by a folded word and opens it", async ({ page }) => {
  await signUp(page);
  await createTask(page, "Işık tasarımı", "Lamba seç");
  await page.goto("/");
  await expect(page.getByText("Bugünün rutinleri")).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.getByLabel("Ne arıyorsun?").fill("isik");
  await page.getByRole("link", { name: "Görev: Işık tasarımı" }).click();
  await expect(page.getByRole("heading", { name: "Işık tasarımı" })).toBeVisible();
});
