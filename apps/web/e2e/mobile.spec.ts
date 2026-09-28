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

test("Today's evidence is a full-height sheet on a phone, and peers keep 44px actions", async ({ page }) => {
  await onboardViaApi("e2e-mobile-sheet");
  await signIn(page, "e2e-mobile-sheet");
  const lead = page.getByRole("list", { name: "Recommendations" }).getByRole("article").first();
  await lead.getByRole("button", { name: /^See evidence/ }).click();
  const sheet = page.getByRole("dialog", { name: "Why Narrow surfaced it" });
  await expect(sheet).toBeVisible();
  const box = (await sheet.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(box.width).toBeGreaterThanOrEqual(viewport.width - 1);
  expect(box.height).toBeGreaterThan(viewport.height * 0.8);
  await expectAccessible(page);
  await sheet.getByRole("button", { name: "Close" }).click();
  await expect(sheet).toBeHidden();
  const peer = page.getByRole("list", { name: "Also worth a look" }).getByRole("article").first();
  for (const name of ["Not now", "Not for me", "I applied", "Save"]) {
    expect((await peer.getByRole("button", { name }).boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test("Preferences work on a phone: summary rows, a focused editor, thumb-sized choices", async ({ page }) => {
  await onboardViaApi("e2e-mobile-prefs");
  await signIn(page, "e2e-mobile-prefs", "/preferences");
  const setup = page.getByRole("group", { name: "Work setup", exact: true });
  await expect(setup).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  const edit = setup.getByRole("button", { name: /^(Edit|Add) work setup$/ });
  expect((await edit.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  await edit.click();
  // Every choice is a 44px target.
  for (const option of ["Remote only", "Prefer remote", "No preference"]) {
    const box = await setup.locator("label").filter({ hasText: option }).boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  await setup.getByText("Remote only").click();
  await setup.getByRole("button", { name: "Save" }).click();
  await expect(setup.getByRole("status")).toHaveText(/Saved|Already in effect/);
  await expect(setup.getByText("Remote only")).toBeVisible();
  const team = page.getByRole("group", { name: "Team", exact: true });
  await team.getByRole("button", { name: /^(Edit|Add) team$/ }).click();
  const small = page.getByRole("group", { name: "Small team", exact: true });
  for (const option of ["Off", "Nice to have", "Must have"]) {
    const box = await small.locator("label").filter({ hasText: option }).boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  await expectAccessible(page);
});
