import { expect, test } from "@playwright/test";

import { expectAccessible } from "./helpers";

test("signed-out visitors only reach sign-in", async ({ page }) => {
  for (const path of ["/today", "/applications", "/preferences", "/profile", "/settings"]) {
    await page.goto(path);
    await expect(page).toHaveURL(new RegExp(`/signin\\?next=${encodeURIComponent(path).replace(/[/]/g, "%2F")}`));
  }
  await expect(page.getByRole("heading", { name: "The few jobs worth your time." })).toBeVisible();
  await expectAccessible(page);
});

test("a tampered session cookie is not a session", async ({ page, context }) => {
  await context.addCookies([{ name: "jh_session", value: "not-a-real-session", url: "http://127.0.0.1:3100" }]);
  await page.goto("/today");
  await expect(page).toHaveURL(/\/signin/);
});
