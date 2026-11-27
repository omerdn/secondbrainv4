import { expect, test } from "@playwright/test";

import { signUp } from "./helpers";

/** Work session (T2.7.7, PRD W1–W4): start, pause, stop, the segment log and closing. */
test("a work session runs a stopwatch segment and keeps its log", async ({ page }) => {
  await signUp(page);
  await page.goto("/session");
  await page.getByRole("button", { name: "Oturum başlat" }).click();
  await page.getByRole("radio", { name: "Kronometre" }).click();
  await page.getByRole("button", { name: "Başlat", exact: true }).click();
  await expect(page.getByRole("button", { name: "Duraklat" })).toBeVisible();
  // The clock counts up from the server's time and survives a reload.
  await page.waitForTimeout(2_000);
  await page.reload();
  await expect(page.getByLabel("Süre")).not.toHaveText("00:00");
  await page.getByRole("button", { name: "Duraklat" }).click();
  await expect(page.getByRole("button", { name: "Devam et" })).toBeVisible();
  await page.getByRole("button", { name: "Durdur" }).click();
  await expect(page.getByText(/dk çalışma$/)).toBeVisible();
  await page.getByRole("button", { name: "Oturumu kapat" }).click();
  await expect(page.getByRole("button", { name: "Oturum başlat" })).toBeVisible();
});
