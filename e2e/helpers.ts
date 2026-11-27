import { expect, type Page } from "@playwright/test";

/** A fresh local account (localhost only; e-mail goes to the log driver). */
export async function signUp(page: Page, name = "E2E"): Promise<void> {
  await page.goto("/register");
  await page.getByLabel("Adın").fill(name);
  await page
    .getByLabel("E-posta")
    .fill(`e2e-${Date.now()}-${Math.random().toString(36).slice(2, 8)}@example.test`);
  await page.getByLabel("Şifre").fill("e2e correct horse battery");
  // Sign-ups are rate limited (also locally): wait out "Çok fazla deneme" and try again.
  const limited = page.getByText(/Çok fazla deneme yaptın/);
  for (let attempt = 0; attempt < 20; attempt += 1) {
    await page.getByRole("button", { name: "Kayıt ol" }).click();
    const outcome = await Promise.race([
      page.waitForURL((url) => !url.pathname.startsWith("/register")).then(() => "in" as const),
      limited.waitFor().then(() => "limited" as const),
    ]);
    if (outcome === "in") return;
    await page.waitForTimeout(3_000);
  }
  throw new Error("sign-up stayed rate limited");
}

/** A task with one subtask, through the task form; ends on the task's screen. */
export async function createTask(
  page: Page,
  title: string,
  subtask: string,
  due?: string,
): Promise<void> {
  await page.goto("/tasks");
  await page.getByRole("button", { name: "Yeni görev" }).click();
  await page.getByLabel("Başlık", { exact: true }).fill(title);
  if (due !== undefined) await page.getByLabel("Bitiş (YYYY-AA-GG)").fill(due);
  await page.getByRole("button", { name: "Alt kırılım ekle" }).click();
  await page.getByLabel("Alt kırılım 1", { exact: true }).fill(subtask);
  await page.getByRole("button", { name: "Oluştur" }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
}

/** `YYYY-MM-DD` of today in this machine's zone, moved by `days`. */
export function localDay(days = 0): string {
  const date = new Date();
  date.setDate(date.getDate() + days);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}
