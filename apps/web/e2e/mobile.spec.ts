import { expect, test } from "@playwright/test";

import { expectAccessible, onboardViaApi, signIn } from "./helpers";

test("Today works on a phone: readable, thumb-sized actions, no sideways scrolling", async ({ page }) => {
  await onboardViaApi("e2e-mobile");
  await signIn(page, "e2e-mobile");
  const card = page.getByRole("list", { name: "Recommendations" }).getByRole("article").first();
  await expect(card).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  // Every action is a 44px target, without becoming a pill.
  for (const name of ["Not now", "Not for me", "I applied", "Save"]) {
    const box = await card.getByRole("button", { name }).boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  await card.getByRole("button", { name: "Save" }).click();
  await expect(card.getByRole("status")).toContainText("Saved");
  await page.getByRole("navigation", { name: "Main" }).getByRole("link", { name: "Applications" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Applications" })).toBeVisible();
  await expectAccessible(page);
});

test("on a phone, Settings is reachable and the opportunity page has its own action bar", async ({ page }) => {
  await onboardViaApi("e2e-mobile-2");
  await signIn(page, "e2e-mobile-2", "/profile");
  await page.getByRole("banner").getByRole("link", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Settings" })).toBeVisible();
  await page.goto("/today");
  const first = page.getByRole("list", { name: "Recommendations" }).getByRole("article").first();
  await first.getByRole("heading", { level: 2 }).getByRole("link").click();
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
  // No tab bar here: Save is at the bottom, within reach, and the way back is at the top.
  await expect(page.getByRole("navigation", { name: "Main" })).toHaveCount(0);
  const save = page.getByRole("button", { name: "Save" });
  const box = await save.boundingBox();
  const viewport = page.viewportSize()!;
  expect(box!.y + box!.height).toBeGreaterThan(viewport.height - 80);
  await expect(page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: /Today/ })).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  await expectAccessible(page);
});

test("Preferences work on a phone: stacked settings, thumb-sized choices, no sideways scrolling", async ({ page }) => {
  await onboardViaApi("e2e-mobile-prefs");
  await signIn(page, "e2e-mobile-prefs", "/preferences");
  const setup = page.getByRole("group", { name: "Work setup", exact: true });
  await expect(setup).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  // Every choice is a 44px target.
  for (const option of ["Remote only", "Prefer remote", "No preference"]) {
    const box = await setup.locator("label").filter({ hasText: option }).boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  const team = page.getByRole("group", { name: "Small team", exact: true });
  for (const option of ["Off", "Nice to have", "Must have"]) {
    const box = await team.locator("label").filter({ hasText: option }).boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  await setup.getByText("Remote only").click();
  await expect(setup.getByRole("status")).toHaveText(/Saved|Already in effect/);
  await expect(setup.getByRole("radio", { name: /Remote only/ })).toBeChecked();
  await expectAccessible(page);
});
