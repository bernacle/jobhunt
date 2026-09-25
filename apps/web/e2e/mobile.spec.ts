import { expect, test } from "@playwright/test";

import { expectAccessible, onboardViaApi, signIn } from "./helpers";

test("Today works on a phone: readable cards, reachable actions, no sideways scrolling", async ({ page }) => {
  await onboardViaApi("e2e-mobile");
  await signIn(page, "e2e-mobile");
  const card = page.getByRole("list", { name: "Recommendations" }).getByRole("article").first();
  await expect(card).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  await card.getByRole("button", { name: "Save" }).click();
  await expect(card.getByRole("status")).toContainText("Saved");
  await page.getByRole("navigation", { name: "Main" }).getByRole("link", { name: "Applications" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Applications" })).toBeVisible();
  await expectAccessible(page);
});
